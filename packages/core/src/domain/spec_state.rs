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
//!   anterior, e o ausente herda.
//!
//! Quem decide (o portão de escrita, a testemunha da aprovação, a cobrança das
//! pendências) recebe os valores já lidos e se testa sem disco, com uma
//! [`SpecState`] falsa.

use serde_json::Value;

use crate::domain::spec_events::{Block, BlockQuery, SpecLog, PHASES};

/// As fases em que a spec já foi aprovada pelo usuário.
const APPROVED_PHASES: &[&str] = &["approved", "running", "closed", "pr_open", "delivered"];

/// As fases que disparam a cobrança das pendências no fim da resposta: a spec
/// fechou, ou entrou no merge.
const CLOSING_PHASES: &[&str] = &["closed", "delivered"];

/// O estado de uma spec, dobrado dos eventos `state` que a leitura mostra.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// A fase atual, uma de [`PHASES`]; `None` quando a spec não tem nenhum
    /// evento `state`, que é o caso de uma spec que o Mustard não abriu.
    pub phase: Option<&'static str>,
    /// A fase atual é a de uma spec aprovada: `approved`, `running`,
    /// `closed`, `pr_open` ou `delivered`.
    pub approved: bool,
    /// A branch em que a spec mora, herdada do último evento que a disse.
    pub branch: Option<String>,
    /// A base de que a branch foi cortada, herdada do mesmo jeito.
    pub base: Option<String>,
    /// O número do primeiro evento `state` com a fase `closed` ou
    /// `delivered`: o gatilho da cobrança das pendências.
    pub closed_by: Option<u64>,
    /// A testemunha da última aprovação gravada, `{question, answer}`.
    pub witness: Option<Value>,
}

impl State {
    /// O estado de uma spec sem arquivo de eventos, ou sem evento `state`.
    #[must_use]
    pub fn absent() -> Self {
        Self::default()
    }

    /// `true` quando nenhum evento `state` foi lido: a spec não foi aberta
    /// pelo Mustard, e nenhuma trava vale para ela.
    #[must_use]
    pub fn is_absent(&self) -> bool {
        self.phase.is_none()
    }

    /// Dobra os eventos `state` visíveis, em ordem de número. Uma fase que
    /// este binário não conhece é ignorada, e a anterior fica.
    #[must_use]
    pub fn from_log(log: &SpecLog) -> Self {
        let mut state = Self::absent();
        for event in log.block(BlockQuery::Block(Block::State)) {
            if event.event_type != "state" {
                continue;
            }
            if let Some(phase) = event.str_field("phase").and_then(known_phase) {
                state.phase = Some(phase);
                if state.closed_by.is_none() && CLOSING_PHASES.contains(&phase) {
                    state.closed_by = Some(event.id);
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
    /// O estado da spec `spec`; [`State::absent`] quando ela não tem arquivo
    /// de eventos.
    fn state(&self, spec: &str) -> State;
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
        assert_eq!(state.closed_by, Some(5), "the first closing event is the trigger");
        assert_eq!(state.witness, Some(json!({"question":"Aprova?","answer":"Aprovar"})));
        assert!(!state.is_absent());
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
        assert_eq!(state.closed_by, None);
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

    #[test]
    fn a_log_without_state_events_has_no_state() {
        let log = log(&[json!({"v":1,"id":1,"type":"message","author":"user","text":"oi"})]);
        assert_eq!(State::from_log(&log), State::absent());
        assert!(State::absent().is_absent());
    }
}
