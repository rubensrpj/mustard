//! `spec_state` — qual é a spec atual e em que estado ela está.
//!
//! Sem disco. Duas perguntas moram aqui:
//!
//! - **Qual é a spec atual.** Uma escada só, na mesma ordem para todo mundo:
//!   a variável de ambiente `MUSTARD_ACTIVE_SPEC`, depois a branch em que o
//!   checkout está, depois a spec ligada à sessão. Cada degrau chega já lido
//!   ([`resolve`]); quem lê o ambiente, o `.git/HEAD` e a ligação da sessão é a
//!   implementação de [`SpecState`] do lado de fora.
//! - **Em que estado a spec está.** O [`State`] é a dobra dos eventos `state`
//!   do `spec.ndjson`, em ordem de número: cada campo presente substitui o
//!   anterior, e o ausente herda. Uma versão revista de um `state` entra na
//!   dobra no lugar do item que ela substitui, e não no fim.
//!
//! O estado que a trava da aprovação lê tem uma regra só, [`lock_state_of`]:
//! com algum `state`, vale a dobra deles; sem nenhum e com `meta.json`, vale
//! o estágio dele (parada antes da execução trava; encerrada ou em execução
//! pelo fluxo velho, não); sem nenhum, sem `meta.json` e com o `spec.ndjson`,
//! a spec conta como em plano, e trava. Livre só quando não há nem o arquivo
//! de eventos nem o `meta.json`: a branch que o Mustard não abriu.
//!
//! Quem decide (o portão de escrita, a testemunha da aprovação, a cobrança das
//! pendências) recebe os valores já lidos e se testa sem disco, com uma
//! [`SpecState`] falsa.
//!
//! O resultado dos critérios ([`qa`]), o veredito das ondas ([`review`]) e os
//! pedidos depois da última execução ([`requests_after`]) também moram aqui:
//! toda porta que pergunta "o QA passou?" ou "a revisão reprovou?" lê a mesma
//! resposta do mesmo arquivo.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::domain::meta::Meta;
use crate::domain::spec_events::{Block, BlockQuery, Refusal, SpecEvent, SpecLog, PHASES};

/// As fases em que a spec já foi aprovada pelo usuário.
const APPROVED_PHASES: &[&str] = &["approved", "running", "closed", "pr_open", "delivered"];

/// As fases que disparam a cobrança das pendências no fim da resposta: a spec
/// fechou, ou entrou no merge.
const CLOSING_PHASES: &[&str] = &["closed", "delivered"];

/// As fases de uma spec que continua fechada: fechada, com o pull request
/// aberto ou entregue.
const SETTLED_PHASES: &[&str] = &["closed", "pr_open", "delivered"];

/// O estado de uma spec, dobrado dos eventos `state` que a leitura mostra.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// A fase atual, uma de [`PHASES`]; `None` quando o arquivo da spec não
    /// tem nenhum evento `state` visível. Uma spec assim não está aprovada.
    pub phase: Option<&'static str>,
    /// A fase atual é a de uma spec aprovada: `approved`, `running`,
    /// `closed`, `pr_open` ou `delivered`.
    pub approved: bool,
    /// A branch em que a spec mora, herdada do último evento que a disse.
    pub branch: Option<String>,
    /// A base de que a branch foi cortada, herdada do mesmo jeito.
    pub base: Option<String>,
    /// O número do último evento `state` com a fase `closed` ou `delivered`,
    /// na ordem da dobra: cada fechamento e cada merge trazem um número novo.
    pub last_closing: Option<u64>,
    /// A testemunha da última aprovação gravada, `{question, answer}`.
    pub witness: Option<Value>,
}

impl State {
    /// Dobra os eventos `state` visíveis, em ordem de número. Uma versão
    /// revista entra no lugar do item que ela substitui: corrigir a branch do
    /// primeiro `state` não traz de volta a fase dele por cima das que vieram
    /// depois. Uma fase que este binário não conhece é ignorada, e a anterior
    /// fica.
    #[must_use]
    pub fn from_log(log: &SpecLog) -> Self {
        let mut states: Vec<(u64, &SpecEvent)> = log
            .block(BlockQuery::Block(Block::State))
            .into_iter()
            .filter(|event| event.event_type == "state")
            .map(|event| (original_of(log, event), event))
            .collect();
        states.sort_by_key(|(at, event)| (*at, event.id));

        let mut state = Self::default();
        for (_, event) in states {
            if let Some(phase) = event.str_field("phase").and_then(known_phase) {
                state.phase = Some(phase);
                if CLOSING_PHASES.contains(&phase) {
                    state.last_closing = Some(event.id);
                }
            }
            if let Some(branch) = text(event.str_field("branch")) {
                state.branch = Some(branch);
            }
            if let Some(base) = text(event.str_field("base")) {
                state.base = Some(base);
            }
            if let Some(witness) = event.fields.get("witness").filter(|w| w.is_object()) {
                state.witness = Some(witness.clone());
            }
        }
        state.approved = state.phase.is_some_and(|p| APPROVED_PHASES.contains(&p));
        state
    }

    /// O fechamento que a cobrança das pendências confere: o número do último
    /// `state` de fechamento ou de entrega, enquanto a spec continua fechada.
    /// Uma spec reaberta não cobra nada até fechar de novo, e então traz um
    /// número novo.
    #[must_use]
    pub fn closing(&self) -> Option<u64> {
        self.last_closing.filter(|_| self.phase.is_some_and(|p| SETTLED_PHASES.contains(&p)))
    }
}

/// A fase é a de uma spec aprovada. A lista é uma só: a que o [`State`] dobra
/// e a que o portão de escrita lê.
#[must_use]
pub fn is_approved_phase(phase: &str) -> bool {
    APPROVED_PHASES.contains(&phase.trim())
}

/// Quem grava uma mudança de fase no `spec.ndjson`. O modelo não está aqui:
/// o `run write` nunca grava o estado, que é dos comandos do fluxo e da
/// testemunha da aprovação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseWriter {
    /// A testemunha da aprovação, com a resposta do usuário.
    Witness,
    /// O próprio binário: o nascimento da spec e a ponte do fechamento e do
    /// merge.
    Binary,
}

/// A regra única da mudança de fase, para as portas do binário que gravam no
/// `spec.ndjson`: uma gravação que leva o estado de `before` a `after`,
/// trazendo no próprio evento a fase `carried` quando ele é um `state`, feita
/// por `by`, é permitida?
///
/// - `approved` só pela testemunha, e só de nenhuma fase ou de `plan`.
/// - As fases depois da aprovação (`running`, `closed`, `pr_open`,
///   `delivered`) só pelo binário, e só a partir de uma fase aprovada.
/// - Uma fase que não aprova (`survey`, `plan`, `discarded`) só fecha a
///   trava, e só o binário a grava.
/// - A branch e a base, só o binário: no nascimento, ou completando a que
///   falta. O portão deixa de travar numa branch diferente da gravada.
/// - A fase nunca some: tirar o último `state` voltaria a spec ao nascimento.
///
/// `carried` é a fase que o evento traz como mudança: numa revisão que
/// repete a fase do item revisto, quem chama não passa fase nenhuma.
///
/// O modelo não passa por aqui: o `run write` recusa o tipo `state`, e
/// nenhuma outra gravação dele pode mudar o estado.
#[must_use]
pub fn phase_write_allowed(before: &State, after: &State, carried: Option<&str>, by: PhaseWriter) -> bool {
    if by != PhaseWriter::Binary && (before.branch != after.branch || before.base != after.base) {
        return false;
    }
    if carried.map(str::trim) == Some("approved") && by != PhaseWriter::Witness {
        return false;
    }
    if before.phase == after.phase {
        return true;
    }
    let Some(to) = after.phase else {
        return false;
    };
    if !is_approved_phase(to) {
        return by == PhaseWriter::Binary;
    }
    match by {
        PhaseWriter::Witness => to == "approved" && matches!(before.phase, None | Some("plan")),
        PhaseWriter::Binary => to != "approved" && before.approved,
    }
}

/// O objetivo de uma spec em levantamento é a resposta do usuário, palavra
/// por palavra: o primeiro `context` visível aponta em `origin` uma mensagem
/// do usuário e repete o texto dela. A regra olha o arquivo antes e depois de
/// uma gravação: numa spec em levantamento ainda sem `context` visível, todo
/// `context` que a gravação traz é o objetivo. Fora do levantamento, ou com o
/// objetivo já gravado, qualquer `context` passa; o objetivo tirado com
/// `remove` deixa a vaga aberta para a próxima resposta.
///
/// # Errors
///
/// [`Refusal::GoalNotVerbatim`] quando o objetivo não aponta uma mensagem do
/// usuário ou não repete o texto dela.
pub fn goal_rule(spec: &str, before: &SpecLog, after: &SpecLog) -> Result<(), Refusal> {
    let contexts = |log: &SpecLog| -> usize { log.visible().iter().filter(|e| e.event_type == "context").count() };
    if State::from_log(before).phase != Some("survey") || contexts(before) > 0 {
        return Ok(());
    }
    for goal in after.visible().into_iter().filter(|e| e.event_type == "context") {
        let origin = goal.int("origin");
        let answer = origin
            .and_then(|id| after.get(id))
            .filter(|m| m.event_type == "message" && m.str_field("author").map(str::trim) == Some("user"))
            .and_then(|m| m.str_field("text"))
            .map(str::trim);
        if answer.is_none() || answer != goal.str_field("text").map(str::trim) {
            return Err(Refusal::GoalNotVerbatim {
                spec: spec.trim().to_string(),
                origin: origin.map_or_else(|| "-".to_string(), |id| id.to_string()),
            });
        }
    }
    Ok(())
}

/// O `meta.json` de uma spec parada antes da execução: em análise ou em plano
/// (ou sem estágio), e ativa (ou sem desfecho). Uma encerrada, ou em execução
/// pelo fluxo velho, não.
#[must_use]
pub fn meta_before_run(meta: &Meta) -> bool {
    let before_run = meta
        .stage
        .as_deref()
        .is_none_or(|stage| ["Analyze", "Plan"].iter().any(|s| stage.trim().eq_ignore_ascii_case(s)));
    let active = meta.outcome.as_deref().is_none_or(|outcome| outcome.trim().eq_ignore_ascii_case("Active"));
    before_run && active
}

/// O estado que a trava da aprovação lê: a regra escrita uma vez, para todos
/// os leitores, a partir do arquivo de eventos (`log`) e do `meta.json`
/// (`meta`) da spec, quando existem.
///
/// - Com algum `state` no arquivo, vale a dobra deles.
/// - Sem nenhum `state` e com `meta.json`, vale o estágio dele: parada antes
///   da execução, em plano; encerrada ou em execução pelo fluxo velho, `None`.
/// - Sem nenhum `state`, sem `meta.json` e com o arquivo, em plano: o
///   "Aprovar" faz nascer e aprova.
/// - `None` só sem arquivo e sem `meta.json`: a branch que o Mustard não
///   abriu.
///
/// A spec em plano pelas regras sem `state` não tem branch.
#[must_use]
pub fn lock_state_of(log: Option<&SpecLog>, meta: Option<&Meta>) -> Option<State> {
    if let Some(log) = log.filter(|log| birth_event(log).is_some()) {
        return Some(State::from_log(log));
    }
    let plan = State { phase: Some("plan"), ..State::default() };
    match meta {
        Some(meta) => meta_before_run(meta).then_some(plan),
        None => log.map(|_| plan),
    }
}

/// O `state` do nascimento: o primeiro na ordem da dobra, já com a revisão
/// dele no lugar, quando houver. `None` num arquivo sem `state` visível.
#[must_use]
pub fn birth_event(log: &SpecLog) -> Option<&SpecEvent> {
    log.block(BlockQuery::Block(Block::State))
        .into_iter()
        .filter(|event| event.event_type == "state")
        .min_by_key(|event| (original_of(log, event), event.id))
}

/// O número do item que `event` revê: segue os `replaces` para trás até a
/// primeira versão. Um evento que não revê nada é o próprio item.
pub(crate) fn original_of(log: &SpecLog, event: &SpecEvent) -> u64 {
    let mut at = event.id;
    // Uma cadeia nunca é maior que o arquivo; o limite só corta um laço feito
    // à mão.
    for _ in 0..=log.events.len() {
        match log.get(at).and_then(|e| e.int("replaces")) {
            Some(older) if older != at => at = older,
            _ => break,
        }
    }
    at
}

/// A fase como a constante de [`PHASES`]; `None` para um nome desconhecido.
fn known_phase(name: &str) -> Option<&'static str> {
    let name = name.trim();
    PHASES.iter().copied().find(|p| *p == name)
}

/// O texto sem espaços nas pontas, ou `None` quando fica vazio.
fn text(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// Os critérios de uma spec diante das execuções gravadas.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Qa {
    /// Os critérios que a leitura mostra.
    pub criteria: usize,
    /// Os critérios com ao menos uma execução.
    pub ran: usize,
    /// Os critérios cuja última execução passou.
    pub passed: usize,
    /// Os critérios cuja última execução falhou.
    pub failed: usize,
    /// Os critérios revistos depois da última execução deles: a execução
    /// conferiu o texto de antes.
    pub stale: usize,
    /// O número da execução mais nova de um critério que a leitura mostra.
    pub last_run: Option<u64>,
}

impl Qa {
    /// O QA passou: a spec tem critério, e cada critério que a leitura mostra
    /// tem a última execução aprovada.
    #[must_use]
    pub fn passed_all(&self) -> bool {
        self.criteria > 0 && self.passed == self.criteria
    }
}

/// O QA de uma spec, pelas execuções (`criterion_run`) que a leitura mostra: a
/// mais nova de cada critério decide. Uma execução que nomeia uma versão
/// anterior de um critério conta para a versão que a substituiu, e o critério
/// revisto depois dela entra em [`Qa::stale`]. A execução de um critério
/// removido não conta.
#[must_use]
pub fn qa(log: &SpecLog) -> Qa {
    let block = log.block(BlockQuery::Block(Block::Criteria));
    let criteria: Vec<u64> = block.iter().filter(|e| e.event_type == "criterion").map(|e| e.id).collect();
    // A execução mais nova de cada critério: (número, passou).
    let mut last: BTreeMap<u64, (u64, bool)> = BTreeMap::new();
    for run in block.iter().filter(|e| e.event_type == "criterion_run") {
        let Some(current) = run.int("criterion").and_then(|named| log.current(named)) else {
            continue;
        };
        let pass = run.str_field("result").map(str::trim) == Some("pass");
        let entry = last.entry(current.id).or_insert((run.id, pass));
        if run.id >= entry.0 {
            *entry = (run.id, pass);
        }
    }
    let mut qa = Qa { criteria: criteria.len(), ..Qa::default() };
    for id in &criteria {
        let Some((run, pass)) = last.get(id) else {
            continue;
        };
        qa.ran += 1;
        if *pass {
            qa.passed += 1;
        } else {
            qa.failed += 1;
        }
        if id > run {
            qa.stale += 1;
        }
        qa.last_run = qa.last_run.max(Some(*run));
    }
    qa
}

/// O veredito das revisões de uma spec.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Review {
    /// Alguma onda tem veredito gravado.
    pub any: bool,
    /// O último veredito de alguma onda reprovou.
    pub rejected: bool,
}

impl Review {
    /// A palavra do veredito: `approved` quando o último veredito de cada onda
    /// aprovou, `rejected` quando o de alguma reprovou; `None` sem veredito.
    #[must_use]
    pub fn word(&self) -> Option<&'static str> {
        self.any.then_some(if self.rejected { "rejected" } else { "approved" })
    }
}

/// O veredito de uma spec, pelos eventos `verdict` que a leitura mostra: o
/// mais novo de cada onda decide, e uma aprovação de uma onda não esconde a
/// reprovação de outra.
#[must_use]
pub fn review(log: &SpecLog) -> Review {
    // O veredito mais novo de cada onda: (número, reprovou).
    let mut last: BTreeMap<u64, (u64, bool)> = BTreeMap::new();
    for verdict in log.block(BlockQuery::Block(Block::Review)).into_iter().filter(|e| e.event_type == "verdict") {
        let Some(wave) = verdict.wave() else {
            continue;
        };
        let rejected = verdict.str_field("result").map(str::trim) == Some("rejected");
        let entry = last.entry(wave).or_insert((verdict.id, rejected));
        if verdict.id >= entry.0 {
            *entry = (verdict.id, rejected);
        }
    }
    Review { any: !last.is_empty(), rejected: last.values().any(|(_, rejected)| *rejected) }
}

/// Os pedidos (`request`) que a leitura mostra e que vieram depois do evento
/// de número `after`, em ordem: mudanças que a última execução dos critérios
/// não conferiu.
#[must_use]
pub fn requests_after(log: &SpecLog, after: u64) -> Vec<&SpecEvent> {
    log.block(BlockQuery::Block(Block::Notes))
        .into_iter()
        .filter(|event| event.event_type == "request" && event.id > after)
        .collect()
}

/// A aprovação que vale: o `state` visível mais novo, na ordem da dobra, com a
/// fase `approved`, enquanto a dobra lê a spec aprovada. `None` numa spec que
/// nunca foi aprovada e numa que voltou a uma fase de antes da aprovação. O
/// leitor da aprovação do rt lê daqui; a página e o aviso de crescimento das
/// ondas medem pela fronteira dela ([`approval_boundary`]).
#[must_use]
pub fn approval_event(log: &SpecLog) -> Option<&SpecEvent> {
    if !State::from_log(log).approved {
        return None;
    }
    log.block(BlockQuery::Block(Block::State))
        .into_iter()
        .filter(|event| event.event_type == "state" && event.str_field("phase").map(str::trim) == Some("approved"))
        .max_by_key(|event| (original_of(log, event), event.id))
}

/// A fronteira da aprovação que vale: o número da primeira versão dela. Uma
/// aprovação revista, como a da spec antiga que nasceu aprovada e ganhou a
/// branch depois, continua valendo desde onde foi dada: o que veio entre ela
/// e a revisão já é depois da aprovação. A página e o aviso de crescimento
/// das ondas medem daqui.
#[must_use]
pub fn approval_boundary(log: &SpecLog) -> Option<u64> {
    approval_event(log).map(|event| original_of(log, event))
}

/// Quantas ondas a leitura mostrava logo depois do evento de número `id`,
/// contando só os eventos até ele. Uma onda conta pelo número dela: a versão
/// nova de uma onda não soma, e uma onda tirada depois de `id` ainda conta.
#[must_use]
pub fn waves_at(log: &SpecLog, id: u64) -> usize {
    let until = SpecLog { events: log.events.iter().filter(|event| event.id <= id).cloned().collect(), skipped: Vec::new() };
    waves_now(&until)
}

/// Quantas ondas a leitura mostra agora, cada uma pelo número dela.
#[must_use]
pub fn waves_now(log: &SpecLog) -> usize {
    log.visible()
        .into_iter()
        .filter(|event| event.event_type == "wave")
        .filter_map(SpecEvent::wave)
        .collect::<BTreeSet<u64>>()
        .len()
}

/// O crescimento que o evento de número `id` trouxe às ondas depois da
/// aprovação que vale: `(aprovadas, agora)`, quando ele somou uma onda e a
/// spec passou a ter mais ondas do que tinha na aprovação. `None` numa spec
/// sem aprovação, num evento que não somou onda (a versão nova de uma onda,
/// por exemplo) e quando a conta não passa da aprovada. É só um aviso: o
/// crescimento nunca barra a gravação.
#[must_use]
pub fn waves_grown_by(log: &SpecLog, id: u64) -> Option<(usize, usize)> {
    let approved = waves_at(log, approval_boundary(log)?);
    let now = waves_at(log, id);
    (now > approved && now > waves_at(log, id.saturating_sub(1))).then_some((approved, now))
}

/// Algum `state` que a leitura mostra, com a fase `phase`, foi gravado no
/// instante `since_secs` (segundos desde a época) ou depois. A hora do evento
/// traz o fuso e vem em segundos; uma hora que não se lê não conta.
#[must_use]
pub fn phase_recorded_since(log: &SpecLog, phase: &str, since_secs: i64) -> bool {
    log.block(BlockQuery::Block(Block::State))
        .into_iter()
        .filter(|event| event.event_type == "state" && event.str_field("phase").map(str::trim) == Some(phase))
        .filter_map(|event| chrono::DateTime::parse_from_rfc3339(event.at()).ok())
        .any(|at| at.timestamp() >= since_secs)
}

/// A escada de "qual é a spec atual": a variável de ambiente, depois a spec da
/// branch em que o checkout está, depois a spec ligada à sessão. O primeiro
/// degrau com um nome vence; um nome em branco não conta.
#[must_use]
pub fn resolve(
    env: Option<&str>,
    branch_spec: Option<String>,
    session_spec: Option<String>,
) -> Option<String> {
    text(env).or_else(|| text(branch_spec.as_deref())).or_else(|| text(session_spec.as_deref()))
}

/// Qual é a spec atual e em que estado cada spec está. A implementação de
/// verdade lê o disco; a dos testes devolve valores prontos.
pub trait SpecState {
    /// A spec atual, pela escada de [`resolve`], para a sessão `session`.
    fn active(&self, session: Option<&str>) -> Option<String>;
    /// A dobra dos `state` da spec `spec`; `None` só quando ela não tem
    /// arquivo de eventos. A trava não lê daqui sozinha: ela passa por
    /// [`lock_state_of`], que junta o arquivo e o `meta.json`.
    fn state(&self, spec: &str) -> Option<State>;
    /// O arquivo de eventos da spec inteiro, para quem precisa de outro bloco;
    /// `None` quando ele não existe.
    fn log(&self, spec: &str) -> Option<SpecLog>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::parse_log;
    use serde_json::json;

    fn log(lines: &[Value]) -> SpecLog {
        let body: Vec<String> = lines.iter().map(Value::to_string).collect();
        parse_log(&body.join("\n"))
    }

    fn at(phase: Option<&'static str>) -> State {
        State { phase, approved: phase.is_some_and(is_approved_phase), ..State::default() }
    }

    /// A regra da trava, nos quatro casos: com `state`, a dobra; sem `state`
    /// e com `meta.json`, o estágio dele; sem os dois e com o arquivo, em
    /// plano; sem nada, livre.
    #[test]
    fn the_lock_rule_reads_the_state_then_the_meta_then_the_file() {
        let meta = |stage: &str, outcome: &str| Meta {
            stage: Some(stage.to_string()),
            outcome: Some(outcome.to_string()),
            ..Meta::default()
        };
        let approved = log(&[json!({ "v": 1, "id": 1, "type": "state", "phase": "approved" })]);
        let note = log(&[json!({ "v": 1, "id": 1, "type": "message", "author": "user", "text": "oi" })]);
        let phase = |log: Option<&SpecLog>, meta: Option<&Meta>| lock_state_of(log, meta).map(|state| state.phase);

        // Com `state`, vale a dobra, diga o `meta.json` o que disser.
        assert!(lock_state_of(Some(&approved), Some(&meta("Plan", "Active"))).is_some_and(|state| state.approved));
        // Sem `state` e com `meta.json`: antes da execução trava; depois, não.
        for (stage, outcome, locks) in [
            ("Analyze", "Active", true),
            ("Plan", "Active", true),
            ("Execute", "Active", false),
            ("Close", "Completed", false),
            ("Plan", "Cancelled", false),
        ] {
            let expected = locks.then_some(Some("plan"));
            assert_eq!(phase(Some(&note), Some(&meta(stage, outcome))), expected, "{stage}/{outcome}, a note");
            assert_eq!(phase(None, Some(&meta(stage, outcome))), expected, "{stage}/{outcome}, no file");
        }
        // Sem `state`, sem `meta.json` e com o arquivo: em plano.
        assert_eq!(phase(Some(&note), None), Some(Some("plan")));
        // Sem nada: a branch que o Mustard não abriu, livre.
        assert_eq!(phase(None, None), None);
    }

    /// Cada fase tem a sua porta: a aprovação só pela testemunha, a partir do
    /// plano; as fases depois dela só pelo binário, a partir de uma aprovada;
    /// a branch e a base só pelo binário.
    #[test]
    fn the_phase_rule_has_one_door_per_phase() {
        use PhaseWriter::{Binary, Witness};
        let allowed = |from, to, by| phase_write_allowed(&at(from), &at(to), to, by);

        assert!(allowed(Some("plan"), Some("approved"), Witness));
        assert!(allowed(None, Some("approved"), Witness));
        assert!(!allowed(Some("survey"), Some("approved"), Witness), "no approval before the plan");
        assert!(!allowed(Some("plan"), Some("approved"), Binary), "the binary never approves");

        for later in ["running", "closed", "pr_open", "delivered"] {
            assert!(allowed(Some("approved"), Some(later), Binary), "{later} after the approval");
            assert!(!allowed(Some("plan"), Some(later), Binary), "{later} never skips the approval");
            assert!(!allowed(Some("approved"), Some(later), Witness));
        }

        for locking in ["survey", "plan", "discarded"] {
            assert!(allowed(Some("running"), Some(locking), Binary), "{locking} only locks");
            assert!(allowed(None, Some(locking), Binary));
            assert!(!allowed(None, Some(locking), Witness), "the witness only approves");
        }

        // A fase nunca some: tirar o último `state` não é gravação de porta
        // nenhuma.
        for by in [Witness, Binary] {
            assert!(!phase_write_allowed(&at(Some("plan")), &at(None), None, by), "{by:?}");
        }

        // A branch e a base, só o binário: no nascimento, ou completando a
        // que falta.
        let on = |branch: Option<&str>, base: &str| State {
            phase: Some("plan"),
            branch: branch.map(str::to_string),
            base: Some(base.to_string()),
            ..State::default()
        };
        assert!(phase_write_allowed(&at(None), &on(Some("feature/x"), "dev"), Some("plan"), Binary), "the birth");
        assert!(
            phase_write_allowed(&on(None, "dev"), &on(Some("feature/x"), "dev"), Some("plan"), Binary),
            "completing the missing branch",
        );
        assert!(!phase_write_allowed(&on(Some("feature/x"), "dev"), &on(Some("outra"), "dev"), Some("plan"), Witness));
        assert!(!phase_write_allowed(&on(Some("feature/x"), "dev"), &on(Some("feature/x"), "main"), Some("plan"), Witness));
    }

    #[test]
    fn the_ladder_takes_the_environment_then_the_branch_then_the_session() {
        let branch = || Some("da-branch".to_string());
        let session = || Some("da-sessao".to_string());
        assert_eq!(resolve(Some("do-ambiente"), branch(), session()).as_deref(), Some("do-ambiente"));
        assert_eq!(resolve(None, branch(), session()).as_deref(), Some("da-branch"));
        assert_eq!(resolve(None, None, session()).as_deref(), Some("da-sessao"));
        assert_eq!(resolve(None, None, None), None);
        // Um degrau em branco não responde, e a escada desce.
        assert_eq!(resolve(Some("  "), Some(String::new()), session()).as_deref(), Some("da-sessao"));
    }

    #[test]
    fn the_state_folds_the_state_events_in_order() {
        let log = log(&[
            json!({"v":1,"id":1,"type":"state","phase":"survey","branch":"feature/x","base":"dev"}),
            json!({"v":1,"id":2,"type":"rule","text":"t","keys":["k"],"example":"e","origin":1}),
            json!({"v":1,"id":3,"type":"state","phase":"plan"}),
            json!({"v":1,"id":4,"type":"state","phase":"approved",
                   "witness":{"question":"Aprova?","answer":"Aprovar"}}),
            json!({"v":1,"id":5,"type":"state","phase":"closed"}),
            json!({"v":1,"id":6,"type":"state","phase":"delivered"}),
        ]);
        let state = State::from_log(&log);
        assert_eq!(state.phase, Some("delivered"));
        assert!(state.approved, "a delivered spec was approved");
        assert_eq!(state.branch.as_deref(), Some("feature/x"), "the branch is inherited");
        assert_eq!(state.base.as_deref(), Some("dev"), "the base is inherited");
        assert_eq!(state.last_closing, Some(6), "the merge after the close is the newest trigger");
        assert_eq!(state.closing(), Some(6));
        assert_eq!(state.witness, Some(json!({"question":"Aprova?","answer":"Aprovar"})));
    }

    /// Cada fechamento e cada merge trazem um gatilho novo; uma spec reaberta
    /// não cobra até fechar de novo, e o pull request aberto não apaga o
    /// fechamento.
    #[test]
    fn every_closing_and_every_merge_bring_a_new_trigger() {
        let mut lines = vec![json!({"v":1,"id":1,"type":"state","phase":"running"})];
        let mut closing_after = |line: Value| {
            lines.push(line);
            State::from_log(&log(&lines)).closing()
        };
        assert_eq!(closing_after(json!({"v":1,"id":2,"type":"state","phase":"closed"})), Some(2));
        assert_eq!(closing_after(json!({"v":1,"id":3,"type":"state","phase":"running"})), None, "reopened");
        assert_eq!(closing_after(json!({"v":1,"id":4,"type":"state","phase":"closed"})), Some(4), "closed again");
        assert_eq!(closing_after(json!({"v":1,"id":5,"type":"state","phase":"pr_open"})), Some(4));
        assert_eq!(closing_after(json!({"v":1,"id":6,"type":"state","phase":"delivered"})), Some(6), "merged");
    }

    /// Uma edição à mão pode deixar as linhas fora da ordem dos números; a
    /// dobra segue o número, e o `state` mais novo continua valendo.
    #[test]
    fn the_fold_follows_the_event_number_not_the_line_order() {
        let log = log(&[
            json!({"v":1,"id":2,"type":"state","phase":"approved",
                   "witness":{"question":"Aprova?","answer":"Aprovar"}}),
            json!({"v":1,"id":1,"type":"state","phase":"plan","branch":"feature/x"}),
        ]);
        let state = State::from_log(&log);
        assert_eq!(state.phase, Some("approved"), "the newer number wins over the later line");
        assert!(state.approved);
        assert_eq!(state.branch.as_deref(), Some("feature/x"));
    }

    /// Corrigir a branch do primeiro `state` não desfaz a aprovação que veio
    /// depois: a versão revista entra no lugar do item que ela substitui.
    #[test]
    fn a_revised_state_folds_where_the_item_it_replaces_stood() {
        let log = log(&[
            json!({"v":1,"id":1,"type":"state","phase":"plan","branch":"feature/erro"}),
            json!({"v":1,"id":2,"type":"state","phase":"approved",
                   "witness":{"question":"Aprova?","answer":"Aprovar"}}),
            json!({"v":1,"id":3,"type":"state","phase":"plan","branch":"feature/certa","replaces":1}),
        ]);
        let state = State::from_log(&log);
        assert_eq!(state.phase, Some("approved"), "the revision of the first state stays first");
        assert!(state.approved);
        assert_eq!(state.branch.as_deref(), Some("feature/certa"), "the revision still counts");
    }

    #[test]
    fn a_plan_is_not_approved_and_a_removed_state_does_not_count() {
        let log = log(&[
            json!({"v":1,"id":1,"type":"state","phase":"plan","branch":"feature/x"}),
            json!({"v":1,"id":2,"type":"state","phase":"approved",
                   "witness":{"question":"Aprova?","answer":"Aprovar"}}),
            json!({"v":1,"id":3,"type":"remove","targets":[2],"reason":"engano"}),
        ]);
        let state = State::from_log(&log);
        assert_eq!(state.phase, Some("plan"));
        assert!(!state.approved);
        assert_eq!(state.witness, None);
        assert_eq!(state.last_closing, None);
    }

    #[test]
    fn an_unknown_phase_keeps_the_previous_one() {
        let log = log(&[
            json!({"v":1,"id":1,"type":"state","phase":"running"}),
            json!({"v":1,"id":2,"type":"state","phase":"inventada"}),
        ]);
        let state = State::from_log(&log);
        assert_eq!(state.phase, Some("running"));
        assert!(state.approved);
    }

    /// Um arquivo sem nenhum `state` visível dá um estado sem fase e não
    /// aprovado, e não "spec que o Mustard não abriu": tirar o único `state`
    /// não destrava a spec.
    #[test]
    fn a_log_without_visible_state_events_is_not_approved() {
        let log = log(&[
            json!({"v":1,"id":1,"type":"state","phase":"plan","branch":"feature/x"}),
            json!({"v":1,"id":2,"type":"remove","targets":[1],"reason":"engano"}),
        ]);
        let state = State::from_log(&log);
        assert_eq!(state.phase, None);
        assert!(!state.approved);
        assert_eq!(state, State::default());
    }

    fn criterion(id: u64) -> Value {
        json!({"v":1,"id":id,"type":"criterion","when":"w","then":"t","proof":"p","origin":1})
    }

    fn run(id: u64, criterion: u64, result: &str) -> Value {
        json!({"v":1,"id":id,"type":"criterion_run","criterion":criterion,"result":result,"exit":0,"ms":1})
    }

    /// O QA passa só quando cada critério que a leitura mostra tem a última
    /// execução aprovada; um critério sem execução segura o QA, e uma spec
    /// sem critério nunca passa.
    #[test]
    fn the_qa_passes_only_when_every_criterion_last_ran_green() {
        let both = log(&[criterion(2), criterion(3), run(4, 2, "pass"), run(5, 3, "pass")]);
        let qa_both = qa(&both);
        assert!(qa_both.passed_all(), "{qa_both:?}");
        assert_eq!((qa_both.criteria, qa_both.ran, qa_both.passed, qa_both.last_run), (2, 2, 2, Some(5)));

        let one_unrun = qa(&log(&[criterion(2), criterion(3), run(4, 2, "pass")]));
        assert!(!one_unrun.passed_all(), "a criterion with no run holds the QA: {one_unrun:?}");
        assert_eq!((one_unrun.ran, one_unrun.failed), (1, 0));

        let none = qa(&log(&[json!({"v":1,"id":1,"type":"note","text":"t","keys":[],"origin":1})]));
        assert_eq!(none, Qa::default());
        assert!(!none.passed_all(), "a spec with no criterion has nothing to claim");
    }

    /// A execução mais nova de cada critério decide: um verde depois de um
    /// vermelho passa, e um vermelho depois de um verde segura a spec aberta.
    #[test]
    fn a_criterion_whose_last_run_failed_keeps_the_spec_open() {
        let fixed = qa(&log(&[criterion(2), run(3, 2, "fail"), run(4, 2, "pass")]));
        assert!(fixed.passed_all(), "{fixed:?}");
        let broke = qa(&log(&[criterion(2), run(3, 2, "pass"), run(4, 2, "fail")]));
        assert!(!broke.passed_all(), "{broke:?}");
        assert_eq!((broke.passed, broke.failed), (0, 1));
    }

    /// A execução de um critério revisto depois dela conta para a versão nova,
    /// mas o critério fica velho; uma execução removida não conta.
    #[test]
    fn a_criterion_revised_after_its_run_is_stale_and_a_removed_run_does_not_count() {
        let revised = log(&[
            criterion(2),
            run(3, 2, "pass"),
            json!({"v":1,"id":4,"type":"criterion","when":"w2","then":"t","proof":"p","origin":1,"replaces":2}),
        ]);
        let stale = qa(&revised);
        assert!(stale.passed_all(), "the run still counts for the revision: {stale:?}");
        assert_eq!(stale.stale, 1, "the run checked the text before the revision");

        let removed = qa(&log(&[
            criterion(2),
            run(3, 2, "pass"),
            json!({"v":1,"id":4,"type":"remove","targets":[3],"reason":"engano"}),
        ]));
        assert_eq!((removed.ran, removed.last_run), (0, None), "{removed:?}");
        assert!(!removed.passed_all());
    }

    /// O veredito mais novo de cada onda decide, e a aprovação de uma onda
    /// não esconde a reprovação de outra.
    #[test]
    fn the_review_takes_the_last_verdict_of_each_wave() {
        let verdict = |id: u64, wave: u64, result: &str| {
            json!({"v":1,"id":id,"type":"verdict","wave":wave,"result":result,"text":"t",
                   "criteria":[{"criterion":1,"tests_rule":"r"}]})
        };
        assert_eq!(review(&log(&[])), Review::default());
        assert_eq!(review(&log(&[])).word(), None);

        let other_wave = review(&log(&[verdict(1, 2, "rejected"), verdict(2, 1, "approved")]));
        assert_eq!(other_wave, Review { any: true, rejected: true });
        assert_eq!(other_wave.word(), Some("rejected"));

        let fixed = review(&log(&[verdict(1, 2, "rejected"), verdict(2, 1, "approved"), verdict(3, 2, "approved")]));
        assert_eq!(fixed, Review { any: true, rejected: false });
        assert_eq!(fixed.word(), Some("approved"));
    }

    /// Um `state` conta a partir do instante em que foi gravado, lido com o
    /// fuso dele; outra fase, uma hora ilegível e um `state` removido, não.
    #[test]
    fn a_phase_counts_from_the_instant_it_was_recorded() {
        let closed = log(&[
            json!({"v":1,"id":1,"at":"2026-09-13T09:00:00-03:00","type":"state","phase":"approved"}),
            json!({"v":1,"id":2,"at":"2026-09-13T10:00:00-03:00","type":"state","phase":"closed"}),
            json!({"v":1,"id":3,"at":"ontem","type":"state","phase":"closed"}),
        ]);
        // 10:00 em -03:00 é 13:00 UTC.
        let at_13 = chrono::DateTime::parse_from_rfc3339("2026-09-13T13:00:00Z").unwrap().timestamp();
        assert!(phase_recorded_since(&closed, "closed", at_13));
        assert!(!phase_recorded_since(&closed, "closed", at_13 + 1), "the close came before");
        assert!(!phase_recorded_since(&closed, "delivered", 0), "another phase does not count");

        let removed = log(&[
            json!({"v":1,"id":1,"at":"2026-09-13T10:00:00-03:00","type":"state","phase":"closed"}),
            json!({"v":1,"id":2,"type":"remove","targets":[1],"reason":"engano"}),
        ]);
        assert!(!phase_recorded_since(&removed, "closed", 0), "a removed state does not count");
    }

    /// Só os pedidos gravados depois do evento dado voltam.
    #[test]
    fn only_the_requests_after_the_given_event_come_back() {
        let request = |id: u64, text: &str| {
            json!({"v":1,"id":id,"type":"request","text":text,"keys":[],"effect":"adjust_waves","origin":1})
        };
        let log = log(&[request(2, "antes"), criterion(3), run(4, 3, "pass"), request(5, "depois")]);
        let after: Vec<&str> = requests_after(&log, 4).iter().filter_map(|e| e.str_field("text")).collect();
        assert_eq!(after, ["depois"]);
        assert_eq!(requests_after(&log, 0).len(), 2);
    }

    fn wave(id: u64, n: u64) -> Value {
        json!({"v":1,"id":id,"type":"wave","n":n,"text":"t","criteria":[1],"done_when":"d","origin":1})
    }

    fn approve(id: u64) -> Value {
        json!({"v":1,"id":id,"type":"state","phase":"approved",
               "witness":{"question":"Aprovar esta spec?","answer":"Aprovar"}})
    }

    /// Uma spec que nasceu aprovada e ganhou a branch depois, pela revisão do
    /// nascimento: a fronteira fica na primeira versão da aprovação, e a onda
    /// gravada entre ela e a revisão conta como crescimento.
    #[test]
    fn a_revised_approval_keeps_the_boundary_of_its_first_version() {
        let mut revision = approve(3);
        revision["replaces"] = json!(1);
        revision["branch"] = json!("feature/x");
        let lines = [approve(1), wave(2, 1), revision, wave(4, 2)];
        assert_eq!(approval_event(&log(&lines)).map(|e| e.id), Some(3), "the revision is the approval shown");
        assert_eq!(approval_boundary(&log(&lines)), Some(1));
        assert_eq!(waves_grown_by(&log(&lines), 4), Some((0, 2)), "the wave between the two is not approved");
    }

    /// Uma spec com o nascimento em plano, as ondas `1..=approved` e a
    /// aprovação logo depois delas.
    fn approved_with(approved: u64) -> Vec<Value> {
        let mut lines = vec![json!({"v":1,"id":1,"type":"state","phase":"plan"})];
        lines.extend((1..=approved).map(|n| wave(n + 1, n)));
        lines.push(approve(approved + 2));
        lines
    }

    /// A aprovação que vale é a mais nova enquanto a spec continua aprovada; a
    /// spec que voltou ao plano não tem aprovação.
    #[test]
    fn the_approval_event_is_the_newest_approval_while_the_spec_stays_approved() {
        let mut lines = approved_with(1);
        lines.push(json!({"v":1,"id":4,"type":"state","phase":"running"}));
        assert_eq!(approval_event(&log(&lines)).map(|e| e.id), Some(3), "running keeps the approval");
        lines.push(json!({"v":1,"id":5,"type":"state","phase":"plan"}));
        assert_eq!(approval_event(&log(&lines)), None, "back in plan, no approval stands");
        lines.push(approve(6));
        assert_eq!(approval_event(&log(&lines)).map(|e| e.id), Some(6));
        assert_eq!(approval_event(&log(&[wave(1, 1)])), None, "a spec never approved");
    }

    /// Quatro ondas aprovadas e duas novas: cada onda nova avisa, com a conta
    /// da aprovação e a de agora.
    #[test]
    fn new_waves_after_the_approval_count_as_growth() {
        let mut lines = approved_with(4);
        lines.push(wave(7, 5));
        assert_eq!(waves_grown_by(&log(&lines), 7), Some((4, 5)));
        lines.push(wave(8, 6));
        assert_eq!(waves_grown_by(&log(&lines), 8), Some((4, 6)));
        assert_eq!((waves_at(&log(&lines), 6), waves_now(&log(&lines))), (4, 6));
    }

    #[test]
    fn a_spec_never_approved_never_warns_growth() {
        let lines: Vec<Value> = (1..=6).map(|n| wave(n, n)).collect();
        assert_eq!(waves_grown_by(&log(&lines), 6), None);
    }

    /// Reaprovada, a conta parte da última aprovação: as ondas que entraram
    /// entre as duas já foram aprovadas.
    #[test]
    fn growth_counts_from_the_latest_approval() {
        let mut lines = approved_with(4);
        lines.push(wave(7, 5));
        lines.push(json!({"v":1,"id":8,"type":"state","phase":"plan"}));
        lines.push(approve(9));
        lines.push(wave(10, 6));
        assert_eq!(waves_grown_by(&log(&lines), 10), Some((5, 6)));
    }

    /// A versão nova de uma onda depois da aprovação não soma onda nenhuma.
    #[test]
    fn a_revised_wave_after_approval_is_not_growth() {
        let mut lines = approved_with(4);
        let mut revised = wave(7, 3);
        revised["replaces"] = json!(4);
        lines.push(revised);
        assert_eq!(waves_now(&log(&lines)), 4);
        assert_eq!(waves_grown_by(&log(&lines), 7), None);
    }

    /// Uma onda tirada e outra somada deixam a conta na aprovada: sem aviso.
    #[test]
    fn removing_one_wave_and_adding_one_does_not_warn() {
        let mut lines = approved_with(4);
        lines.push(json!({"v":1,"id":7,"type":"remove","targets":[5],"reason":"sai"}));
        lines.push(wave(8, 5));
        assert_eq!(waves_at(&log(&lines), 6), 4, "the removal came after the approval");
        assert_eq!(waves_now(&log(&lines)), 4);
        assert_eq!(waves_grown_by(&log(&lines), 8), None);
    }
}
