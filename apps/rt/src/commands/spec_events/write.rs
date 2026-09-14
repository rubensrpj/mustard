//! `mustard-rt run write <tipo> --spec <nome> --json '{…}'` — grava um evento
//! no arquivo de eventos da spec, a única porta de escrita dele.
//!
//! O binário põe a versão do formato, o número, o código do item, a hora com o
//! fuso e o campo de busca; o resto vem do `--json`, que não pode trazer o
//! código. Quem aponta um item (`replaces`, os alvos de `remove` e `purge`)
//! usa o número do evento ou o código que a página mostra. A saída diz o
//! número e o código gravados e, num `remove` ou num `purge`, os números
//! afetados:
//!
//! ```text
//! {"ok": true, "spec": "teste", "id": 41, "type": "remove", "code": "MSTD-RMV-0002", "removed": [12, 13]}
//! ```
//!
//! A página e o `.md` da spec e a linha dela no índice das specs são refeitos
//! a cada gravação, ainda com a trava do arquivo de eventos presa.
//!
//! Com o tipo `lesson`, a gravação vai para o banco de lições
//! (`.claude/spec/lessons.ndjson`), e não para a spec: a classe vem em
//! `class`, a lição diz onde vale (`applies_to`) e onde nasceu (`found_in`), e
//! o `--spec`, opcional só aqui, diz a spec em que ela nasceu quando a lição
//! não diz. A página e o índice não mudam:
//!
//! ```text
//! {"ok": true, "id": 8, "type": "lesson", "class": "defect"}
//! ```
//!
//! Uma onda gravada depois da aprovação que leva a spec a ter mais ondas do
//! que tinha quando foi aprovada avisa o crescimento em `warnings`, com as
//! duas contas; o aviso nunca recusa. Um pedido (`request`) devolve em `next`
//! o passo seguinte, pelo `effect`: gravar as ondas novas no fim ou as versões
//! novas das que mudam, na mesma spec e na mesma branch.
//!
//! Um pedido adiado (`deferred`) aponta uma pendência aberta da lista do
//! projeto, a do checkout principal num worktree: o número pode vir escrito
//! `P-12`, e é gravado `12`. O número que a lista não tem, ou que já fechou, é
//! recusado com o comando que cria a pendência; o `deferred` nunca cria uma.
//!
//! Num worktree, o evento vai para o arquivo do checkout principal. As
//! citações de arquivo de um ponto são conferidas a partir de onde o comando
//! roda, e os nomes de código citados, no mapa do projeto: o nome que o mapa
//! não confirma entra em `warnings`, e o ponto é gravado.
//!
//! Uma spec aberta pelo `spec-draft` tem o `spec.md` escrito por ele, ao lado
//! do `meta.json`. Ali a página e o `.md` não são refeitos: o `.md` é o
//! documento do rascunho, e refazê-lo do arquivo de eventos apagaria o texto
//! da spec. O evento e a linha do índice são gravados do mesmo jeito.
//!
//! Quem grava por dentro do binário, como a testemunha da aprovação e o
//! `spec-draft`, usa [`record`], a mesma gravação deste comando.
//!
//! Este comando também não grava a execução de um critério (`criterion_run`)
//! nem o veredito (`verdict`), nem tira ou revê um deles: quem os grava é o
//! binário, quando roda o QA ([`record_run`]) e quando registra a revisão. Numa
//! spec cujo `spec.md` é o documento, os critérios vêm dos ACs
//! ([`sync_criteria`]), e ele não grava, não tira nem revê um `criterion`. O
//! autor `binary` é só das gravações de dentro do binário.
//!
//! Este comando não grava o tipo `state`: o estado da spec é dos comandos do
//! fluxo e da testemunha da aprovação. As portas que gravam `state` são
//! quatro, todas aqui — o [`record`] da testemunha, o [`record_birth`], o
//! [`record_open`] do `open` e o [`record_phase`] — e passam pela regra única
//! da mudança de fase
//! (`mustard_core::domain::spec_state::phase_write_allowed`), conferida com a
//! trava presa, no arquivo como ele ficaria. As outras gravações deste comando
//! também são conferidas: nenhuma delas muda o estado, nem removendo nem
//! revendo um `state`.
//!
//! Numa spec em levantamento ainda sem `context`, o primeiro `context` é o
//! objetivo, e ele é a resposta do usuário palavra por palavra: aponta em
//! `origin` uma mensagem do usuário e repete o texto dela
//! (`mustard_core::domain::spec_state::goal_rule`), na mesma conferência.
//!
//! O tipo de trabalho (`work_type`) é gravado pelo `grill`, que monta a lista
//! de pontos junto: este comando não o grava, nem o tira ou o revê.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mustard_core::domain::lessons::LESSON;
use mustard_core::domain::spec_events::{type_spec, Refusal, SpecEvent, SpecLog, PHASES};
use mustard_core::domain::spec_index;
use mustard_core::domain::spec_state::{
    birth_event, goal_rule, phase_write_allowed, waves_grown_by, PhaseWriter, SpecState, State,
};
use mustard_core::io::{lessons, spec_events as store};
use mustard_core::platform::i18n::translate;
use mustard_core::ClaudePaths;
use serde_json::{json, Map, Value};

use super::pages::SpecPages;
use crate::shared::spec_state::DiskSpecState;

/// Os tipos que só o binário grava: a execução de um critério, quando ele
/// roda o QA, e o veredito, quando ele registra a revisão. O `run write` não
/// os grava, nem tira ou revê um deles.
const BINARY_ONLY: &[&str] = &["criterion_run", "verdict"];

/// Os números dos eventos `event_type` que a leitura de `log` mostra.
fn visible_of(log: &SpecLog, event_type: &str) -> Vec<u64> {
    log.visible().into_iter().filter(|event| event.event_type == event_type).map(|event| event.id).collect()
}

/// Options for `mustard-rt run write`.
pub struct WriteOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec que recebe o evento; na lição, a spec em que ela nasceu.
    pub spec: Option<String>,
    pub event_type: String,
    /// Os campos do evento, num objeto JSON.
    pub json: String,
}

/// O núcleo testável de [`run`]: o relatório da gravação ou a recusa. Nunca
/// entra em pânico.
pub(crate) fn write_at(opts: &WriteOpts) -> Value {
    let project = super::project(&opts.root);
    let lang = project.lang;
    let refuse = move |refusal: Refusal| super::refused(&refusal, lang);

    let mut draft = match serde_json::from_str::<Value>(&opts.json) {
        Ok(Value::Object(map)) => map,
        Ok(other) => {
            let shown: String = other.to_string().chars().take(80).collect();
            return refuse(Refusal::NotAnObject { detail: shown });
        }
        Err(e) => return refuse(Refusal::NotAnObject { detail: e.to_string() }),
    };
    if draft.get("author").and_then(Value::as_str).map(str::trim) == Some("binary") {
        return refuse(Refusal::BinaryAuthor);
    }
    let event_type = opts.event_type.trim();
    if event_type == LESSON {
        return write_lesson(&project, opts.spec.as_deref(), draft);
    }
    let Some(spec) = opts.spec.as_deref() else {
        // Sem spec, um tipo que não existe continua recusado pelo nome.
        return refuse(if type_spec(event_type).is_some() {
            Refusal::SpecRequired { event_type: event_type.to_string() }
        } else {
            Refusal::UnknownType { found: event_type.to_string() }
        });
    };
    if event_type == "state" {
        return refuse(Refusal::StateByFlowOnly { spec: spec.trim().to_string() });
    }
    if event_type == "work_type" {
        return refuse(Refusal::WorkTypeByGrill);
    }
    if BINARY_ONLY.contains(&event_type) {
        return refuse(Refusal::BinaryOnlyType { event_type: event_type.to_string(), spec: spec.trim().to_string() });
    }
    if event_type == "criterion" && super::pages::drafted_by_spec_draft(&project.root, spec) {
        return refuse(Refusal::CriteriaFromSpecMd { spec: spec.trim().to_string() });
    }
    if event_type == "deferred"
        && let Err(refusal) = point_to_open_pending(&opts.root, &mut draft)
    {
        return refuse(refusal);
    }
    // O passo seguinte de um pedido, pelo efeito dele; a conferência do tipo
    // recusa um efeito que não existe antes de o relatório sair.
    let next = (event_type == "request")
        .then(|| draft.get("effect").and_then(Value::as_str).map(|effect| format!("request.{}", effect.trim())))
        .flatten();
    match record_in(&project, &opts.root, spec, event_type, draft, None) {
        Ok(Recorded { written, pages, grew }) => {
            let mut report = json!({
                "ok": true,
                "spec": spec.trim(),
                "id": written.id,
                "type": event_type,
            });
            if let Some(code) = &written.code {
                report["code"] = json!(code);
            }
            if !written.removed.is_empty() {
                report["removed"] = json!(written.removed);
            }
            if !written.purged.is_empty() {
                report["purged"] = json!(written.purged);
            }
            // Se não deu para gravar a página e o `.md`, ou para refazer a
            // linha da spec no índice, o evento já está no arquivo: fica o
            // aviso. O nome citado num fato que o mapa não confirma também
            // só avisa.
            let mut warnings = Vec::new();
            if let Some(Err(refusal)) = &pages {
                warnings.push(refusal.message(lang));
            }
            if let Some(refusal) = &written.index_warning {
                warnings.push(spec_index::write_warning(refusal, lang));
            }
            for (fact, finding) in &written.citation_warnings {
                warnings.extend(finding.warning(*fact, lang));
            }
            // O crescimento das ondas depois da aprovação só avisa.
            if let Some((approved, now)) = grew {
                warnings.push(
                    translate("spec_events.waves_grew", lang)
                        .replace("{approved}", &approved.to_string())
                        .replace("{now}", &now.to_string()),
                );
            }
            if !warnings.is_empty() {
                report["warnings"] = json!(warnings);
            }
            if let Some(key) = next {
                report["next"] = json!(translate(&key, lang));
            }
            report
        }
        Err(refusal) => refuse(refusal),
    }
}

/// O pedido adiado aponta uma pendência aberta da lista do projeto, vista de
/// `start`: o número escrito `P-12` vira `12`, e o que a lista não tem, ou que
/// já fechou, é recusado. Um valor que não é número fica como veio, para a
/// conferência do tipo recusar.
fn point_to_open_pending(start: &Path, draft: &mut Map<String, Value>) -> Result<(), Refusal> {
    let number = match draft.get("pending") {
        // Um número que não é inteiro positivo nunca é o de uma pendência: a
        // cobrança da entrega o descartaria calada.
        Some(Value::Number(number)) => match number.as_u64().filter(|n| *n > 0) {
            Some(n) => Some(n),
            None => return Err(Refusal::DeferredUnknownPending { pending: number.to_string() }),
        },
        Some(Value::String(text)) => {
            let text = text.trim();
            let digits = text.strip_prefix("P-").or_else(|| text.strip_prefix("p-")).unwrap_or(text);
            digits.trim().parse::<u64>().ok()
        }
        _ => None,
    };
    let Some(number) = number else {
        return Ok(());
    };
    draft.insert("pending".to_string(), json!(number));
    let pending = format!("P-{number}");
    match crate::commands::event::pending::pending_is_open(start, &pending) {
        Some(true) => Ok(()),
        Some(false) => Err(Refusal::DeferredClosedPending { pending }),
        None => Err(Refusal::DeferredUnknownPending { pending }),
    }
}

/// O que uma gravação deixou: o evento e, quando a página e o `.md` foram
/// refeitos, onde eles estão ou por que não foram gravados.
pub(crate) struct Recorded {
    pub(crate) written: store::Written,
    /// `None` quando a spec tem o `spec.md` do `spec-draft`, que fica como
    /// está.
    pub(crate) pages: Option<Result<SpecPages, Refusal>>,
    /// As ondas aprovadas e as de agora, quando a onda gravada fez a spec
    /// passar das ondas que tinha na aprovação que vale.
    pub(crate) grew: Option<(usize, usize)>,
}

/// Grava um evento da spec `spec`, vista de `start`, pela mesma gravação do
/// `run write`: a linha no arquivo de eventos, a linha da spec no índice e a
/// página e o `.md`, quando a spec não é um rascunho do `spec-draft`. `by`
/// diz quem grava, para a regra da mudança de fase.
pub(crate) fn record(
    start: &Path,
    spec: &str,
    event_type: &str,
    draft: Map<String, Value>,
    by: PhaseWriter,
) -> Result<Recorded, Refusal> {
    record_in(&super::project(start), start, spec, event_type, draft, Some(by))
}

/// A única gravação no arquivo de eventos de uma spec: toda porta chega
/// aqui, e a regra da mudança de fase confere o arquivo antes e depois, com a
/// trava presa.
fn record_in(
    project: &super::Project,
    start: &Path,
    spec: &str,
    event_type: &str,
    draft: Map<String, Value>,
    by: Option<PhaseWriter>,
) -> Result<Recorded, Refusal> {
    let path = store::spec_file(&project.root, spec)?;
    let roots = store::citation_roots(start, &project.root);
    let drafted = super::pages::drafted_by_spec_draft(&project.root, spec);
    let carried = (event_type == "state")
        .then(|| draft.get("phase").and_then(Value::as_str).map(|phase| phase.trim().to_string()))
        .flatten();
    let replaces = (event_type == "state").then(|| draft.get("replaces").and_then(Value::as_u64)).flatten();
    let name = spec.trim().to_string();
    // A página e o `.md` acompanham cada gravação e são refeitos antes de a
    // trava soltar, do que acabou de ser gravado: a gravação seguinte, de
    // outra sessão, só entra depois, e refaz os dois por último. A conta das
    // ondas também sai dali, com a trava presa: duas ondas gravadas ao mesmo
    // tempo nunca avisam a mesma conta.
    let mut pages = None;
    let mut grew = None;
    let wave = event_type == "wave";
    let written = store::write_guarded(
        &path,
        event_type,
        draft,
        &roots,
        |before, after| {
            phase_rule(&name, before, after, carried.as_deref(), replaces, by, drafted)?;
            goal_rule(&name, before, after)
        },
        |log| {
            if wave {
                grew = waves_grown_by(log, log.max_id());
            }
            if !drafted {
                pages = Some(super::pages::rebuild(&project.root, spec, log, project.lang));
            }
        },
    )?;
    Ok(Recorded { written, pages, grew })
}

/// A regra da mudança de fase sobre o arquivo antes e depois da gravação.
/// Sem porta do binário (`by` vazio), é o modelo pelo `run write`: nenhuma
/// gravação dele muda o estado.
///
/// Uma revisão (`replaces`) que repete a fase do item revisto não muda a
/// fase: não traz fase nenhuma para a regra. É o caso da branch que falta,
/// completada no `state` da própria aprovação.
fn phase_rule(
    spec: &str,
    before: &SpecLog,
    after: &SpecLog,
    carried: Option<&str>,
    replaces: Option<u64>,
    by: Option<PhaseWriter>,
    document: bool,
) -> Result<(), Refusal> {
    let (was, now) = (State::from_log(before), State::from_log(after));
    let revised = replaces.and_then(|id| before.get(id)).and_then(|event| event.str_field("phase")).map(str::trim);
    let carried = carried.filter(|phase| revised != Some(*phase));
    let Some(by) = by else {
        if let Some(event_type) = BINARY_ONLY.iter().find(|t| visible_of(before, t) != visible_of(after, t)) {
            return Err(Refusal::BinaryOnlyType { event_type: (*event_type).to_string(), spec: spec.to_string() });
        }
        // O tipo de trabalho é do `grill`: o modelo não o tira nem o revê.
        if visible_of(before, "work_type") != visible_of(after, "work_type") {
            return Err(Refusal::WorkTypeByGrill);
        }
        // Os critérios de uma spec cujo `spec.md` é o documento vêm dos ACs:
        // o modelo não os tira nem os revê.
        if document && visible_of(before, "criterion") != visible_of(after, "criterion") {
            return Err(Refusal::CriteriaFromSpecMd { spec: spec.to_string() });
        }
        return if was == now { Ok(()) } else { Err(Refusal::StateByFlowOnly { spec: spec.to_string() }) };
    };
    if phase_write_allowed(&was, &now, carried, by) {
        return Ok(());
    }
    Err(Refusal::PhaseChangeRefused {
        spec: spec.to_string(),
        from: was.phase.unwrap_or("-").to_string(),
        to: carried.or(now.phase).unwrap_or("-").to_string(),
    })
}

/// A ponte até os gravadores definitivos do fechamento e do merge: grava no
/// estado da spec `spec`, vista de `start`, a fase `phase` (`closed` no
/// fechamento, `delivered` no merge), pela mesma gravação do `run write`.
///
/// Só grava quando a spec tem arquivo de eventos (uma branch que o Mustard
/// não abriu fica como está) e quando a fase de agora vem antes de `phase` na
/// ordem das fases: repetir um fechamento não grava outro, e uma spec entregue
/// não volta a fechada. `true` quando gravou.
///
/// No mesmo passo, arma a cobrança das pendências nascidas na spec para o
/// número do `state` gravado, no checkout principal: o fim da resposta a lê
/// sem perguntar qual é a spec atual, então a arrumação que troca de branch,
/// a sessão desligada e o worktree não a perdem.
///
/// A sessão de quem fecha é dita por quem chama: uma entrada `run` a lê do
/// ambiente, um gancho a sabe pelo evento que recebeu. O contador guarda
/// essa sessão, e só ela é cobrada; sem sessão, qualquer sessão principal é.
pub(crate) fn record_phase(start: &Path, spec: &str, phase: &str, session: Option<&str>) -> bool {
    let order = |name: &str| PHASES.iter().position(|known| *known == name);
    let Some(target) = order(phase) else {
        return false;
    };
    let Some(state) = DiskSpecState::new(start).state(spec) else {
        return false;
    };
    if state.phase.and_then(order).is_some_and(|now| now >= target) {
        return false;
    }
    let mut draft = Map::new();
    draft.insert("phase".to_string(), json!(phase));
    draft.insert("author".to_string(), json!("binary"));
    match record(start, spec, "state", draft, PhaseWriter::Binary) {
        Ok(recorded) => {
            // A cobrança dispara com o fechamento e com o merge; a entrada na
            // execução não cobra nada.
            if matches!(phase, "closed" | "delivered") {
                let _ = crate::commands::event::pending::arm_charge(start, spec.trim(), recorded.written.id, session);
            }
            true
        }
        Err(_) => false,
    }
}

/// Os critérios da spec `spec`, vista de `start`, acertados com os ACs do
/// `spec.md` quando ele é o documento da spec: cada AC tem o seu `criterion`,
/// casado pelo id do AC, que fica no `label`. O AC sem critério ganha um; o AC
/// cujo comando mudou ganha uma versão nova do critério, com o comando novo;
/// o critério que não é de nenhum AC sai da leitura. Tudo pelo binário, pela
/// mesma gravação do `run write`.
///
/// É a ponte das specs do `spec-draft` até os gravadores definitivos, e a
/// função única de quem mexe nos critérios dessas specs: o `qa-run`, o
/// `review-result`, o `ac-add` e o `ac-amend` passam por aqui. Numa spec já
/// acertada, não grava nada. Devolve, para cada id de AC (em maiúsculas), o
/// número do critério vigente; vazio sem arquivo de eventos ou sem ACs.
pub(crate) fn sync_criteria(start: &Path, spec: &str) -> BTreeMap<String, u64> {
    use crate::commands::review::qa_run::{extract_ac_section, parse_ac_items, spec_file_for};
    let mut current = BTreeMap::new();
    let Some(log) = DiskSpecState::new(start).log(spec) else {
        return current;
    };
    let Some(items) = spec_file_for(&store::spec_root(start), spec)
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|markdown| extract_ac_section(&markdown))
        .map(|section| parse_ac_items(&section))
    else {
        return current;
    };
    let unquoted = |text: &str| text.trim().trim_matches('`').trim().to_string();
    let criteria: Vec<&SpecEvent> = log.visible().into_iter().filter(|event| event.event_type == "criterion").collect();
    let recorded: BTreeMap<String, (u64, String)> = criteria
        .iter()
        .filter_map(|event| {
            let proof = unquoted(event.str_field("proof").unwrap_or_default());
            event.str_field("label").map(|label| (ac_key(label), (event.id, proof)))
        })
        .collect();
    for item in &items {
        let key = ac_key(&item.id);
        let found = recorded.get(&key);
        if let Some((id, proof)) = found
            && *proof == unquoted(&item.command)
        {
            current.insert(key, *id);
            continue;
        }
        let (when, then) = split_statement(&item.statement, &item.id);
        let mut draft = Map::new();
        draft.insert("author".to_string(), json!("binary"));
        draft.insert("label".to_string(), json!(item.id));
        draft.insert("when".to_string(), json!(when));
        draft.insert("then".to_string(), json!(then));
        draft.insert("proof".to_string(), json!(item.command));
        if let Some((old, _)) = found {
            draft.insert("replaces".to_string(), json!(old));
        }
        if let Ok(written) = record(start, spec, "criterion", draft, PhaseWriter::Binary) {
            current.insert(key, written.written.id);
        }
    }
    // O critério que não é de nenhum AC não tem como receber execução: sai da
    // leitura, e o QA não fica preso nele.
    let orphans: Vec<u64> = criteria
        .iter()
        .filter(|event| event.str_field("label").is_none_or(|label| !current.contains_key(&ac_key(label))))
        .map(|event| event.id)
        .collect();
    if !orphans.is_empty() {
        let mut draft = Map::new();
        draft.insert("author".to_string(), json!("binary"));
        draft.insert("targets".to_string(), json!(orphans));
        draft.insert("reason".to_string(), json!("critério fora dos ACs do spec.md"));
        let _ = record(start, spec, "remove", draft, PhaseWriter::Binary);
    }
    current
}

/// O id de um AC como chave de casamento: sem espaços nas pontas e em
/// maiúsculas.
fn ac_key(id: &str) -> String {
    id.trim().to_ascii_uppercase()
}

/// O veredito que a revisão de um subprojeto deu, lido de um evento
/// `verdict`: o da ponte traz o subprojeto no `label` e o veredito dele nos
/// critérios; um veredito sem `label` vale pelo resultado.
fn own_approval(event: &SpecEvent) -> bool {
    if event.str_field("label").is_some()
        && let Some(items) = event.fields.get("criteria").and_then(Value::as_array)
    {
        return !items.is_empty() && items.iter().all(|c| c.get("tests_rule") == Some(&Value::Bool(true)));
    }
    event.str_field("result").map(str::trim) == Some("approved")
}

/// A frase de um AC partida em "quando" e "então", pela vírgula antes de
/// `then` ou de `então`; sem ela, a frase inteira vale para os dois. Uma
/// frase vazia vira o id do AC.
fn split_statement(statement: &str, id: &str) -> (String, String) {
    let text = statement.trim();
    if text.is_empty() {
        return (id.to_string(), id.to_string());
    }
    for marker in [", then ", ", então ", ", Then ", ", Então "] {
        if let Some((when, then)) = text.split_once(marker) {
            return (when.trim().to_string(), then.trim().to_string());
        }
    }
    (text.to_string(), text.to_string())
}

/// A ponte até o gravador definitivo da revisão: grava o veredito da revisão
/// do subprojeto `subproject` em cada onda que ele toca, pelas mesmas ondas e
/// subprojetos do plano de despacho, ou em todas as ondas quando não há
/// subprojeto ou quando ele não casa com nenhuma. Uma spec sem ondas tem a
/// onda 1. Antes, acerta os critérios com os ACs do `spec.md`.
///
/// Em cada onda, o resultado gravado junta o último veredito de cada
/// subprojeto daquela onda com este: basta um reprovado para a onda ficar
/// reprovada, e o veredito sem subprojeto só conta quando é o único. O evento
/// leva o subprojeto no `label` e o veredito dele nos critérios, conferidos
/// quando ele aprova. Devolve quantas ondas receberam o veredito.
pub(crate) fn record_verdict(
    start: &Path,
    spec: &str,
    verdict: &str,
    critical: i64,
    subproject: Option<&str>,
) -> usize {
    use crate::commands::pipeline::dispatch_plan::{build_plan_with_cycle, resolve_spec_dir};
    if !matches!(verdict, "approved" | "rejected") {
        return 0;
    }
    sync_criteria(start, spec);
    let Some(log) = DiskSpecState::new(start).log(spec) else {
        return 0;
    };
    let approved = verdict == "approved";
    let criteria: Vec<Value> = log
        .visible()
        .iter()
        .filter(|event| event.event_type == "criterion")
        .map(|event| json!({ "criterion": event.id, "tests_rule": approved }))
        .collect();
    if criteria.is_empty() {
        return 0;
    }
    let root = store::spec_root(start);
    let (plan, _) = build_plan_with_cycle(&root, &resolve_spec_dir(&root, spec), spec, None);
    let same = |a: &str, b: &str| {
        let clean = |s: &str| s.trim().trim_start_matches("./").trim_end_matches('/').to_string();
        clean(a) == clean(b)
    };
    let subproject = subproject.map(str::trim).filter(|s| !s.is_empty() && *s != ".");
    let all: std::collections::BTreeSet<u64> = plan.iter().map(|item| u64::from(item.wave)).collect();
    let touched: std::collections::BTreeSet<u64> = subproject.map_or_else(
        || all.clone(),
        |sub| plan.iter().filter(|item| same(&item.subproject, sub)).map(|item| u64::from(item.wave)).collect(),
    );
    let waves = match (touched.is_empty(), all.is_empty()) {
        (false, _) => touched,
        (true, false) => all,
        (true, true) => std::iter::once(1).collect(),
    };
    let key = subproject.unwrap_or(".").to_string();
    let text = format!("review-result: {verdict}, {critical} critical, subproject {key}");
    let mut written = 0;
    for wave in waves {
        // O último veredito de cada subprojeto desta onda, e o deste.
        let mut last: BTreeMap<String, (u64, bool)> = BTreeMap::new();
        for event in log.visible().into_iter().filter(|e| e.event_type == "verdict" && e.wave() == Some(wave)) {
            let sub = event.str_field("label").map(str::trim).filter(|s| !s.is_empty()).unwrap_or(".").to_string();
            let entry = last.entry(sub).or_insert((event.id, own_approval(event)));
            if event.id >= entry.0 {
                *entry = (event.id, own_approval(event));
            }
        }
        last.insert(key.clone(), (u64::MAX, approved));
        let real = last.keys().any(|sub| sub != ".");
        let joined = last.iter().filter(|(sub, _)| !(real && sub.as_str() == ".")).all(|(_, (_, ok))| *ok);
        let mut draft = Map::new();
        draft.insert("author".to_string(), json!("review"));
        draft.insert("label".to_string(), json!(key));
        draft.insert("wave".to_string(), json!(wave));
        draft.insert("result".to_string(), json!(if joined { "approved" } else { "rejected" }));
        draft.insert("text".to_string(), json!(text));
        draft.insert("criteria".to_string(), json!(criteria));
        if record(start, spec, "verdict", draft, PhaseWriter::Binary).is_ok() {
            written += 1;
        }
    }
    written
}

/// A ponte até o gravador definitivo do QA: grava na spec `spec`, vista de
/// `start`, uma execução do critério de número `criterion` (`pass` quando
/// `passed`), com o código de saída, o tempo e o fim da saída, pela mesma
/// gravação do `run write`. Só grava quando a spec tem arquivo de eventos.
/// `true` quando gravou.
pub(crate) fn record_run(
    start: &Path,
    spec: &str,
    criterion: u64,
    passed: bool,
    exit: Option<i64>,
    ms: u128,
    output: &str,
) -> bool {
    if DiskSpecState::new(start).log(spec).is_none() {
        return false;
    }
    let mut draft = Map::new();
    draft.insert("criterion".to_string(), json!(criterion));
    draft.insert("result".to_string(), json!(if passed { "pass" } else { "fail" }));
    draft.insert("exit".to_string(), json!(exit.unwrap_or(i64::from(!passed)).max(0)));
    draft.insert("ms".to_string(), json!(u64::try_from(ms).unwrap_or(u64::MAX)));
    draft.insert("author".to_string(), json!("binary"));
    if !output.trim().is_empty() {
        draft.insert("output".to_string(), json!(output.trim()));
    }
    record(start, spec, "criterion_run", draft, PhaseWriter::Binary).is_ok()
}

/// O nascimento de uma spec aberta fora do arquivo de eventos — pelo
/// `spec-draft`, pelo `tactical-fix-create` ou, numa spec aberta antes dele,
/// pela testemunha da aprovação: um `state` na fase `plan`, com a branch da
/// spec e a base de que ela foi cortada, quando se sabem, pela mesma gravação
/// do `run write`.
///
/// A branch é `branch`, quando o chamador a sabe (a que o rascunho cortou, a
/// da spec-mãe de um tactical fix); senão, a do checkout, quando ela é a
/// desta spec. A base vem do `meta.json` da spec. Uma spec que já tem fase
/// nunca volta ao plano, e um rascunho refeito nunca desfaz uma aprovação.
/// `Ok(true)` quando gravou.
///
/// Numa spec que já nasceu, completa a branch e a base que faltam, quando se
/// sabem, revendo o `state` do nascimento: a fase fica como está, e uma
/// branch ou uma base já gravadas nunca são trocadas. É o caso da spec
/// rascunhada numa base e cortada depois, e da branch que nasce só no corte.
///
/// Um ajuste tático (o `meta.json` com `parent`) mora na branch da mãe: sem
/// branch dita, vale a gravada da mãe, ou a do checkout quando ela é a da
/// mãe. O nome do ajuste nunca é o de uma branch.
pub(crate) fn record_birth(start: &Path, spec: &str, branch: Option<&str>) -> Result<bool, Refusal> {
    let born = DiskSpecState::new(start).log(spec).filter(|log| State::from_log(log).phase.is_some());
    // O `meta.json` mora na pasta da spec do checkout principal, também vista
    // de um worktree; a branch é a do checkout em `start`.
    let meta = ClaudePaths::for_project(store::spec_root(start))
        .and_then(|paths| paths.for_spec(spec.trim()))
        .ok()
        .and_then(|paths| mustard_core::read_meta(&paths.meta_json_path()));
    let base = meta.as_ref().and_then(|meta| meta.base.clone());
    let parent = meta
        .and_then(|meta| meta.parent)
        .map(|parent| parent.trim().trim_end_matches(['/', '\\']).trim().to_string())
        .filter(|parent| !parent.is_empty());
    let branch = match (branch, parent) {
        (Some(branch), _) => Some(branch.to_string()),
        (None, Some(parent)) => DiskSpecState::new(start)
            .state(&parent)
            .and_then(|state| state.branch)
            .or_else(|| branch_of_spec(start, &parent)),
        (None, None) => branch_of_spec(start, spec),
    };
    if let Some(log) = born {
        return complete_missing(start, spec, &log, branch, base);
    }
    let mut draft = Map::new();
    draft.insert("phase".to_string(), json!("plan"));
    draft.insert("author".to_string(), json!("binary"));
    if let Some(branch) = branch {
        draft.insert("branch".to_string(), json!(branch));
    }
    if let Some(base) = base {
        draft.insert("base".to_string(), json!(base));
    }
    record(start, spec, "state", draft, PhaseWriter::Binary).map(|_| true)
}

/// O nascimento de uma spec aberta pelo `open`: um `state` em levantamento,
/// com a branch que ele acabou de criar e a base de que ela saiu, pela mesma
/// gravação do `run write`. Uma spec que já tem fase nunca volta ao
/// levantamento: aí nada é gravado, e a resposta é `Ok(false)`.
pub(crate) fn record_open(start: &Path, spec: &str, branch: &str, base: &str) -> Result<bool, Refusal> {
    if DiskSpecState::new(start).log(spec).is_some_and(|log| State::from_log(&log).phase.is_some()) {
        return Ok(false);
    }
    let mut draft = Map::new();
    draft.insert("phase".to_string(), json!("survey"));
    draft.insert("author".to_string(), json!("binary"));
    draft.insert("branch".to_string(), json!(branch));
    draft.insert("base".to_string(), json!(base));
    record(start, spec, "state", draft, PhaseWriter::Binary).map(|_| true)
}

/// Antes de uma porta do binário avançar o estágio do `meta.json` de uma spec
/// sem nenhum `state` que a trava lê em plano — a antiga parada antes da
/// execução, ou a aberta pelo `run write` sem `meta.json` —, a spec nasce em
/// plano pelo [`record_birth`]. A trava dela deixa de seguir o `meta.json` e
/// passa a ler o estado: a execução só vem depois do "Aprovar". O
/// `emit-pipeline` cria o `meta.json` que falta já no estágio novo, e sem
/// este nascimento a trava soltaria. Nas outras specs, não faz nada.
pub(crate) fn birth_before_advance(start: &Path, spec: &str) {
    use crate::shared::spec_state::{lock_state, unborn};
    if unborn(start, spec) && lock_state(start, spec).is_some_and(|state| state.phase == Some("plan")) {
        let _ = record_birth(start, spec, None);
    }
}

/// Completa a branch e a base que faltam no estado de uma spec que já nasceu,
/// revendo o `state` do nascimento com os campos dele e o que faltava: a
/// revisão entra no lugar do nascimento na dobra, e a fase de agora não muda.
/// Nada é trocado: só o que falta entra. `Ok(false)` quando não falta nada que
/// se saiba.
fn complete_missing(
    start: &Path,
    spec: &str,
    log: &SpecLog,
    branch: Option<String>,
    base: Option<String>,
) -> Result<bool, Refusal> {
    /// Os campos que o binário carimba e que uma revisão não traz.
    const STAMPED: &[&str] = &["v", "id", "code", "at", "type", "search", "replaces", "author"];
    let state = State::from_log(log);
    let branch = branch.filter(|_| state.branch.is_none());
    let base = base.filter(|_| state.base.is_none());
    if branch.is_none() && base.is_none() {
        return Ok(false);
    }
    let Some(birth) = birth_event(log) else {
        return Ok(false);
    };
    let mut draft: Map<String, Value> = birth
        .fields
        .iter()
        .filter(|(key, _)| !STAMPED.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    draft.insert("replaces".to_string(), json!(birth.id));
    draft.insert("author".to_string(), json!("binary"));
    if let Some(branch) = branch {
        draft.insert("branch".to_string(), json!(branch));
    }
    if let Some(base) = base {
        draft.insert("base".to_string(), json!(base));
    }
    record(start, spec, "state", draft, PhaseWriter::Binary).map(|_| true)
}

/// A branch do checkout em `start`, quando ela é a da spec `spec`.
fn branch_of_spec(start: &Path, spec: &str) -> Option<String> {
    use crate::commands::event::work_branch::{current_branch, slug_of_work_branch};
    let config = mustard_core::ProjectConfig::load(start);
    let vcs = config.vcs()?;
    let current = current_branch(&vcs, &start.to_string_lossy())?;
    (slug_of_work_branch(&current, &config).as_deref() == Some(spec.trim())).then_some(current)
}

/// Grava uma lição no banco de lições do projeto. `spec`, quando vem, diz em
/// que spec a lição nasceu.
fn write_lesson(project: &super::Project, spec: Option<&str>, draft: Map<String, Value>) -> Value {
    let refuse = |refusal: Refusal| super::refused(&refusal, project.lang);
    let path = match ClaudePaths::for_project(&project.root) {
        Ok(paths) => paths.lessons_path(),
        Err(e) => return refuse(Refusal::Io { detail: e.to_string() }),
    };
    match lessons::write(&path, draft, spec) {
        Ok(written) => json!({ "ok": true, "id": written.id, "type": LESSON, "class": written.class }),
        Err(refusal) => refuse(refusal),
    }
}

/// Run `write` and print the JSON report; exit 1 on a refusal.
pub fn run(opts: &WriteOpts) {
    let report = write_at(opts);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    if report["ok"] != json!(true) {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_to(root: &std::path::Path, spec: Option<&str>, event_type: &str, json: &str) -> Value {
        write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: spec.map(str::to_string),
            event_type: event_type.into(),
            json: json.into(),
        })
    }

    fn write(root: &std::path::Path, event_type: &str, json: &str) -> Value {
        write_to(root, Some("teste"), event_type, json)
    }

    #[test]
    fn a_write_reports_its_number_and_what_a_removal_took_out() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let first = write(root, "message", r#"{"author":"user","text":"um"}"#);
        assert_eq!(
            first,
            json!({"ok": true, "spec": "teste", "id": 1, "type": "message", "code": "MSTD-MSG-0001"})
        );
        write(root, "message", r#"{"author":"user","text":"dois"}"#);
        let removal = write(root, "remove", r#"{"targets":[1,2],"reason":"engano"}"#);
        assert_eq!(removal["removed"], json!([1, 2]), "{removal}");
        assert!(root.join(".claude").join("spec").join("teste").join("spec.ndjson").is_file());
    }

    /// Um fato que cita um nome que o mapa não conhece entra, e o relatório
    /// avisa, com o número do fato; o nome declarado no arquivo citado passa
    /// calado.
    #[test]
    fn a_cited_name_the_map_does_not_know_warns_and_the_point_is_written() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/real.rs"), "fn um() {}\nfn dois_passos() {}\n").unwrap();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(
            mustard_core::io::project_map::model_path(root),
            r#"{"modules":[{"path":"src/real.rs","declarations":[{"kind":"function","name":"dois_passos","line":2}]}]}"#,
        )
        .unwrap();
        let msg = write(root, "message", r#"{"author":"user","text":"oi"}"#)["id"].as_u64().unwrap();
        let point = json!({
            "block": "limits", "gap": "g", "from": "gap", "status": "open", "origin": msg,
            "facts": [
                {"text": "quem lê é `ler_tudo`", "source": "src/real.rs:2"},
                {"text": "e depois `dois_passos`", "source": "src/real.rs:1"}
            ]
        });
        let report = write(root, "point", &point.to_string());
        assert_eq!(report["ok"], json!(true), "{report}");
        let warnings = report["warnings"].as_array().unwrap_or_else(|| panic!("no warnings: {report}"));
        assert_eq!(warnings.len(), 1, "{report}");
        let warning = warnings[0].as_str().unwrap();
        assert!(warning.contains("`ler_tudo`") && warning.contains('1'), "{warning}");
        assert!(!warning.contains("dois_passos"), "{warning}");
        let log = store::read(&root.join(".claude/spec/teste/spec.ndjson")).unwrap().unwrap();
        assert!(log.visible().iter().any(|event| event.event_type == "point"), "the point is in the file");
    }

    /// O autor `binary` é só das gravações de dentro do binário, e numa spec
    /// cujo `spec.md` é o documento os critérios vêm dos ACs: o `run write`
    /// recusa gravar, tirar e rever um `criterion` dela.
    #[test]
    fn run_write_refuses_the_binary_author_and_the_criteria_of_a_drafted_spec() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let msg = write(root, "message", r#"{"author":"user","text":"oi"}"#)["id"].as_u64().unwrap();
        let decision = r#"{"author":"binary","text":"t","keys":["k"],"why":"w"}"#;
        assert_eq!(write(root, "decision", decision)["reason"], json!("binary-author"));

        let criterion = format!(r#"{{"when":"w","then":"t","proof":"cd .","origin":{msg}}}"#);
        let accepted = write(root, "criterion", &criterion);
        assert_eq!(accepted["ok"], json!(true), "a spec rendered from its events takes a criterion: {accepted}");
        std::fs::write(
            root.join(".claude").join("spec").join("teste").join("spec.md"),
            "# T\n\n## Acceptance Criteria\n\n- **AC-1** — a. Command: `cd .`\n",
        )
        .unwrap();
        let refused = write(root, "criterion", &criterion);
        assert_eq!(refused["reason"], json!("criteria-from-spec-md"), "{refused}");
        let removal = write(root, "remove", &format!(r#"{{"targets":[{}],"reason":"engano"}}"#, accepted["id"]));
        assert_eq!(removal["reason"], json!("criteria-from-spec-md"), "{removal}");
        let revision = format!(r#"{{"when":"w","then":"t","proof":"cd ..","origin":{msg},"replaces":{}}}"#, accepted["id"]);
        assert_eq!(write(root, "criterion", &revision)["reason"], json!("criteria-from-spec-md"));
    }

    /// A execução de um critério e o veredito são gravados só pelo binário: o
    /// `run write` recusa os dois, e recusa tirar uma execução gravada. Uma
    /// execução aprovada escrita à mão nunca abre o fechamento.
    #[test]
    fn criteria_runs_and_verdicts_are_written_by_the_binary_only() {
        use crate::shared::spec_state::{seed_run, seed_runs};
        let dir = tempdir().unwrap();
        let root = dir.path();
        let criteria = seed_runs(root, "teste", &[None]);
        let failing = seed_run(root, "teste", criteria[0], "fail");

        let run = format!(r#"{{"criterion":{},"result":"pass","exit":0,"ms":1}}"#, criteria[0]);
        let refused = write(root, "criterion_run", &run);
        assert_eq!(refused["reason"], json!("binary-only-type"), "{refused}");
        let verdict = format!(
            r#"{{"wave":1,"result":"approved","text":"ok","criteria":[{{"criterion":{},"tests_rule":true}}]}}"#,
            criteria[0]
        );
        let refused = write(root, "verdict", &verdict);
        assert_eq!(refused["reason"], json!("binary-only-type"), "{refused}");
        let removal = write(root, "remove", &format!(r#"{{"targets":[{failing}],"reason":"engano"}}"#));
        assert_eq!(removal["reason"], json!("binary-only-type"), "taking the red run out is refused too: {removal}");

        assert!(
            crate::commands::spec::complete_spec::close_admission(root, "teste").is_err(),
            "the close still refuses"
        );
    }

    /// Cada gravação refaz a página e o `.md` da spec. Uma decisão revista
    /// mostra só a versão nova fora da conversa, onde a antiga aparece
    /// marcada como substituída; um item removido some dos dois e continua
    /// no arquivo de eventos, com o motivo.
    #[test]
    fn every_write_rebuilds_the_page_and_the_md() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "message", r#"{"author":"user","text":"decida"}"#);
        write(root, "decision", r#"{"text":"Texto antigo.","keys":["k"],"why":"w","origin":1}"#);
        let revised =
            write(root, "decision", r#"{"text":"Texto novo.","keys":["k"],"why":"w","origin":1,"replaces":2}"#);
        assert_eq!(revised["code"], json!("MSTD-DEC-0001"), "the new version keeps the code");
        write(root, "note", r#"{"text":"Anotação que sai.","keys":["n"],"origin":1}"#);
        let removal = write(root, "remove", r#"{"targets":[4],"reason":"engano"}"#);
        assert!(removal.get("warnings").is_none(), "{removal}");

        let spec = root.join(".claude").join("spec").join("teste");
        let md = std::fs::read_to_string(spec.join("spec.md")).unwrap();
        let html = std::fs::read_to_string(spec.join("spec.html")).unwrap();
        let (html_before, html_talk) = html.split_once("<section id=\"conversation\">").unwrap();
        let (md_before, md_talk) = md.rsplit_once("\n## ").unwrap();
        for (before, talk) in [(html_before, html_talk), (md_before, md_talk)] {
            assert!(before.contains("Texto novo.") && !before.contains("Texto antigo."), "{before}");
            assert!(talk.contains("Texto antigo."), "{talk}");
            assert!(!before.contains("Anotação que sai.") && !talk.contains("Anotação que sai."));
        }
        let events = std::fs::read_to_string(spec.join("spec.ndjson")).unwrap();
        assert!(events.contains("Anotação que sai.") && events.contains("engano"), "{events}");
    }

    /// Remover pelo código que a página mostra tira o item da leitura, da
    /// página e do `.md`, e ele continua no arquivo com o motivo. Um código
    /// que não existe é recusado citando o código, e nada é gravado.
    #[test]
    fn removing_by_the_code_takes_the_item_out_of_the_reading_the_page_and_the_md() {
        use crate::commands::spec_events::read::{read_at, ReadOpts};
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "message", r#"{"author":"user","text":"combine as regras"}"#);
        for text in ["Regra um.", "Regra dois.", "Regra três."] {
            let json = json!({"text": text, "keys": ["k"], "example": "e", "origin": 1}).to_string();
            assert_eq!(write(root, "rule", &json)["ok"], json!(true));
        }
        let removal = write(root, "remove", r#"{"targets":["MSTD-RULE-0002"],"reason":"Regra repetida."}"#);
        assert_eq!(removal["removed"], json!([3]), "{removal}");
        assert!(removal.get("warnings").is_none(), "{removal}");

        let agreed = read_at(&ReadOpts {
            root: root.to_path_buf(),
            spec: Some("teste".into()),
            block: "agreed".into(),
            term: None,
        })
        .unwrap();
        assert!(!agreed.contains("Regra dois.") && agreed.contains("Regra três."), "{agreed}");
        let spec = root.join(".claude").join("spec").join("teste");
        for page in ["spec.md", "spec.html"] {
            let shown = std::fs::read_to_string(spec.join(page)).unwrap();
            assert!(!shown.contains("Regra dois."), "{page}: {shown}");
            assert!(shown.contains("Regra um.") && shown.contains("Regra três."), "{page}");
        }
        let events = std::fs::read_to_string(spec.join("spec.ndjson")).unwrap();
        assert!(events.contains("Regra dois.") && events.contains("Regra repetida."), "{events}");

        let unknown = write(root, "remove", r#"{"targets":["MSTD-RULE-0009"],"reason":"r"}"#);
        assert_eq!(unknown["reason"], json!("unknown-target"), "{unknown}");
        assert!(unknown["hint"].as_str().unwrap().contains("MSTD-RULE-0009"), "{unknown}");
        let revised = write(root, "rule", r#"{"text":"Regra três, revista.","keys":["k"],"example":"e","origin":1,"replaces":"MSTD-RULE-0003"}"#);
        assert_eq!(revised["code"], json!("MSTD-RULE-0003"), "{revised}");
        let with_code = write(root, "note", r#"{"text":"t","keys":["k"],"origin":1,"code":"MSTD-NOTE-0001"}"#);
        assert_eq!(with_code["reason"], json!("binary-only-field"), "{with_code}");
        let after = std::fs::read_to_string(spec.join("spec.ndjson")).unwrap();
        assert_eq!(after.lines().count(), events.lines().count() + 1, "only the revision was written");
    }

    /// Numa spec aberta pelo `spec-draft`, o `spec.md` é o documento do
    /// rascunho: gravar um evento não o refaz, e a página não nasce. O evento
    /// e a linha do índice são gravados do mesmo jeito.
    #[test]
    fn a_spec_drafted_by_spec_draft_keeps_its_md() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec = root.join(".claude").join("spec").join("teste");
        std::fs::create_dir_all(&spec).unwrap();
        std::fs::write(spec.join("meta.json"), r#"{"scope":"light","stage":"Plan"}"#).unwrap();
        std::fs::write(spec.join("spec.md"), "# Rascunho\n\n## Contexto\n\nO texto da spec.\n").unwrap();

        let out = write(root, "message", r#"{"author":"user","text":"um recado"}"#);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(
            std::fs::read_to_string(spec.join("spec.md")).unwrap(),
            "# Rascunho\n\n## Contexto\n\nO texto da spec.\n",
            "the draft's document is left alone",
        );
        assert!(!spec.join("spec.html").exists(), "no page over a draft");
        assert!(std::fs::read_to_string(spec.join("spec.ndjson")).unwrap().contains("um recado"));
        let index = std::fs::read_to_string(root.join(".claude").join("spec").join("index.ndjson")).unwrap();
        assert!(index.contains("\"teste\""), "{index}");
    }

    fn witness_approves(root: &std::path::Path) {
        let draft = json!({
            "phase": "approved",
            "author": "user",
            "witness": { "question": "Aprovar esta spec?", "answer": "Aprovar" }
        });
        record(root, "teste", "state", draft.as_object().cloned().unwrap(), PhaseWriter::Witness)
            .expect("the witness approves a spec in plan");
    }

    fn lines(root: &std::path::Path) -> usize {
        let events = root.join(".claude").join("spec").join("teste").join("spec.ndjson");
        std::fs::read_to_string(events).unwrap().lines().count()
    }

    fn born(root: &std::path::Path) {
        assert_eq!(record_birth(root, "teste", None), Ok(true), "the binary opens the spec");
    }

    /// O `run write` não grava o estado: com branch, com fase ou sem nada, numa
    /// spec sem `state`, numa em plano e numa aprovada, a recusa é a mesma, e
    /// nada é gravado. A porta da testemunha grava.
    #[test]
    fn the_model_never_writes_the_state() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "message", r#"{"author":"user","text":"um"}"#);
        let payloads = [
            json!({ "phase": "plan", "branch": "feature/outra" }),
            json!({ "phase": "approved", "witness": { "question": "Aprovar esta spec?", "answer": "Aprovar" } }),
            json!({ "phase": "running" }),
            json!({}),
        ];
        let refuse_all = |stage: &str| {
            let before = lines(root);
            for payload in &payloads {
                let out = write(root, "state", &payload.to_string());
                assert_eq!(out["reason"], json!("state-by-flow-only"), "{stage}: {payload}: {out}");
                assert!(out["hint"].as_str().unwrap().contains("teste"), "{out}");
            }
            assert_eq!(lines(root), before, "{stage}: nothing was written");
        };
        refuse_all("no state");
        born(root);
        refuse_all("in plan");
        witness_approves(root);
        refuse_all("approved");
        assert!(DiskSpecState::new(root).state("teste").unwrap().approved);
    }

    /// Nenhuma outra gravação do modelo muda o estado: tirar o `state` da
    /// aprovação é recusado; tirar um item que não é estado passa.
    #[test]
    fn a_removal_by_the_model_never_changes_the_state() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        born(root);
        witness_approves(root);
        let approval = lines(root) as u64;
        let removed = write(root, "remove", &json!({ "targets": [approval], "reason": "engano" }).to_string());
        assert_eq!(removed["reason"], json!("state-by-flow-only"), "{removed}");
        let note = write(root, "message", r#"{"author":"user","text":"sai"}"#);
        let id = note["id"].as_u64().unwrap();
        let ok = write(root, "remove", &json!({ "targets": [id], "reason": "engano" }).to_string());
        assert_eq!(ok["ok"], json!(true), "{ok}");
        assert!(DiskSpecState::new(root).state("teste").unwrap().approved);
    }

    /// A ponte do fechamento não fecha uma spec em plano: o fechamento só vem
    /// depois da aprovação.
    #[test]
    fn the_bridge_never_closes_a_spec_in_plan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        born(root);
        assert!(!record_phase(root, "teste", "closed", None), "no closing before the approval");
        assert_eq!(lines(root), 1);
        witness_approves(root);
        assert!(record_phase(root, "teste", "closed", None), "the approved spec closes");
    }

    /// A branch que falta é completada quando a do checkout é a da spec, sem
    /// mexer na fase; uma branch já gravada nunca é trocada.
    #[test]
    fn a_missing_branch_is_completed_and_a_recorded_one_never_changes() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#).unwrap();
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            assert!(ok, "git {args:?} failed");
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "t"]);
        git(&["checkout", "-q", "-b", "dev"]);
        std::fs::write(root.join("README.md"), "oi\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "init"]);
        let state = || DiskSpecState::new(root).state("teste").unwrap();

        // Rascunhada na base: nasce sem branch, e é aprovada.
        born(root);
        assert_eq!(state().branch, None);
        witness_approves(root);

        // Cortada depois: a branch que faltava é completada, e a aprovação fica.
        git(&["checkout", "-q", "-b", "feature/teste"]);
        assert_eq!(record_birth(root, "teste", None), Ok(true));
        assert_eq!(state().branch.as_deref(), Some("feature/teste"));
        assert!(state().approved, "the approval stays");

        // Uma branch gravada nunca é trocada.
        git(&["checkout", "-q", "-b", "feature/outra"]);
        assert_eq!(record_birth(root, "teste", Some("feature/zzz")), Ok(false));
        assert_eq!(state().branch.as_deref(), Some("feature/teste"));
    }

    /// Um repositório em `root`, com o fluxo `dev` → `main`, no checkout
    /// `branch`.
    fn repo_on(root: &std::path::Path, branch: &str) {
        std::fs::write(root.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#).unwrap();
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            assert!(ok, "git {args:?} failed");
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "t"]);
        git(&["checkout", "-q", "-b", "dev"]);
        std::fs::write(root.join("README.md"), "oi\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "init"]);
        if branch != "dev" {
            git(&["checkout", "-q", "-b", branch]);
        }
    }

    /// Um `state` gravado direto no arquivo da spec, sem regra nenhuma.
    fn seed_state(root: &std::path::Path, spec: &str, fields: Value) {
        let path = store::spec_file(root, spec).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        store::write(&path, "state", fields.as_object().cloned().unwrap(), &[]).unwrap();
    }

    /// Numa spec cujo primeiro `state` é a própria aprovação, a branch que
    /// falta é completada: a revisão repete a fase aprovada, e isso não é
    /// mudança de fase.
    #[test]
    fn a_missing_branch_is_completed_on_the_approval_itself() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        repo_on(root, "feature/teste");
        seed_state(
            root,
            "teste",
            json!({ "phase": "approved", "author": "user", "witness": { "question": "Aprovar esta spec?", "answer": "Aprovar" } }),
        );
        assert_eq!(record_birth(root, "teste", None), Ok(true));
        let state = DiskSpecState::new(root).state("teste").unwrap();
        assert_eq!(state.branch.as_deref(), Some("feature/teste"));
        assert!(state.approved, "the approval stays");
    }

    /// Um ajuste tático nasce na branch da mãe: a gravada dela, de qualquer
    /// checkout; sem ela, a do checkout, quando ela é a da mãe.
    #[test]
    fn a_tactical_fix_is_born_on_its_parents_branch() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        repo_on(root, "feature/epic");
        let fix = |name: &str, parent: &str| {
            let folder = root.join(".claude").join("spec").join(name);
            std::fs::create_dir_all(&folder).unwrap();
            let meta = json!({ "scope": "light", "stage": "Analyze", "outcome": "Active", "parent": parent });
            std::fs::write(folder.join("meta.json"), meta.to_string()).unwrap();
        };
        let branch = |name: &str| DiskSpecState::new(root).state(name).unwrap().branch;

        // A mãe sem branch gravada: vale a do checkout, que é a dela.
        seed_state(root, "epic", json!({ "phase": "running" }));
        fix("ajuste", "epic");
        assert_eq!(record_birth(root, "ajuste", None), Ok(true));
        assert_eq!(branch("ajuste").as_deref(), Some("feature/epic"));

        // A mãe com branch gravada: vale a gravada, mesmo em outro checkout.
        seed_state(root, "mae", json!({ "phase": "running", "branch": "feature/mae" }));
        fix("outro-ajuste", "mae");
        assert_eq!(record_birth(root, "outro-ajuste", None), Ok(true));
        assert_eq!(branch("outro-ajuste").as_deref(), Some("feature/mae"));
    }

    #[test]
    fn what_is_not_a_json_object_is_refused() {
        let dir = tempdir().unwrap();
        for json in ["[1,2]", "{quebrado", "\"texto\""] {
            let out = write(dir.path(), "note", json);
            assert_eq!(out["reason"], json!("not-an-object"), "{json}: {out}");
        }
        assert!(!dir.path().join(".claude").exists(), "a refusal writes nothing");
    }

    /// Um tipo que não existe e um campo que falta são recusados pelo nome; um
    /// tipo da spec sem `--spec` pede a spec, e nada é gravado.
    #[test]
    fn an_unknown_type_and_a_missing_field_are_refused_by_name() {
        let dir = tempdir().unwrap();
        let unknown = write(dir.path(), "licao", r#"{"text":"x"}"#);
        assert_eq!(unknown["reason"], json!("unknown-type"));
        assert!(unknown["hint"].as_str().unwrap().contains("licao"));
        let missing = write(dir.path(), "rule", r#"{"text":"t","keys":["k"],"origin":1}"#);
        assert_eq!(missing["reason"], json!("missing-field"));
        assert!(missing["hint"].as_str().unwrap().contains("example"));
        let no_spec = write_to(dir.path(), None, "rule", r#"{"text":"t","keys":["k"],"example":"e","origin":1}"#);
        assert_eq!(no_spec["reason"], json!("spec-required"), "{no_spec}");
        assert!(no_spec["hint"].as_str().unwrap().contains("--spec"), "{no_spec}");
        let unknown_no_spec = write_to(dir.path(), None, "licao", "{}");
        assert_eq!(unknown_no_spec["reason"], json!("unknown-type"), "{unknown_no_spec}");
        assert!(!dir.path().join(".claude").exists(), "a refusal writes nothing");
    }

    /// A lição vai para o banco de lições, com a spec do `--spec` dizendo
    /// onde ela nasceu; o arquivo de eventos, a página, o `.md` e o índice
    /// ficam como estavam. Sem `--spec`, a lição diz sozinha onde nasceu.
    #[test]
    fn writing_a_lesson_goes_to_the_bank_and_leaves_the_spec_untouched() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "message", r#"{"author":"user","text":"um"}"#);
        let specs = root.join(".claude").join("spec");
        let files = [specs.join("teste").join("spec.ndjson"), specs.join("teste").join("spec.md"), specs.join("teste").join("spec.html"), specs.join("index.ndjson")];
        let before: Vec<Vec<u8>> = files.iter().map(|f| std::fs::read(f).unwrap()).collect();

        let lesson = r#"{"class":"defect","text":"Um rm -rf na pasta errada perde trabalho.","keys":["apagar","rm"],"applies_to":{"subproject":"apps/rt"}}"#;
        assert_eq!(write(root, "lesson", lesson), json!({"ok": true, "id": 1, "type": "lesson", "class": "defect"}));
        let after: Vec<Vec<u8>> = files.iter().map(|f| std::fs::read(f).unwrap()).collect();
        assert!(before == after, "the spec's files did not move");
        let bank = std::fs::read_to_string(specs.join("lessons.ndjson")).unwrap();
        assert!(bank.contains(r#""found_in":{"spec":"teste"}"#) && bank.contains(r#""type":"defect""#), "{bank}");

        let everywhere = r#"{"class":"user_preference","text":"Resposta curta.","keys":["resposta"],"applies_to":{"files":["**"]},"found_in":{"source":"CLAUDE.md"}}"#;
        let second = write_to(root, None, "lesson", everywhere);
        assert_eq!(second["id"], json!(2), "{second}");
        let no_origin = r#"{"class":"defect","text":"t","keys":["k"],"applies_to":{"skill":"s"}}"#;
        let refused = write_to(root, None, "lesson", no_origin);
        assert_eq!(refused["reason"], json!("lesson-origin-missing"), "{refused}");
        assert_eq!(std::fs::read_to_string(specs.join("lessons.ndjson")).unwrap().lines().count(), 2);
    }

    /// O `search` é gravado no arquivo de eventos e nunca aparece na página
    /// nem no `.md`: os dois mostram só o texto original.
    #[test]
    fn the_page_and_the_md_never_show_the_search_field() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "message", r#"{"author":"user","text":"combine"}"#);
        let rule = r#"{"text":"Apagando a pasta, a trava barra o comando.","keys":["apagar","trava"],"example":"rm -rf pasta","origin":1}"#;
        assert_eq!(write(root, "rule", rule)["ok"], json!(true));
        let spec = root.join(".claude").join("spec").join("teste");
        let events = std::fs::read_to_string(spec.join("spec.ndjson")).unwrap();
        let line = events.lines().find(|l| l.contains("\"type\":\"rule\"")).unwrap();
        let search = serde_json::from_str::<Value>(line).unwrap()["search"].as_str().unwrap().to_string();
        assert!(search.contains(' '), "{search}");
        for page in ["spec.md", "spec.html"] {
            let shown = std::fs::read_to_string(spec.join(page)).unwrap();
            assert!(shown.contains("Apagando a pasta, a trava barra o comando."), "{page}");
            assert!(!shown.contains(&search) && !shown.contains("\"search\""), "{page} shows the search field");
        }
    }

    /// Uma spec nascida em plano, com uma mensagem, um critério e as ondas
    /// `1..=waves`; devolve o número da mensagem e o do critério.
    fn planned_with_waves(root: &std::path::Path, waves: u64) -> (u64, u64) {
        born(root);
        let msg = write(root, "message", r#"{"author":"user","text":"o plano"}"#)["id"].as_u64().unwrap();
        let criterion = json!({"when": "w", "then": "t", "proof": "cargo test", "origin": msg});
        let criterion = write(root, "criterion", &criterion.to_string())["id"].as_u64().unwrap();
        for n in 1..=waves {
            assert_eq!(write_wave(root, n, msg, criterion, None)["ok"], json!(true));
        }
        (msg, criterion)
    }

    fn write_wave(root: &std::path::Path, n: u64, origin: u64, criterion: u64, replaces: Option<u64>) -> Value {
        let mut wave = json!({"n": n, "text": format!("Onda {n}."), "criteria": [criterion], "done_when": "d", "origin": origin});
        if let Some(old) = replaces {
            wave["replaces"] = json!(old);
        }
        write(root, "wave", &wave.to_string())
    }

    /// Um pedido do usuário depois da aprovação entra na mesma spec, na mesma
    /// branch: nenhuma pasta de spec nova, nenhuma branch nova, e a aprovação
    /// continua valendo. O relatório diz o passo seguinte pelo efeito.
    #[test]
    fn a_request_after_approval_keeps_the_spec_and_the_branch() {
        use mustard_core::platform::i18n::Locale;
        let dir = tempdir().unwrap();
        let root = dir.path();
        repo_on(root, "feature/teste");
        born(root);
        witness_approves(root);
        let branches = || {
            let out = std::process::Command::new("git").args(["branch", "--list"]).current_dir(root).output().unwrap();
            String::from_utf8_lossy(&out.stdout).to_string()
        };
        let before = branches();

        let msg = write(root, "message", r#"{"author":"user","text":"inclua o Windows"}"#)["id"].as_u64().unwrap();
        for (effect, key) in [("new_waves", "request.new_waves"), ("adjust_waves", "request.adjust_waves")] {
            let request = json!({"text": "Incluir o Windows.", "keys": ["windows"], "effect": effect, "origin": msg});
            let out = write(root, "request", &request.to_string());
            assert_eq!(out["ok"], json!(true), "{out}");
            assert_eq!(out["spec"], json!("teste"), "{out}");
            assert_eq!(out["next"], json!(translate(key, Locale::PtBr)), "{out}");
        }
        let note = write(root, "note", &json!({"text": "t", "keys": ["k"], "origin": msg}).to_string());
        assert!(note.get("next").is_none(), "only a request says the next step: {note}");

        let state = DiskSpecState::new(root).state("teste").unwrap();
        assert!(state.approved, "the user's request needs no new approval");
        assert_eq!(state.branch.as_deref(), Some("feature/teste"));
        assert_eq!(branches(), before, "no branch was created");
        let specs: Vec<String> = std::fs::read_dir(root.join(".claude").join("spec"))
            .unwrap()
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(specs, ["teste"], "no spec was opened");
    }

    /// Quatro ondas aprovadas e duas novas: cada onda nova é gravada, sai
    /// sem recusa e avisa a conta; a segunda diz "tinha 4, agora tem 6". A
    /// versão nova de uma onda não avisa.
    #[test]
    fn new_waves_after_approval_warn_the_growth_and_are_written() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (msg, criterion) = planned_with_waves(root, 4);
        let before = write_wave(root, 5, msg, criterion, None);
        assert!(before.get("warnings").is_none(), "no approval yet, no growth: {before}");
        let removed = write(root, "remove", &json!({"targets": [before["id"]], "reason": "cedo"}).to_string());
        assert_eq!(removed["ok"], json!(true), "{removed}");
        witness_approves(root);

        let fifth = write_wave(root, 5, msg, criterion, None);
        assert_eq!(fifth["ok"], json!(true), "{fifth}");
        assert_eq!(fifth["warnings"], json!(["A spec tinha 4 ondas aprovadas, agora tem 5."]), "{fifth}");
        let sixth = write_wave(root, 6, msg, criterion, None);
        assert_eq!(sixth["ok"], json!(true), "{sixth}");
        assert_eq!(sixth["warnings"], json!(["A spec tinha 4 ondas aprovadas, agora tem 6."]), "{sixth}");

        let revised = write_wave(root, 6, msg, criterion, sixth["id"].as_u64());
        assert_eq!(revised["ok"], json!(true), "{revised}");
        assert!(revised.get("warnings").is_none(), "a new version of a wave is not growth: {revised}");

        let log = DiskSpecState::new(root).log("teste").unwrap();
        assert_eq!(mustard_core::domain::spec_state::waves_now(&log), 6, "the new waves are in the file");
        assert!(DiskSpecState::new(root).state("teste").unwrap().approved);
    }

    /// Acrescenta na lista de pendências do projeto em `root` uma pendência
    /// aberta com o título `title` e devolve o número dela.
    fn add_pending(root: &std::path::Path, title: &str) -> String {
        use crate::commands::event::pending::{pending_at, PendingOpts};
        let out = pending_at(&PendingOpts {
            root: root.to_path_buf(),
            add: true,
            title: Some(title.into()),
            detail: Some("combinado na spec".into()),
            ..PendingOpts::default()
        });
        assert_eq!(out["ok"], json!(true), "{out}");
        out["id"].as_str().unwrap().to_string()
    }

    /// Um projeto em pasta temporária, com o `mustard.json` que prende a
    /// lista de pendências nele, e uma mensagem do usuário na spec `teste`.
    fn project_with_message() -> (tempfile::TempDir, u64) {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), "{}").unwrap();
        let msg = write(dir.path(), "message", r#"{"author":"user","text":"e o antivírus?"}"#)["id"].as_u64().unwrap();
        (dir, msg)
    }

    fn deferred(root: &std::path::Path, pending: Value, origin: u64) -> Value {
        let draft = json!({"text": "Medir o antivírus do Windows.", "keys": ["windows"], "pending": pending, "origin": origin});
        write(root, "deferred", &draft.to_string())
    }

    /// O pedido de outro assunto vira uma pendência com número e, na spec, só
    /// o pedido adiado que aponta para ela, nunca uma onda. A cobrança da
    /// entrega acha a pendência pelo pedido adiado.
    #[test]
    fn a_request_on_another_subject_becomes_a_numbered_pending_and_a_deferred() {
        use crate::commands::event::pending::{born_in, open_born_in};
        let (dir, msg) = project_with_message();
        let root = dir.path();
        let id = add_pending(root, "Medir o antivírus do Windows");
        assert_eq!(id, "P-1");

        let out = deferred(root, json!(id), msg);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["code"], json!("MSTD-DEFER-0001"), "{out}");
        let log = DiskSpecState::new(root).log("teste").unwrap();
        let event = log.visible().into_iter().find(|event| event.event_type == "deferred").unwrap();
        assert_eq!(event.int("pending"), Some(1));
        assert_eq!(born_in(&log), ["P-1"]);
        let open: Vec<String> = open_born_in(root, &log).into_iter().map(|item| item.id).collect();
        assert_eq!(open, ["P-1"], "the delivery asks about it");
        assert_eq!(mustard_core::domain::spec_state::waves_now(&log), 0, "never a wave");
    }

    /// O número da pendência pode vir como `P-n`, como `p-n` ou como o número
    /// puro: no arquivo, fica sempre o número.
    #[test]
    fn a_pending_number_written_as_p_n_is_recorded_as_the_number() {
        let (dir, msg) = project_with_message();
        let root = dir.path();
        add_pending(root, "um");
        add_pending(root, "dois");
        for (pending, number) in [(json!("P-2"), 2), (json!(" p-1 "), 1), (json!(2), 2), (json!("1"), 1)] {
            let out = deferred(root, pending.clone(), msg);
            assert_eq!(out["ok"], json!(true), "{pending}: {out}");
            let log = DiskSpecState::new(root).log("teste").unwrap();
            let written = log.get(out["id"].as_u64().unwrap()).unwrap();
            assert_eq!(written.fields.get("pending"), Some(&json!(number)), "{pending}");
        }
        let odd = deferred(root, json!("P-dois"), msg);
        assert_eq!(odd["reason"], json!("invalid-value"), "{odd}");
    }

    /// Um número de pendência que não é inteiro positivo (negativo, zero ou
    /// com fração) é recusado como pendência que a lista não tem, e nada é
    /// gravado.
    #[test]
    fn a_deferred_request_with_a_number_that_is_not_a_positive_integer_is_refused() {
        let (dir, msg) = project_with_message();
        let root = dir.path();
        add_pending(root, "a única");
        let before = lines(root);
        for (pending, shown) in [(json!(-12), "-12"), (json!(0), "0"), (json!(1.5), "1.5")] {
            let out = deferred(root, pending.clone(), msg);
            assert_eq!(out["reason"], json!("deferred-unknown-pending"), "{pending}: {out}");
            assert!(out["hint"].as_str().unwrap().contains(shown), "{pending}: {out}");
        }
        assert_eq!(lines(root), before, "nothing was written");
    }

    /// Um pedido adiado para uma pendência que a lista não tem é recusado,
    /// com o comando que cria a pendência, e nada é gravado; sem lista
    /// nenhuma, também.
    #[test]
    fn a_deferred_request_pointing_to_a_missing_pending_is_refused() {
        let (dir, msg) = project_with_message();
        let root = dir.path();
        let before = lines(root);
        let no_list = deferred(root, json!(1), msg);
        assert_eq!(no_list["reason"], json!("deferred-unknown-pending"), "{no_list}");
        add_pending(root, "a única");
        for pending in [json!(7), json!("P-7")] {
            let out = deferred(root, pending, msg);
            assert_eq!(out["reason"], json!("deferred-unknown-pending"), "{out}");
            let hint = out["hint"].as_str().unwrap();
            assert!(hint.contains("P-7") && hint.contains("mustard-rt run pending --add"), "{hint}");
        }
        assert_eq!(lines(root), before, "nothing was written");
    }

    /// Um pedido adiado para uma pendência fechada ou descartada é recusado:
    /// a cobrança da entrega nunca veria esse pedido.
    #[test]
    fn a_deferred_request_pointing_to_a_closed_pending_is_refused() {
        use crate::commands::event::pending::{pending_at, PendingOpts};
        let (dir, msg) = project_with_message();
        let root = dir.path();
        let closed = add_pending(root, "fechada");
        let dropped = add_pending(root, "descartada");
        let settle = |close: Option<String>, drop: Option<String>, confirm: Option<String>| {
            let out = pending_at(&PendingOpts {
                root: root.to_path_buf(),
                close,
                drop,
                confirm,
                reason: Some("resolvida".into()),
                ..PendingOpts::default()
            });
            assert_eq!(out["ok"], json!(true), "{out}");
            out
        };
        settle(Some(closed.clone()), None, None);
        // A remoção sai em duas chamadas: a prévia devolve o código, e a
        // segunda, com ele, descarta.
        let preview = settle(None, Some(dropped.clone()), None);
        let token = preview["token"].as_str().map(str::to_string);
        assert_eq!(settle(None, Some(dropped.clone()), token)["removed"], json!([dropped]));
        let before = lines(root);
        for id in [closed, dropped] {
            let out = deferred(root, json!(id), msg);
            assert_eq!(out["reason"], json!("deferred-closed-pending"), "{out}");
            assert!(out["hint"].as_str().unwrap().contains(&id), "{out}");
        }
        assert_eq!(lines(root), before, "nothing was written");
    }

    /// De um worktree ligado, o pedido adiado confere a lista do checkout
    /// principal, e vai para a spec de lá.
    #[test]
    fn a_deferred_request_from_a_linked_worktree_checks_the_list_of_the_main_checkout() {
        let tmp = tempdir().unwrap();
        let main = tmp.path().join("repo");
        std::fs::create_dir_all(&main).unwrap();
        repo_on(&main, "dev");
        let id = add_pending(&main, "Medir o antivírus do Windows");
        let wt = tmp.path().join("wt");
        let ok = std::process::Command::new("git")
            .args(["worktree", "add", "-q", &wt.to_string_lossy(), "-b", "feature/teste"])
            .current_dir(&main)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git worktree add failed");

        let msg = write(&wt, "message", r#"{"author":"user","text":"e o antivírus?"}"#)["id"].as_u64().unwrap();
        let out = deferred(&wt, json!(id), msg);
        assert_eq!(out["ok"], json!(true), "{out}");
        let missing = deferred(&wt, json!("P-2"), msg);
        assert_eq!(missing["reason"], json!("deferred-unknown-pending"), "{missing}");
        let events = std::fs::read_to_string(main.join(".claude/spec/teste/spec.ndjson")).unwrap();
        assert!(events.contains("\"type\":\"deferred\""), "the request lives in the main checkout: {events}");
        assert!(!wt.join(".claude").exists(), "nothing of the Mustard inside the worktree");
    }

    /// Uma spec em levantamento, nascida pela porta do `open`, sem o git.
    fn surveyed(root: &std::path::Path) {
        assert_eq!(record_open(root, "teste", "feature/teste", "dev"), Ok(true));
    }

    fn message(root: &std::path::Path, author: &str, text: &str) -> u64 {
        write(root, "message", &json!({ "author": author, "text": text }).to_string())["id"].as_u64().unwrap()
    }

    fn context(root: &std::path::Path, text: &str, origin: u64) -> Value {
        write(root, "context", &json!({ "text": text, "origin": origin }).to_string())
    }

    /// O primeiro `context` de uma spec em levantamento é a resposta do
    /// usuário palavra por palavra: outro texto, ou a mensagem que não é do
    /// usuário, é recusado, e nada é gravado; a resposta igual entra, e o
    /// `context` seguinte já não é o objetivo.
    #[test]
    fn the_first_context_of_a_survey_is_the_users_answer_word_for_word() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root);
        let answer = "Travar o merge com pendência aberta.";
        let said = message(root, "user", answer);
        let reply = message(root, "assistant", answer);
        let before = lines(root);
        let reworded = context(root, "Travar o merge.", said);
        assert_eq!(reworded["reason"], json!("goal-not-verbatim"), "{reworded}");
        assert!(reworded["hint"].as_str().unwrap().contains(&said.to_string()), "{reworded}");
        let from_reply = context(root, answer, reply);
        assert_eq!(from_reply["reason"], json!("goal-not-verbatim"), "{from_reply}");
        assert_eq!(lines(root), before, "a refusal writes nothing");
        assert_eq!(context(root, answer, said)["ok"], json!(true));
        assert_eq!(context(root, "Outro contexto, livre.", said)["ok"], json!(true));
    }

    /// O objetivo errado sai com `remove`, e a próxima resposta do usuário
    /// vira o objetivo, também palavra por palavra.
    #[test]
    fn a_removed_goal_lets_the_next_answer_become_the_goal() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root);
        let first = message(root, "user", "Quero isso.");
        let goal = context(root, "Quero isso.", first)["id"].as_u64().unwrap();
        let removal = write(
            root,
            "remove",
            &json!({ "targets": [goal], "reason": "o usuário respondeu outra coisa" }).to_string(),
        );
        assert_eq!(removal["ok"], json!(true), "{removal}");
        let second = message(root, "user", "Travar o merge com pendência aberta.");
        assert_eq!(context(root, "Qualquer coisa.", second)["reason"], json!("goal-not-verbatim"));
        assert_eq!(context(root, "Travar o merge com pendência aberta.", second)["ok"], json!(true));
    }

    /// Uma spec que nasce em plano, como as do `spec-draft`, não tem a vaga do
    /// objetivo: o primeiro `context` dela é livre, e a porta do `open` nunca
    /// a leva de volta ao levantamento.
    #[test]
    fn a_spec_born_in_plan_keeps_a_free_first_context_and_never_returns_to_the_survey() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        born(root);
        let said = message(root, "user", "oi");
        assert_eq!(context(root, "Um contexto qualquer.", said)["ok"], json!(true));
        let before = lines(root);
        assert_eq!(record_open(root, "teste", "feature/teste", "dev"), Ok(false));
        assert_eq!(lines(root), before);
        assert_eq!(DiskSpecState::new(root).state("teste").unwrap().phase, Some("plan"));
    }

    /// O tipo de trabalho sai só pelo `grill`: o `run write work_type` é
    /// recusado nos dois idiomas, o modelo também não o tira, e nada é
    /// gravado.
    #[test]
    fn the_work_type_is_written_only_by_grill() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root);
        let said = message(root, "user", "Travar o merge.");
        let draft = json!({ "kinds": ["feature"], "origin": said }).to_string();
        let before = lines(root);
        let refused = write(root, "work_type", &draft);
        assert_eq!(refused["reason"], json!("work-type-by-grill"), "{refused}");
        assert!(refused["hint"].as_str().unwrap().contains("mustard-rt run grill"), "{refused}");
        assert_eq!(lines(root), before);

        let mut by_grill = Map::new();
        by_grill.insert("kinds".to_string(), json!(["feature"]));
        by_grill.insert("origin".to_string(), json!(said));
        let recorded = record(root, "teste", "work_type", by_grill, PhaseWriter::Binary).unwrap().written.id;
        let before = lines(root);
        let removal = write(root, "remove", &json!({ "targets": [recorded], "reason": "outro tipo" }).to_string());
        assert_eq!(removal["reason"], json!("work-type-by-grill"), "{removal}");
        assert_eq!(lines(root), before);

        std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"en-US"}}"#).unwrap();
        let english = write(root, "work_type", &draft);
        assert!(english["hint"].as_str().unwrap().contains("is written by `mustard-rt run grill`"), "{english}");
    }
}
