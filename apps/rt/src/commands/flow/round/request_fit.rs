//! O pedido de onda acima do teto de tokens: a rodada divide a onda sozinha.
//!
//! A formação do lote não tem teto de tarefas, e o pedido de uma onda grande
//! passava do teto de tokens ([`WAVE_REQUEST_TOKEN_CAP`]): a rodada inteira
//! era recusada, mandando dividir o lote sem comando nenhum para isso. Agora
//! a onda sai com as primeiras tarefas da ordem de execução que cabem no
//! teto, e as outras voltam ao backlog, sem onda — a mesma volta das tarefas
//! de um lote cortado ([`backlog_return`]) —, para a montagem da rodada
//! seguinte. A onda ganha uma versão com a ordem, os itens e o pronto-quando
//! só das tarefas que ficaram, pela mesma conta da formação do lote
//! ([`backlog_fields`]). Quanto cabe se mede no pedido de verdade, montado
//! sobre um ensaio das gravações ([`RecordCheck`]), sem gravar nada antes de
//! achar a divisão. A onda cuja primeira tarefa sozinha já passa do teto não
//! tem divisão: quem despacha continua recusando.

use std::collections::BTreeSet;
use std::path::Path;

use mustard_core::domain::spec_events::{BlockQuery, Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::domain::wave_prompt::{WAVE_REQUEST_TOKEN_CAP, backlog_fields};
use mustard_core::io::spec_events as store;
use mustard_core::io::wave_prompt::{Flight, WavePrompt, prompts};
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use super::queue::{task_revision, tasks_not_delivered};
use super::report::backlog_return;
use crate::commands::spec_events::task_dependents::TaskRefs;
use crate::commands::spec_events::write::{RecordCheck, record};

/// A leitura da spec depois da divisão e os pedidos montados de novo sobre
/// ela.
pub(super) type Refit = (SpecLog, Vec<WavePrompt>);

/// Divide cada onda de `next` cujo pedido montado em `built` passa do teto:
/// fica o maior começo da ordem de execução cujo pedido cabe, e o resto volta
/// ao backlog. `start` é de onde a rodada grava; `root`, o repositório
/// principal. O que gravou vai a `recorded`, e o aviso de cada onda dividida,
/// a `warnings`. Com alguma divisão, devolve a spec relida e os pedidos de
/// `outgoing` montados de novo sobre ela; sem nenhuma, `None`.
///
/// # Errors
///
/// A recusa da leitura da spec ou de uma gravação.
#[allow(clippy::too_many_arguments)]
pub(super) fn fit_requests(
    (start, root, spec): (&Path, &Path, &str),
    log: &SpecLog,
    lang: Locale,
    flight: &Flight,
    built: &[WavePrompt],
    (next, outgoing): (&[u64], &BTreeSet<u64>),
    recorded: &mut Vec<Value>,
    warnings: &mut Vec<Value>,
) -> Result<Option<Refit>, Refusal> {
    let mut divided = false;
    for wave in next {
        let Some(prompt) = built.iter().find(|prompt| prompt.wave == *wave) else {
            continue;
        };
        if prompt.tokens() <= WAVE_REQUEST_TOKEN_CAP {
            continue;
        }
        let tasks = execution_order(log, *wave);
        if tasks.len() < 2 {
            continue;
        }
        // O maior começo que cabe, por busca binária: cada tentativa monta o
        // pedido inteiro, e a onda grande tem dezenas de tarefas.
        let Some(mut best) = trial(start, root, spec, log, lang, flight, *wave, &tasks, 1)? else {
            continue;
        };
        let (mut low, mut high) = (1, tasks.len() - 1);
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            match trial(start, root, spec, log, lang, flight, *wave, &tasks, middle)? {
                Some(writes) => {
                    best = writes;
                    low = middle;
                }
                None => high = middle - 1,
            }
        }
        let (kept, returned, writes) = best;
        for (event_type, draft) in writes {
            let written = record(start, spec, event_type, draft, PhaseWriter::Binary)?;
            recorded.push(json!({ "wave": wave, "type": event_type, "id": written.written.id }));
        }
        divided = true;
        let codes = log.codes();
        let named: Vec<String> = returned.iter().map(|id| codes.get(id).cloned().unwrap_or_else(|| id.to_string())).collect();
        let hint = translate("round.request_split", lang)
            .replace("{wave}", &wave.to_string())
            .replace("{cap}", &WAVE_REQUEST_TOKEN_CAP.to_string())
            .replace("{kept}", &kept.to_string())
            .replace("{total}", &tasks.len().to_string())
            .replace("{tasks}", &named.join(", "));
        warnings.push(json!({ "reason": "wave-request-split", "wave": wave, "hint": hint }));
    }
    if !divided {
        return Ok(None);
    }
    let reread = store::read(&store::spec_file(root, spec)?)?.ok_or_else(|| Refusal::NoSpecFile { spec: spec.to_string() })?;
    let rebuilt = prompts(root, spec, &reread, lang, flight, outgoing);
    Ok(Some((reread, rebuilt)))
}

/// Quantas tarefas ficam na onda, as que voltam ao backlog e as gravações.
type Trial = (usize, Vec<u64>, Vec<(&'static str, Map<String, Value>)>);

/// As gravações que deixam na onda `wave` as `keep` primeiras tarefas de
/// `tasks` — menos a que depende de uma que volta — e devolvem as outras ao
/// backlog, quando o pedido montado sobre o ensaio delas cabe no teto. `None`
/// quando não cabe.
#[allow(clippy::too_many_arguments)]
fn trial(
    start: &Path,
    root: &Path,
    spec: &str,
    log: &SpecLog,
    lang: Locale,
    flight: &Flight,
    wave: u64,
    tasks: &[&SpecEvent],
    keep: usize,
) -> Result<Option<Trial>, Refusal> {
    let codes = log.codes();
    let refs = TaskRefs::new(log, &codes);
    let mut returned: BTreeSet<u64> = tasks[keep..].iter().map(|task| task.id).collect();
    let mut kept: Vec<&SpecEvent> = Vec::new();
    for task in &tasks[..keep] {
        if refs.depends_on(task).any(|id| returned.contains(&id)) {
            returned.insert(task.id);
        } else {
            kept.push(task);
        }
    }
    if kept.is_empty() {
        return Ok(None);
    }
    let Some(event) = log.block(BlockQuery::Wave(wave)).into_iter().find(|event| event.event_type == "wave") else {
        return Ok(None);
    };
    let kept_ids: BTreeSet<u64> = kept.iter().map(|task| task.id).collect();
    // A prova do item que uma tarefa que volta também cobre fica para a onda
    // dela, como na formação do lote.
    let waiting: BTreeSet<u64> = tasks_not_delivered(log, &BTreeSet::new())
        .into_iter()
        .filter(|id| !kept_ids.contains(id))
        .filter_map(|id| log.get(id))
        .flat_map(|task| task.ints("covers"))
        .collect();
    let commands = mustard_core::ProjectConfig::load(root).commands();
    let fields = backlog_fields(log, &kept, &commands, &waiting);
    let order: Vec<u64> = kept.iter().map(|task| task.id).collect();
    let mut revised = Map::from_iter([("order".to_string(), json!(order)), ("done_when".to_string(), json!(fields.done_when))]);
    if !fields.criteria.is_empty() {
        revised.insert("criteria".into(), json!(fields.criteria));
    }
    if event.str_field("author") == Some("binary") {
        revised.insert("text".into(), json!(fields.text));
    }
    let mut writes: Vec<(&'static str, Map<String, Value>)> = Vec::new();
    writes.extend(task_revision(log, event.id, revised).map(|draft| ("wave", draft)));
    writes.extend(tasks.iter().filter(|task| returned.contains(&task.id)).map(|task| ("task", backlog_return(task))));
    // A gravação que o ensaio recusa não divide a onda: a recusa do teto
    // continua com quem despacha.
    let mut check = RecordCheck::open(start, spec, PhaseWriter::Binary)?;
    for (event_type, draft) in &writes {
        if check.record(event_type, draft.clone()).is_err() {
            return Ok(None);
        }
    }
    let rebuilt = prompts(root, spec, check.log(), lang, flight, &BTreeSet::from([wave]));
    let fits = rebuilt.iter().find(|prompt| prompt.wave == wave).is_some_and(|prompt| prompt.tokens() <= WAVE_REQUEST_TOKEN_CAP);
    Ok(fits.then(|| (kept.len(), tasks.iter().map(|task| task.id).filter(|id| returned.contains(id)).collect(), writes)))
}

/// As tarefas da onda `wave` na ordem de execução que o pedido usa: as que a
/// onda lista em `order`, pelo código, que sobrevive às versões, e depois o
/// resto, na ordem do arquivo.
fn execution_order(log: &SpecLog, wave: u64) -> Vec<&SpecEvent> {
    let block = log.block(BlockQuery::Wave(wave));
    let codes = log.codes();
    let order: Vec<u64> = block.iter().find(|event| event.event_type == "wave").map(|event| event.ints("order")).unwrap_or_default();
    let tasks: Vec<&SpecEvent> = block.into_iter().filter(|event| event.event_type == "task").collect();
    let mut out: Vec<&SpecEvent> = Vec::new();
    for id in order {
        let same = |task: &&SpecEvent| task.id == id || codes.get(&id).is_some_and(|code| codes.get(&task.id) == Some(code));
        if let Some(task) = tasks.iter().copied().find(same)
            && !out.iter().any(|had| had.id == task.id)
        {
            out.push(task);
        }
    }
    for task in tasks {
        if !out.iter().any(|had| had.id == task.id) {
            out.push(task);
        }
    }
    out
}
