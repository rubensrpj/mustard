//! A fila da rodada e as ondas em andamento: quais ondas saem agora, a
//! escolha do pedido antes do envio, a cópia separada e a pasta de compilação
//! de cada uma, quais estão em andamento, quais já estão entregues e
//! aprovadas, e o estado de cada uma que a página mostra. A rodada não pede
//! revisão de onda nenhuma: quem confere o trabalho, uma vez por obra, é o
//! agente de teste dedicado que o fechamento pede.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Block, BlockQuery, EventRef, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::State;
use mustard_core::domain::spec_index::title_of;
use mustard_core::domain::wave_prompt::{candidates, dispatch_items, recorded_choice, Candidates, Choice, WaveCopy};
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::io::wave_prompt::{copy_path, lesson_bank, recorded_copy, shown, wave_lessons};
use mustard_core::platform::git;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

use super::report::{has_agent_lines, tagged};
use super::stops::waves_replanned;
use crate::commands::git_settle::{enter_unit_branch, submodule_holding, submodules_of};
use crate::commands::wave::wave_overlap_check::wave_graph;

/// Quantas ondas saem juntas quando o projeto não diz outra coisa: duas, que é
/// quanto a máquina aguenta compilando ao mesmo tempo.
const DEFAULT_PARALLEL: usize = 2;

/// Quantas ondas o projeto deixa compilar ao mesmo tempo.
pub(super) fn max_parallel(root: &Path) -> usize {
    mustard_core::ProjectConfig::load(root).max_compiling_waves().unwrap_or(DEFAULT_PARALLEL)
}

/// As ondas que saem nesta rodada: as que ainda não saíram nem entregaram,
/// cujas dependências já foram entregues, no máximo `limit` junto com as que
/// estão em andamento (`running`). Duas ondas que declaram o mesmo arquivo
/// saem juntas: cada uma trabalha na sua cópia, e a volta junta os arquivos.
/// A onda em andamento conta como uma que já saiu nesta rodada e ocupa uma
/// vaga. A onda parada pelo limite de consertos (`stuck`) não sai, nem a que
/// depende dela, direta ou por outra onda.
pub(super) fn next_waves(
    log: &SpecLog,
    limit: usize,
    running: &BTreeMap<u64, u64>,
    stuck: &BTreeMap<u64, Vec<&SpecEvent>>,
) -> Vec<u64> {
    let graph = wave_graph(log);
    // A onda reprovada volta para a fila: sem isso o ciclo de conserto não
    // fecha, porque o fechamento recusa e diz qual refazer e a rodada nunca a
    // despacharia de novo.
    let to_redo = waves_to_redo(log);
    // O pedido gravado descreve o plano daquele momento: a onda que ganhou
    // versão nova depois dele, e ainda não entregou, sai de novo com o pedido
    // do plano atual.
    let replanned = waves_replanned(log);
    // O que já saiu da fila: a onda com pedido e também a onda que já
    // entregou. Só o pedido não basta, porque a onda entregue antes de a
    // rodada existir não tem pedido nenhum e apareceria como pronta para
    // sair — e sairia de novo um trabalho já feito.
    // Quais ondas já entregaram é pergunta do núcleo, e é ele que responde:
    // recalcular o filtro aqui deixava duas camadas decidindo a mesma coisa,
    // concordando hoje e livres para divergir amanhã.
    let delivered = log.delivered_waves();
    let already_out: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "send")
        .filter_map(SpecEvent::wave)
        .filter(|n| !replanned.contains(n))
        .chain(delivered.iter().copied())
        .filter(|n| !to_redo.contains(n))
        .collect();
    let mut depends: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for wave in log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "wave") {
        if let Some(n) = wave.wave() {
            depends.insert(n, wave.ints("depends_on"));
        }
    }
    let done = waves_done(log, running);
    let slots = limit.saturating_sub(running.len());
    ready_in_order(&graph, &depends, &already_out, &delivered, &done)
        .into_iter()
        .filter(|n| !stuck.contains_key(n) && !dependencies_of(*n, &depends).iter().any(|d| stuck.contains_key(d)))
        .take(slots)
        .collect()
}

// ---------------------------------------------------------------------------
// A escolha do pedido antes do envio
// ---------------------------------------------------------------------------

/// A linha em que o orquestrador devolve a escolha de uma onda.
const ANALYSIS_LINE: &str = "ANALYSIS";

/// O que a linha `ANALYSIS` de uma onda trouxe: cada entrada que sai e cada
/// uma que entra, como veio — o item pelo código ou pelo número em `item`, a
/// lição pelo número do banco em `lesson` —, com o motivo.
pub(super) struct AnalysisLine {
    wave: u64,
    removed: Vec<(Value, String)>,
    added: Vec<(Value, String)>,
}

/// `true` quando o relatório só traz a escolha antes do envio: não há entrega
/// nem veredito a juntar, e a rodada vai direto ao despacho.
pub(super) fn only_analysis(raw: &str) -> bool {
    !has_agent_lines(raw) && !tagged(raw, ANALYSIS_LINE).is_empty()
}

/// As linhas `ANALYSIS` do relatório `raw`. A que não se lê fica de fora com
/// um aviso, e a onda dela pede a escolha de novo: nada é recusado.
pub(super) fn analysis_lines(raw: Option<&str>, lang: Locale) -> (Vec<AnalysisLine>, Vec<Value>) {
    let mut lines = Vec::new();
    let mut warnings = Vec::new();
    for body in raw.map(|raw| tagged(raw, ANALYSIS_LINE)).unwrap_or_default() {
        let parsed = serde_json::from_str::<Value>(body).map_err(|e| e.to_string());
        match parsed.as_ref().ok().and_then(|fields| Some((fields, fields.get("wave")?.as_u64()?))) {
            Some((fields, wave)) => {
                let entries = |key: &str| -> Vec<(Value, String)> {
                    let listed = fields.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
                    listed
                        .iter()
                        .map(|entry| {
                            let why = entry.get("why").and_then(Value::as_str).map(str::trim).unwrap_or_default();
                            (entry.clone(), why.to_string())
                        })
                        .collect()
                };
                lines.push(AnalysisLine { wave, removed: entries("removed"), added: entries("added") });
            }
            None => {
                let detail = parsed.err().unwrap_or_else(|| body.to_string());
                let hint = translate("round.analysis_unreadable", lang).replace("{detail}", &detail);
                warnings.push(json!({ "reason": "analysis-unreadable", "hint": hint }));
            }
        }
    }
    (lines, warnings)
}

/// O que a escolha antes do envio decidiu para as ondas prontas.
pub(super) struct Analysed {
    /// As ondas que saem agora, na ordem da fila.
    pub go: Vec<u64>,
    /// A escolha de cada onda que sai com uma.
    pub choices: BTreeMap<u64, Choice>,
    /// Os candidatos de cada onda que espera a escolha do orquestrador.
    pub asked: Vec<Value>,
    /// As entradas da linha da escolha que ficaram como estavam.
    pub warnings: Vec<Value>,
}

/// A escolha antes do envio de cada onda pronta (`ready`), antes de a cópia
/// dela ser criada. Os candidatos de cada onda são os itens do projeto todo,
/// os sem dono e as lições do banco que casam com ela; a onda sem nenhum sai
/// como hoje. A que tem sai com a escolha que a linha `ANALYSIS` do relatório
/// trouxe (`given`) ou, quando a mesma onda sai de novo sem plano novo, com a
/// escolha gravada no envio anterior dela, se essa escolha julgou cada
/// candidato de agora. Sem escolha, a onda não sai, e a resposta traz os
/// candidatos dela ao orquestrador, cada um com o título. Nada é recusado, e
/// nenhum agente é aberto para isso.
pub(super) fn analyse(root: &Path, log: &SpecLog, ready: &[u64], given: &[AnalysisLine], lang: Locale) -> Analysed {
    let replanned = waves_replanned(log);
    let codes = log.codes();
    let bank = lesson_bank(root);
    let mut out = Analysed { go: Vec::new(), choices: BTreeMap::new(), asked: Vec::new(), warnings: Vec::new() };
    for wave in ready.iter().copied() {
        let mut found = candidates(log, wave);
        found.lessons = bank.as_ref().map(|bank| wave_lessons(bank, log, wave)).unwrap_or_default();
        if found.is_empty() {
            out.go.push(wave);
            continue;
        }
        let choice = match given.iter().rfind(|line| line.wave == wave) {
            Some(line) => Some(chosen(log, &codes, line, &found, lang, &mut out.warnings)),
            None => recorded_choice(log, wave)
                .filter(|choice| !replanned.contains(&wave) && choice.covers(&found))
                .map(|choice| choice.within(&found)),
        };
        match choice {
            Some(choice) => {
                out.choices.insert(wave, choice);
                out.go.push(wave);
            }
            None => out.asked.push(shown_candidates(log, &codes, wave, &found)),
        }
    }
    out
}

/// A escolha que a linha da onda traz, dentro dos candidatos de agora
/// (`found`): sai só o item do projeto todo ou a lição, entra só o item sem
/// dono, e cada um com o motivo. A entrada fora dos candidatos, ou sem
/// motivo, fica como estava, e um aviso diz qual.
fn chosen(
    log: &SpecLog,
    codes: &BTreeMap<u64, String>,
    line: &AnalysisLine,
    found: &Candidates,
    lang: Locale,
    warnings: &mut Vec<Value>,
) -> Choice {
    // A entrada aponta um item da spec em `item` ou uma lição do banco em
    // `lesson`: os números dos dois não se misturam.
    let find = |group: &[&SpecEvent], entry: &Value, lesson: bool| -> Option<u64> {
        let id = if lesson {
            entry.get("lesson")?.as_u64()?
        } else {
            match EventRef::from_value(entry.get("item")?)? {
                EventRef::Id(n) => log.current(n)?.id,
                EventRef::Code(code) => group.iter().find(|e| codes.get(&e.id) == Some(&code))?.id,
            }
        };
        group.iter().any(|e| e.id == id).then_some(id)
    };
    let mut pick = |group: &[&SpecEvent], entries: &[(Value, String)], lesson: bool| -> Vec<(u64, String)> {
        let mut out: Vec<(u64, String)> = Vec::new();
        for (entry, why) in entries.iter().filter(|(entry, _)| entry.get("lesson").is_some() == lesson) {
            match find(group, entry, lesson).filter(|_| !why.is_empty()) {
                Some(id) => {
                    if !out.iter().any(|(had, _)| *had == id) {
                        out.push((id, why.clone()));
                    }
                }
                None => warnings.push(ignored(line.wave, entry, lang)),
            }
        }
        out
    };
    let removed = pick(&found.project, &line.removed, false);
    let removed_lessons = pick(&found.lessons, &line.removed, true);
    let added = pick(&found.unowned, &line.added, false);
    // Lição não entra por escolha: as que casam com a onda já vão, e a lição
    // em `added` fica como estava.
    for (entry, _) in line.added.iter().filter(|(entry, _)| entry.get("lesson").is_some()) {
        warnings.push(ignored(line.wave, entry, lang));
    }
    Choice { judged: found.ids(), removed, added, judged_lessons: found.lesson_ids(), removed_lessons }
}

/// O aviso da entrada da linha da escolha que ficou como estava.
fn ignored(wave: u64, entry: &Value, lang: Locale) -> Value {
    let said = entry.get("item").or_else(|| entry.get("lesson")).unwrap_or(entry);
    let shown = said.as_str().map_or_else(|| said.to_string(), str::to_string);
    let hint = translate("round.analysis_ignored", lang).replace("{wave}", &wave.to_string()).replace("{item}", &shown);
    json!({ "reason": "analysis-item-ignored", "wave": wave, "hint": hint })
}

/// Os candidatos da onda `wave`, como a resposta da rodada os entrega ao
/// orquestrador: o título da onda e, em cada grupo, cada candidato com o
/// título dele — o item pelo código, a lição pelo número do banco. O texto
/// inteiro se lê pelo código.
fn shown_candidates(log: &SpecLog, codes: &BTreeMap<u64, String>, wave: u64, found: &Candidates) -> Value {
    let title = |e: &SpecEvent| title_of(e).unwrap_or_default();
    let items = |group: &[&SpecEvent]| -> Vec<Value> {
        let code = |e: &SpecEvent| codes.get(&e.id).cloned().unwrap_or_else(|| e.id.to_string());
        group.iter().map(|e| json!({ "item": code(e), "title": title(e) })).collect()
    };
    let named = log.block(BlockQuery::Wave(wave)).into_iter().find(|e| e.event_type == "wave").map(title);
    json!({
        "wave": wave,
        "title": named.unwrap_or_default(),
        "project": items(&found.project),
        "unowned": items(&found.unowned),
        "lessons": found.lessons.iter().map(|e| json!({ "lesson": e.id, "title": title(e) })).collect::<Vec<_>>(),
    })
}

/// As pastas de compilação fixas do checkout `root`, uma por vaga do limite
/// de compilações, dentro da pasta de compilação do projeto. Elas passam de
/// uma cópia para a seguinte, e a compilação de uma aproveita a da anterior.
fn build_dirs(root: &Path, count: usize) -> Vec<PathBuf> {
    let base = root.join("target").join("copias");
    (0..count)
        .map(|slot| match u8::try_from(slot).ok().filter(|n| *n < 26) {
            Some(n) => base.join(char::from(b'a' + n).to_string()),
            None => base.join((slot + 1).to_string()),
        })
        .collect()
}

/// As cópias das ondas `waves`, que saem agora, cada uma com uma pasta de
/// compilação livre: a pasta que nenhuma onda em andamento (`running`) usa.
/// Cada cópia sai do commit atual; a que já existe, de um envio anterior da
/// mesma onda, é a mesma, e ela traz cada submódulo que as tarefas da onda
/// tocam. A onda cuja cópia não pôde ser criada não sai, e o aviso diz por
/// quê; a onda sem pasta livre também não sai, e fica para a rodada seguinte.
/// Roda com a trava do passo do git que o despacho já prendeu (`_held`): duas
/// rodadas ao mesmo tempo não criam a mesma cópia duas vezes.
pub(super) fn open_copies(
    root: &Path,
    spec: &str,
    log: &SpecLog,
    _held: &LockedFile,
    waves: &[u64],
    running: &BTreeMap<u64, u64>,
    lang: Locale,
) -> (BTreeMap<u64, WaveCopy>, Vec<Value>) {
    let dir_of = |n: &u64| recorded_copy(log, *n).and_then(|copy| copy.build_dir);
    let held: BTreeSet<String> = running.keys().filter_map(dir_of).collect();
    let mut free: Vec<String> =
        build_dirs(root, max_parallel(root)).iter().map(|dir| shown(dir)).filter(|dir| !held.contains(dir)).collect();

    let mut copies = BTreeMap::new();
    let mut warnings = Vec::new();
    let failed = |wave: u64, detail: String| {
        let hint = translate("round.copy_failed", lang).replace("{wave}", &wave.to_string()).replace("{detail}", &detail);
        json!({ "reason": "copy-not-created", "wave": wave, "hint": hint })
    };
    let head = git::run(root, &["rev-parse", "HEAD"]).result();
    let subs = submodules_of(root);
    let files = if subs.is_empty() { BTreeMap::new() } else { wave_graph(log).files };
    let unit = State::from_log(log).branch.unwrap_or_default();
    for wave in waves.iter().copied() {
        if free.is_empty() {
            break;
        }
        let path = copy_path(root, spec, wave, false);
        let touched: BTreeSet<&str> = files
            .get(&wave)
            .into_iter()
            .flatten()
            .filter_map(|file| submodule_holding(&subs, file).map(|(sub, _)| sub))
            .collect();
        let made = match &head {
            Err(detail) => Err(detail.clone()),
            Ok(head) => ensure_copy(root, &path, head)
                .and_then(|()| touched.iter().try_for_each(|sub| copy_submodule(root, &path, sub, &unit))),
        };
        match made {
            Ok(()) => {
                copies.insert(wave, WaveCopy { path: shown(&path), build_dir: Some(free.remove(0)) });
            }
            Err(detail) => warnings.push(failed(wave, detail)),
        }
    }
    (copies, warnings)
}

/// A cópia em `path`, criada no commit `head` do checkout `root`. A pasta que
/// já é uma cópia ligada ao repositório fica como está: é a de um envio
/// anterior da mesma onda.
fn ensure_copy(root: &Path, path: &Path, head: &str) -> Result<(), String> {
    if path.join(".git").is_file() {
        return Ok(());
    }
    let target = path.to_string_lossy();
    git::run(root, &["worktree", "add", "--detach", &target, head]).result().map(|_| ())
}

/// A cópia do submódulo `sub` dentro da cópia `copy`: o submódulo do
/// repositório principal entra na branch `unit` da spec, criada na primeira
/// vez sobre a base dele, e a cópia dele sai do commit em que ele fica. A que
/// já existe, de um envio anterior da mesma onda, é a mesma.
fn copy_submodule(root: &Path, copy: &Path, sub: &str, unit: &str) -> Result<(), String> {
    let inner = copy.join(sub);
    if inner.join(".git").is_file() {
        return Ok(());
    }
    let repo = root.join(sub);
    enter_unit_branch(&repo, unit)?;
    let target = inner.to_string_lossy();
    git::run(&repo, &["worktree", "add", "--detach", &target, "HEAD"]).result().map(|_| ())
}

/// As ondas em andamento, cada uma com o número do pedido dela: a onda tem
/// pedido e nenhuma entrega depois dele. O pedido mais antigo que a versão
/// mais nova da onda ou de uma tarefa dela descreve um plano que já mudou, e
/// não conta. Uma onda que já entregou só volta a sair por uma reprovação: o
/// pedido que veio depois de uma entrega, sem reprovação entre as duas, não é
/// trabalho em curso, nem o pedido de uma onda que saiu do plano.
pub(crate) fn waves_in_progress(log: &SpecLog) -> BTreeMap<u64, u64> {
    let planned = log.planned_waves();
    let replanned = waves_replanned(log);
    let verdicts = log.verdicts_by_wave();
    let mut deliveries: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for delivered in log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "delivered") {
        if let Some(n) = delivered.wave() {
            deliveries.entry(n).or_default().push(delivered.id);
        }
    }
    log.last_by_wave("send")
        .into_iter()
        .filter(|(n, _)| planned.contains(n) && !replanned.contains(n))
        .filter(|(n, sent)| {
            let ids = deliveries.get(n).map(Vec::as_slice).unwrap_or_default();
            if ids.iter().any(|id| id > sent) {
                return false;
            }
            let judged_before = verdicts
                .get(n)
                .and_then(|list| list.iter().rev().find(|v| v.id < *sent))
                .and_then(|v| v.str_field("result"));
            !ids.iter().any(|id| id < sent) || judged_before == Some("rejected")
        })
        .collect()
}

/// As ondas entregues e aprovadas: têm entrega, não estão em andamento, e a
/// última revisão delas — quando há uma, do agente de teste dedicado que o
/// fechamento pede — não reprovou. A onda entregue antes de a rodada existir,
/// sem pedido e sem veredito, está provada pelo código que entrou. A rodada
/// não pede revisão de onda nenhuma: a entrega já basta, e só uma reprovação
/// devolve a onda para a fila.
fn waves_done(log: &SpecLog, running: &BTreeMap<u64, u64>) -> BTreeSet<u64> {
    let rejected = log.last_rejected();
    log.delivered_waves().into_iter().filter(|n| !running.contains_key(n) && !rejected.contains_key(n)).collect()
}

/// A primeira onda planejada que ainda não está entregue e aprovada, com as
/// em andamento em `running`. `None` quando todas estão.
pub(super) fn first_unfinished(log: &SpecLog, running: &BTreeMap<u64, u64>) -> Option<u64> {
    let done = waves_done(log, running);
    log.planned_waves().into_iter().find(|n| !done.contains(n))
}

/// A onda `n` depende de todas as outras que ainda não terminaram: as
/// dependências dela, diretas ou por outra onda, alcançam cada onda planejada
/// que não está entregue e aprovada. É a última onda da obra.
fn depends_on_all(n: u64, depends: &BTreeMap<u64, Vec<u64>>, done: &BTreeSet<u64>) -> bool {
    let reached = dependencies_of(n, depends);
    depends.keys().filter(|w| **w != n && !done.contains(w)).all(|w| reached.contains(w))
}

/// As ondas de que `n` depende, diretas ou por outra onda.
fn dependencies_of(n: u64, depends: &BTreeMap<u64, Vec<u64>>) -> BTreeSet<u64> {
    let mut reached: BTreeSet<u64> = BTreeSet::new();
    let mut stack: Vec<u64> = depends.get(&n).cloned().unwrap_or_default();
    while let Some(on) = stack.pop() {
        if reached.insert(on) {
            stack.extend(depends.get(&on).cloned().unwrap_or_default());
        }
    }
    reached
}

/// As ondas que voltam para a fila: a última revisão delas reprovou, e o
/// conserto ainda não saiu — o pedido mais novo da onda e a entrega mais nova
/// dela são anteriores a essa reprovação. Depois que o conserto sai, a onda
/// espera a revisão dele, e não é despachada de novo pela mesma reprovação.
/// O pedido do conserto que o plano da onda deixou para trás, por uma versão
/// nova da onda ou de uma tarefa dela, conta como se não existisse: a onda
/// volta para a fila e sai com o pedido do plano atual.
///
/// O fechamento lê daqui também: uma onda cuja última revisão reprovou, mas
/// que já recebeu o conserto e entregou de novo, sai daqui — mesmo sem
/// veredito novo — porque o que falta agora é o agente de teste dedicado
/// conferir o conserto, não a rodada despachar de novo.
pub(crate) fn waves_to_redo(log: &SpecLog) -> BTreeSet<u64> {
    let last_send = log.last_by_wave("send");
    let replanned = waves_replanned(log);
    let delivered = log.last_by_wave("delivered");
    log.last_rejected()
        .into_iter()
        .filter(|(n, id)| replanned.contains(n) || last_send.get(n).is_none_or(|sent| sent < id))
        .filter(|(n, id)| delivered.get(n).is_none_or(|fix| fix < id))
        .map(|(n, _)| n)
        .collect()
}

/// As ondas prontas para sair, em ordem de nível e de número. `already_out`
/// são as ondas que já saíram da fila e `delivered` as que já entregaram, que
/// é o que solta as ondas dependentes delas. A onda que depende de todas as
/// outras só sai com as dependências entregues e aprovadas (`done`).
fn ready_in_order(
    graph: &crate::commands::wave::wave_overlap_check::WaveGraph,
    depends: &BTreeMap<u64, Vec<u64>>,
    already_out: &BTreeSet<u64>,
    delivered: &BTreeSet<u64>,
    done: &BTreeSet<u64>,
) -> Vec<u64> {
    let mut ready: Vec<(u32, u64)> = depends
        .iter()
        .filter(|(n, _)| !already_out.contains(n))
        .filter(|(n, on)| {
            let released = if depends_on_all(**n, depends, done) { done } else { delivered };
            on.iter().all(|d| released.contains(d))
        })
        .map(|(n, _)| (graph.level.get(n).copied().unwrap_or(0), *n))
        .collect();
    ready.sort_unstable();
    ready.into_iter().map(|(_, n)| n).collect()
}

/// Os itens que o pedido de uma onda leva: os números de tudo que entrou
/// nele, com a escolha do orquestrador antes do envio (`choice`).
pub(super) fn sent_items(log: &SpecLog, wave: u64, choice: Option<&Choice>) -> Vec<u64> {
    dispatch_items(log, wave, choice).into_iter().map(|e| e.id).collect()
}

/// O estado de cada onda que já saiu, pela mesma leitura que decide o que a
/// rodada despacha: em andamento, reprovada na última revisão ou entregue e
/// aprovada — a rodada não pede revisão de onda nenhuma, então a entrega já
/// vale como aprovada. A onda que não está aqui está por fazer. A página da
/// spec mostra este estado.
pub(crate) fn wave_states(log: &SpecLog) -> mustard_core::view::document::WaveStates {
    use mustard_core::view::document::WaveState;
    let running = waves_in_progress(log);
    let rejected = log.last_rejected();
    let done = waves_done(log, &running);
    let mut states = mustard_core::view::document::WaveStates::new();
    for n in running.keys().chain(rejected.keys()).chain(&done) {
        let state = if running.contains_key(n) {
            WaveState::Running
        } else if rejected.contains_key(n) {
            WaveState::Rejected
        } else {
            WaveState::Approved
        };
        states.insert(*n, state);
    }
    states
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use mustard_core::io::spec_events as store;
    use tempfile::tempdir;

    use super::*;
    use crate::commands::flow::round::tests::*;

    /// O envio gravado da onda `wave`: a cópia e a pasta de compilação dele.
    fn sent_copy(root: &Path, wave: u64) -> (String, String) {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let copy = mustard_core::io::wave_prompt::recorded_copy(&log, wave).unwrap_or_else(|| panic!("wave {wave}"));
        (copy.path, copy.build_dir.unwrap_or_default())
    }

    /// Duas ondas sem dependência que mexem no mesmo arquivo saem juntas,
    /// cada uma na sua cópia, criada no commit atual, e com a sua pasta de
    /// compilação, uma das fixas do projeto, porque a pasta é também a vaga
    /// das ondas que rodam juntas. O pedido de cada uma traz a cópia; a pasta,
    /// com o nome do Cargo, só quando o mapa marca o projeto como Rust, e não
    /// num projeto Node. O teto de compilações do projeto limita quantas
    /// saem.
    #[test]
    fn two_waves_on_the_same_file_go_out_together_each_in_its_own_copy_and_the_cap_holds() {
        for (kind, cites) in [("npm", false), ("cargo", true)] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[]), (3, &["src/c.rs"], &[])]);
            mapped(root, kind);

            let out = round(root, "x", None);
            assert_eq!(waves_in(&out, "dispatch"), vec![1, 2], "a onda 2 divide arquivo com a 1 e sai junto: {out}");
            let head = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(root).output().unwrap();
            let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
            let target = mustard_core::io::wave_prompt::shown(&root.join("target").join("copias"));
            let mut folders = Vec::new();
            for (at, wave) in [1_u64, 2].iter().enumerate() {
                let (copy, build) = sent_copy(root, *wave);
                let expected = mustard_core::io::wave_prompt::copy_path(root, "x", *wave, false);
                assert_eq!(copy, mustard_core::io::wave_prompt::shown(&expected), "{out}");
                assert!(expected.join(".git").is_file(), "the copy of wave {wave} is a linked checkout");
                assert_eq!(std::fs::read_to_string(expected.join("src/a.rs")).unwrap(), "fn um() {}\n");
                let copy_head = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(&expected).output().unwrap();
                assert_eq!(String::from_utf8_lossy(&copy_head.stdout).trim(), head, "the copy stands on the current commit");
                assert!(build.starts_with(&target), "{build}");
                let prompt = out["dispatch"][at]["prompt"].as_str().unwrap_or_default();
                assert!(prompt.contains(&format!("`{copy}`")), "{prompt}");
                assert_eq!(prompt.contains(&format!("={build}`")), cites, "{kind}: {prompt}");
                assert_eq!(prompt.contains("Cargo") || prompt.contains("target/copias"), cites, "{kind}: {prompt}");
                folders.push(build);
            }
            assert_eq!(folders, [format!("{target}/a"), format!("{target}/b")], "{kind}: each copy gets its own folder");
        }

        // Com o teto do projeto em 1, só uma onda sai por rodada.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        let out = round(root, "x", None);
        assert_eq!(out["dispatch"].as_array().map(Vec::len), Some(1), "{out}");
    }

    /// A onda que já entregou não é despachada de novo, mesmo sem pedido
    /// nenhum: a onda entregue antes de a rodada existir não tem pedido, e
    /// mandá-la sair seria mandar refazer um trabalho já feito.
    #[test]
    fn a_wave_that_already_delivered_does_not_go_out_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        write(
            root,
            "x",
            "delivered",
            json!({"wave": 1, "text": "A onda 1 saiu antes da rodada.", "files": ["src/a.rs"]}),
        );

        let out = round(root, "x", None);
        let waves: Vec<u64> =
            out["dispatch"].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect();
        assert_eq!(waves, vec![2], "a onda 1 já tem entrega: {out}");
    }

    /// A onda cuja última revisão reprovou volta a ser despachada, e uma vez
    /// só: depois que o conserto sai, a mesma reprovação não a manda de novo.
    /// Entregue o conserto, ele para de estar em andamento e de sair de novo
    /// — falta o agente de teste dedicado, no fechamento, dizer se ficou
    /// certo, e a rodada não pede revisão nenhuma dele.
    #[test]
    fn a_rejected_wave_goes_out_again_and_only_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(first["dispatch"].as_array().map(Vec::len), Some(1), "{first}");

        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        let again = round(root, "x", Some(&verdict(1, "rejected", "faltou o teste")));
        assert_eq!(again["dispatch"].as_array().map(Vec::len), Some(1), "a onda reprovada volta a sair: {again}");

        let quiet = round(root, "x", None);
        assert_eq!(quiet["dispatch"], json!([]), "o conserto já saiu: {quiet}");

        let back = round(root, "x", Some(&delivered(root, 1, "O teste entrou.", &["src/a.rs"])));
        assert_eq!(back["ok"], json!(true), "{back}");
        assert_eq!(back["dispatch"], json!([]), "o conserto não sai de novo: {back}");
        assert!(back.get("reviews").is_none(), "a rodada não pede revisão do conserto: {back}");
    }

    /// A onda reprovada cujo conserto já saiu e que ganha uma tarefa nova
    /// depois desse envio volta para a fila, como se o envio não existisse: a
    /// rodada seguinte a despacha de novo, com a tarefa nova no pedido, e ela
    /// fica em andamento com o envio novo; a rodada depois dessa não a solta
    /// outra vez.
    #[test]
    fn a_rejected_wave_that_gets_a_new_task_after_its_fix_went_out_goes_out_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        let fix = round(root, "x", Some(&verdict(1, "rejected", "faltou o teste")));
        assert_eq!(waves_in(&fix, "dispatch"), vec![1], "{fix}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let said = log.visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id).unwrap();
        let task = write(root, "x", "task", json!({"wave": 1, "text": "Tarefa nova da onda 1.",
            "files": [{"path": "src/b.rs"}], "origin": said}));
        let task_code = task["code"].as_str().unwrap_or_else(|| panic!("{task}")).to_string();

        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![1], "the replanned fix goes out again: {again}");
        let prompt = again["dispatch"][0]["prompt"].as_str().unwrap_or_default();
        let mut waves_lines = prompt.lines().filter_map(|l| l.strip_prefix("- `waves`: "));
        assert!(waves_lines.any(|codes| codes.split(", ").any(|code| code == task_code)), "{prompt}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let newest = log.visible().into_iter().rfind(|e| e.event_type == "send").map(|e| codes[&e.id].clone()).unwrap();
        assert_eq!(again["running"], json!([{"wave": 1, "send": newest}]), "{again}");

        let quiet = round(root, "x", None);
        assert_eq!(waves_in(&quiet, "dispatch"), Vec::<u64>::new(), "{quiet}");
        assert_eq!(waves_in(&quiet, "running"), vec![1], "{quiet}");
    }

    /// O estado de cada onda que a página mostra acompanha a rodada: em
    /// andamento depois do pedido, aprovada assim que entrega — a rodada não
    /// pede revisão nenhuma —, o que já solta a onda seguinte, reprovada por
    /// quem julgar (o agente de teste dedicado, no fechamento), em andamento
    /// de novo com o conserto e aprovada no fim; a onda que ainda não saiu
    /// não aparece, e a página a mostra por fazer.
    #[test]
    fn the_wave_states_follow_the_round() {
        use mustard_core::view::document::WaveState::{Approved, Rejected, Running};
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1])]);
        let events = root.join(".claude/spec/x/spec.ndjson");
        let states = || {
            let log = mustard_core::io::spec_events::read(&events).unwrap().unwrap();
            wave_states(&log).into_iter().collect::<Vec<_>>()
        };
        assert_eq!(states(), [], "nothing went out yet");
        round(root, "x", None);
        assert_eq!(states(), [(1, Running)]);
        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(states(), [(1, Approved), (2, Running)], "the delivery already frees the next wave");

        // A reprovação, que só o agente de teste dedicado grava, no
        // fechamento.
        let rejected = json!({"author": "review", "final": true, "wave": 1, "result": "rejected", "text": "faltou o teste"});
        mustard_core::io::spec_events::write(&events, "verdict", rejected.as_object().cloned().unwrap(), &[]).unwrap();
        assert_eq!(states(), [(1, Rejected), (2, Running)]);
        round(root, "x", None);
        assert_eq!(states(), [(1, Running), (2, Running)], "the fix went out");
        round(root, "x", Some(&delivered(root, 1, "O teste entrou.", &["src/a.rs"])));
        let approval = json!({"author": "review", "final": true, "wave": 1, "result": "approved", "text": "pronto"});
        mustard_core::io::spec_events::write(&events, "verdict", approval.as_object().cloned().unwrap(), &[]).unwrap();
        assert_eq!(states(), [(1, Approved), (2, Running)]);
    }

    /// A onda em andamento — com pedido e sem entrega depois dele — ocupa uma
    /// vaga do limite e a pasta de compilação dela: a rodada não passa do
    /// limite contando as que já saíram, e a onda que sai no lugar da que
    /// voltou fica com a pasta livre, e não com a da que segue em andamento.
    /// O pedido anterior ao replanejamento da onda não conta como andamento.
    /// A resposta lista as ondas em andamento com o código do pedido de cada
    /// uma.
    #[test]
    fn a_wave_in_flight_holds_a_slot_and_its_build_folder_and_a_send_before_the_replan_does_not_count() {
        // A onda 2 volta; a 3 sai no lugar dela, enquanto a 1 segue.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[]), (3, &["src/c.rs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");
        let (_, held) = sent_copy(root, 1);
        let (_, freed) = sent_copy(root, 2);
        let out = round(root, "x", Some(&delivered(root, 2, "Saiu.", &["src/a.rs"])));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), vec![3], "a vaga da 2 ficou livre: {out}");
        assert_eq!(sent_copy(root, 3).1, freed, "a 3 compila na pasta que a 2 deixou, e não na da 1 ({held})");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let sent = |wave: u64| {
            log.visible()
                .into_iter()
                .rfind(|e| e.event_type == "send" && e.wave() == Some(wave))
                .map(|e| codes[&e.id].clone())
                .unwrap()
        };
        assert_eq!(out["running"], json!([{"wave": 1, "send": sent(1)}, {"wave": 3, "send": sent(3)}]), "{out}");

        // Duas ondas em andamento enchem o limite de duas.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);
        let full = round(root, "x", None);
        assert_eq!(waves_in(&full, "dispatch"), Vec::<u64>::new(), "as duas vagas estão ocupadas: {full}");
        assert_eq!(waves_in(&full, "running"), vec![1, 2], "{full}");

        // Replanejada depois do pedido, a onda 2 deixa de estar em andamento:
        // ela sai de novo, e a vaga dela não fica presa ao pedido velho.
        replan(root, 2);
        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![2], "{again}");
        let running = again["running"].as_array().cloned().unwrap_or_default();
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let newest = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "send" && e.wave() == Some(2))
            .map(|e| codes[&e.id].clone())
            .next_back()
            .unwrap();
        assert_eq!(running[1], json!({"wave": 2, "send": newest}), "o pedido novo é o que conta: {again}");

        // A onda 1 entregou antes de a rodada existir, e um pedido saiu para
        // ela depois, sem reprovação no meio: ela não está em andamento, e as
        // duas vagas ficam para as outras.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[]), (3, &["src/c.rs"], &[])]);
        write(root, "x", "delivered", json!({"wave": 1, "text": "Saiu antes da rodada.", "files": ["src/a.rs"]}));
        seed_send(root, 1);
        let free = round(root, "x", None);
        assert_eq!(waves_in(&free, "dispatch"), vec![2, 3], "{free}");
        assert_eq!(waves_in(&free, "running"), vec![2, 3], "a onda 1 não está em andamento: {free}");
    }

    /// A onda que depende de todas as outras só sai depois de elas estarem
    /// aprovadas, não só entregues — a rodada não pede revisão nenhuma, então
    /// a entrega já vale como aprovação, menos para a onda que voltou
    /// reprovada: essa segura a que depende de todas até o conserto voltar. A
    /// que depende de uma parte sai com a entrega, como antes.
    #[test]
    fn the_wave_that_depends_on_all_the_others_waits_for_their_approvals() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[1, 2])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":3}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);

        // As duas entregam: sem revisão nenhuma da rodada, a entrega já basta
        // para soltar a 3.
        let both = format!("{}\n{}", delivered(root, 1, "Saiu.", &["src/a.rs"]), delivered(root, 2, "Saiu.", &["src/b.rs"]));
        let out = round(root, "x", Some(&both));
        assert!(out.get("reviews").is_none(), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), vec![3], "as duas entregas já soltam a 3: {out}");

        // A onda 2 depende só da 1, e a 3 não passa por ela: a 2 sai com a
        // entrega da 1.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1]), (3, &["src/c.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 3]);
        let out = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&out, "dispatch"), vec![2], "a entrega basta para quem não depende de todas: {out}");

        // A onda 2 entrega e é reprovada antes de a 1 entregar: a 3, que
        // depende das duas, não sai enquanto a 2 não voltar aprovada, mesmo
        // com a 1 pronta.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[1, 2])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":3}"#).unwrap();
        round(root, "x", None);
        round(root, "x", Some(&delivered(root, 2, "Saiu.", &["src/b.rs"])));
        let events = root.join(".claude/spec/x/spec.ndjson");
        let rejected = json!({"author": "review", "final": true, "wave": 2, "result": "rejected", "text": "faltou algo"});
        mustard_core::io::spec_events::write(&events, "verdict", rejected.as_object().cloned().unwrap(), &[]).unwrap();
        let out = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&out, "dispatch"), vec![2], "a reprovada volta antes da que depende de todas: {out}");
        assert_eq!(waves_in(&out, "running"), vec![2], "a 3 ainda espera a 2 aprovada: {out}");
    }

    /// A spec aprovada da análise antes do envio: uma onda, com a tarefa que
    /// faz uma das regras do projeto todo, duas regras do projeto todo que
    /// ela não faz, dois itens sem dono e uma decisão da onda. Devolve o
    /// número de cada item, pelo código.
    fn with_items_to_judge(root: &Path) -> BTreeMap<String, u64> {
        approved_with(root, "x", &[(1, &["src/a.rs"], &[])], |said| {
            let rule = |text: &str| {
                id_of(&write(root, "x", "rule", json!({"text": text, "example": "e", "keys": ["k"],
                    "applies_to": {"files": ["**"]}, "origin": said})))
            };
            rule("Vale sempre: a tabela nova tem chave.");
            rule("Vale sempre: a spec vira um PR só.");
            let done = rule("Vale sempre: a tabela nova tem índice.");
            for (text, extra) in [("Sem dono: a tabela nasce vazia.", json!({})), ("Sem dono: o download não muda.", json!({})),
                ("Da onda um: a coluna é texto.", json!({"waves": [1]}))] {
                let mut body = json!({"text": text, "keys": ["k"], "why": "w", "origin": said});
                body.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
                id_of(&write(root, "x", "decision", body));
            }
            write(root, "x", "task", json!({"wave": 1, "text": "Criar o índice da tabela.", "files": [{"path": "src/a.rs"}],
                "covers": [done], "origin": said}));
        });
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        log.codes().into_iter().map(|(id, code)| (code, id)).collect()
    }

    /// A linha da escolha da onda 1: o que sai e o que entra, com o motivo.
    fn analysis(removed: Value, added: Value) -> String {
        line("ANALYSIS", json!({"wave": 1, "removed": removed, "added": added}))
    }

    /// Os códigos de um grupo de candidatos que a rodada entregou.
    fn codes_in(asked: &Value, group: &str) -> Vec<String> {
        let listed = asked[group].as_array().map(Vec::as_slice).unwrap_or_default();
        listed.iter().filter_map(|c| c["item"].as_str()).map(str::to_string).collect()
    }

    /// A rodada que vai soltar uma onda com regras do projeto todo, itens sem
    /// dono e lições que casam com ela entrega esses candidatos ao
    /// orquestrador, cada um com o título, e nenhum agente é pedido: a
    /// resposta não traz modelo nem pedido pronto, e o próximo passo não fala
    /// de Sonnet. A lição que não casa com a onda não é candidata. A escolha
    /// volta na linha que a rodada lê e fica gravada no envio com o motivo, a
    /// da lição inclusive: o pedido leva a lição que ficou e não a que saiu.
    #[test]
    fn the_round_hands_the_candidates_to_the_orchestrator() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let ids = with_items_to_judge(root);
        let lesson = |text: &str, key: &str| {
            id_of(&write(root, "x", "lesson", json!({"class": "defect", "text": text, "keys": [key],
                "applies_to": {"files": ["**"]}})))
        };
        let kept = lesson("A tabela nova precisa de migração.", "tabela");
        let dropped = lesson("O índice novo deixa a busca lenta.", "índice");
        let far = lesson("O terminal do Windows troca a barra.", "terminal");

        let asked = round(root, "x", None);
        assert_eq!(asked["ok"], json!(true), "{asked}");
        assert_eq!(waves_in(&asked, "dispatch"), Vec::<u64>::new(), "{asked}");
        let title = |code: &str, title: &str| json!({"item": code, "title": title});
        assert_eq!(
            asked["analysis"],
            json!([{
                "wave": 1,
                "title": "Onda 1.",
                "project": [title("MSTD-RULE-0001", "Vale sempre: a tabela nova tem chave."),
                    title("MSTD-RULE-0002", "Vale sempre: a spec vira um PR só.")],
                "unowned": [title("MSTD-DEC-0001", "Sem dono: a tabela nasce vazia."),
                    title("MSTD-DEC-0002", "Sem dono: o download não muda.")],
                "lessons": [{"lesson": kept, "title": "A tabela nova precisa de migração."},
                    {"lesson": dropped, "title": "O índice novo deixa a busca lenta."}],
            }]),
            "the lesson {far} does not fit the wave: {asked}"
        );
        let next = asked["next"].as_str().unwrap_or_default();
        assert!(!next.to_lowercase().contains("sonnet") && !next.contains("model"), "{next}");
        assert!(next.contains("<ANALYSIS>"), "the next step teaches the line: {next}");

        let removed = json!([{"item": "MSTD-RULE-0002", "why": "Fala da entrega, e não da tabela."},
            {"lesson": dropped, "why": "A busca não muda nesta onda."}]);
        let added = json!([{"item": "MSTD-DEC-0001", "why": "A tabela nova nasce vazia."}]);
        let out = round(root, "x", Some(&analysis(removed, added)));
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        assert!(out.get("warnings").is_none(), "{out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "send").collect();
        assert_eq!(sent.len(), 1, "{out}");
        let judged: Vec<u64> = ["MSTD-RULE-0001", "MSTD-RULE-0002", "MSTD-DEC-0001", "MSTD-DEC-0002"].map(|c| ids[c]).to_vec();
        let recorded = json!({"judged": judged,
            "removed": [{"item": ids["MSTD-RULE-0002"], "why": "Fala da entrega, e não da tabela."}],
            "added": [{"item": ids["MSTD-DEC-0001"], "why": "A tabela nova nasce vazia."}],
            "judged_lessons": [kept, dropped],
            "removed_lessons": [{"lesson": dropped, "why": "A busca não muda nesta onda."}]});
        assert_eq!(sent[0].fields.get("analysis"), Some(&recorded), "the send records the choice: {out}");
        let prompt = out["dispatch"][0]["prompt"].as_str().unwrap_or_default();
        assert!(prompt.contains("A tabela nova precisa de migração."), "{prompt}");
        for out_of_it in ["O índice novo deixa a busca lenta.", "O terminal do Windows troca a barra."] {
            assert!(!prompt.contains(out_of_it), "{out_of_it}: {prompt}");
        }
        assert!(prompt.contains("- `agreed`: MSTD-RULE-0001, MSTD-RULE-0003, MSTD-DEC-0001, MSTD-DEC-0003\n"), "{prompt}");
    }

    /// A rodada que vai soltar uma onda com itens do projeto todo e itens sem
    /// dono entrega os candidatos ao orquestrador: nada sai, nenhum envio é
    /// gravado e nenhuma cópia é criada; sem a escolha, entrega de novo. Com
    /// a linha da escolha, a onda sai: o pedido e o envio levam o que ficou,
    /// sem o que saiu e com o que entrou, e o envio grava à parte o que saiu
    /// e o que entrou, com o motivo. O item que as tarefas da onda fazem vai
    /// sempre: ele não é candidato, e a linha que tenta tirá-lo fica sem
    /// efeito, como a que vem sem motivo. O gancho que monta o pedido no
    /// despacho chega ao mesmo texto. Na divisa do conserto: com os mesmos
    /// candidatos, o conserto sai com a escolha gravada; com uma regra nova do
    /// projeto todo, a rodada pede a escolha de novo.
    #[test]
    fn the_round_records_the_analysis_before_sending() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let ids = with_items_to_judge(root);
        let sends = || {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            log.visible().into_iter().filter(|e| e.event_type == "send").map(|e| e.fields.clone()).collect::<Vec<_>>()
        };

        // Sem a escolha, a onda não sai.
        for _ in 0..2 {
            let asked = round(root, "x", None);
            assert_eq!(asked["ok"], json!(true), "{asked}");
            assert_eq!(waves_in(&asked, "dispatch"), Vec::<u64>::new(), "{asked}");
            assert!(sends().is_empty(), "no send before the analysis: {asked}");
            assert!(!copy_path(root, "x", 1, false).exists(), "no copy before the analysis");
            let request = &asked["analysis"][0];
            assert_eq!(request["wave"], json!(1), "{asked}");
            assert_eq!(codes_in(request, "project"), ["MSTD-RULE-0001", "MSTD-RULE-0002"], "the rule the task does is not judged");
            assert_eq!(codes_in(request, "unowned"), ["MSTD-DEC-0001", "MSTD-DEC-0002"], "{asked}");
            assert_eq!(request["lessons"], json!([]), "{asked}");
            let next = translate("round.analysis", Locale::PtBr).replace("{waves}", "1");
            assert!(asked["next"].as_str().unwrap_or_default().contains(&next), "{asked}");
        }

        // Com a escolha, a onda sai com ela.
        let removed = json!([{"item": "MSTD-RULE-0002", "why": "Fala da entrega, e não da tabela."},
            {"item": "MSTD-RULE-0003", "why": "Tentou tirar a que a tarefa faz."}]);
        let added = json!([{"item": "MSTD-DEC-0001", "why": "A tabela nova nasce vazia."},
            {"item": "MSTD-DEC-0002", "why": ""}]);
        let out = round(root, "x", Some(&analysis(removed, added)));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        assert!(out.get("analysis").is_none(), "{out}");
        let prompt = out["dispatch"][0]["prompt"].as_str().unwrap_or_default().to_string();
        assert!(prompt.contains("- `agreed`: MSTD-RULE-0001, MSTD-RULE-0003, MSTD-DEC-0001, MSTD-DEC-0003\n"), "{prompt}");
        let ignored: Vec<&str> = out["warnings"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter(|w| w["reason"] == json!("analysis-item-ignored"))
            .filter_map(|w| w["hint"].as_str())
            .collect();
        assert_eq!(ignored.len(), 2, "{out}");
        assert!(ignored[0].contains("MSTD-RULE-0003") && ignored[1].contains("MSTD-DEC-0002"), "{ignored:?}");
        let sent = sends();
        assert_eq!(sent.len(), 1, "{out}");
        assert_eq!(sent[0]["text"], json!(prompt));
        let kept: Vec<u64> = ["MSTD-RULE-0001", "MSTD-RULE-0003", "MSTD-DEC-0001", "MSTD-DEC-0003"].map(|c| ids[c]).to_vec();
        let items: Vec<u64> = sent[0]["items"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
        for code in ["MSTD-RULE-0002", "MSTD-DEC-0002"] {
            assert!(!items.contains(&ids[code]), "{code} stayed out: {items:?}");
        }
        assert!(kept.iter().all(|id| items.contains(id)), "{items:?}");
        let judged: Vec<u64> = ["MSTD-RULE-0001", "MSTD-RULE-0002", "MSTD-DEC-0001", "MSTD-DEC-0002"].map(|c| ids[c]).to_vec();
        assert_eq!(sent[0]["analysis"], json!({"judged": judged,
            "removed": [{"item": ids["MSTD-RULE-0002"], "why": "Fala da entrega, e não da tabela."}],
            "added": [{"item": ids["MSTD-DEC-0001"], "why": "A tabela nova nasce vazia."}],
            "judged_lessons": [], "removed_lessons": []}));
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let running = waves_in_progress(&log).into_keys().collect();
        let flight = mustard_core::io::wave_prompt::Flight { running, ..Default::default() };
        let hooked = mustard_core::io::wave_prompt::prompts(root, "x", &log, Locale::PtBr, &flight);
        assert_eq!(hooked.iter().find(|p| p.wave == 1).map(|p| p.text.as_str()), Some(prompt.as_str()), "the hook builds the same request");

        // O conserto, com os mesmos itens a julgar, sai com a escolha gravada.
        round(root, "x", Some(&delivered(root, 1, "A tabela saiu.", &["src/a.rs"])));
        let fix = round(root, "x", Some(&verdict(1, "rejected", "faltou o índice")));
        assert_eq!(waves_in(&fix, "dispatch"), vec![1], "{fix}");
        let sent = sends();
        assert_eq!(sent.len(), 2, "{fix}");
        assert_eq!(sent[1]["analysis"], sent[0]["analysis"], "{fix}");

        // Com uma regra nova do projeto todo, o conserto seguinte pede a
        // análise de novo, e sai com a escolha nova.
        round(root, "x", Some(&delivered(root, 1, "O índice entrou.", &["src/a.rs"])));
        let said = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
            .visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id).unwrap();
        let newer = id_of(&write(root, "x", "rule", json!({"text": "Vale sempre: o nome é curto.", "example": "e",
            "keys": ["k"], "applies_to": {"files": ["**"]}, "origin": said})));
        let again = round(root, "x", Some(&verdict(1, "rejected", "faltou o nome")));
        assert_eq!(waves_in(&again, "dispatch"), Vec::<u64>::new(), "{again}");
        assert_eq!(codes_in(&again["analysis"][0], "project"), ["MSTD-RULE-0001", "MSTD-RULE-0002", "MSTD-RULE-0004"], "{again}");
        assert_eq!(sends().len(), 2, "{again}");
        let out = round(root, "x", Some(&analysis(json!([]), json!([]))));
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        let sent = sends();
        assert!(sent[2]["analysis"]["judged"].as_array().unwrap().contains(&json!(newer)), "{out}");
        assert_eq!(sent[2]["analysis"]["removed"], json!([]), "{out}");
    }

    /// Duas rodadas ao mesmo tempo, com a mesma linha da análise. As duas
    /// chegam ao despacho enquanto outro passo do git segura a trava; solta a
    /// trava, uma solta a onda com a escolha, e a outra lê a spec depois do
    /// envio dela e não a solta de novo: a onda tem um envio só, com a escolha.
    #[test]
    fn two_rounds_with_the_same_analysis_send_the_wave_once() {
        use std::time::Duration;
        let dir = tempdir().unwrap();
        let root = dir.path();
        with_items_to_judge(root);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), Vec::<u64>::new());
        let report = analysis(json!([{"item": "MSTD-RULE-0002", "why": "Fala da entrega."}]), json!([]));

        let outs: Vec<Value> = std::thread::scope(|scope| {
            let Ok(lock) = super::super::commit::git_lock(root) else { panic!("the git lock") };
            let rounds = [scope.spawn(|| round(root, "x", Some(&report))), scope.spawn(|| round(root, "x", Some(&report)))];
            std::thread::sleep(Duration::from_millis(1000));
            drop(lock);
            rounds.into_iter().map(|r| r.join().unwrap()).collect()
        });
        let dispatched: Vec<u64> = outs.iter().flat_map(|out| waves_in(out, "dispatch")).collect();
        assert_eq!(dispatched, vec![1], "only one round sends the wave out: {outs:?}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "send").collect();
        assert_eq!(sent.len(), 1, "{outs:?}");
        assert_eq!(sent[0].fields["analysis"]["removed"][0]["why"], json!("Fala da entrega."), "{outs:?}");
        for out in &outs {
            assert!(out.get("analysis").is_none(), "neither round asks again: {out}");
        }
    }
}
