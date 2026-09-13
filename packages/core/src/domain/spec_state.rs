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
//! Uma spec sem `spec.ndjson` não tem estado nenhum aqui. Fora do núcleo, uma
//! spec do `spec-draft` parada antes da execução conta como em plano, e
//! trava; só uma branch que o Mustard não abriu fica livre. Uma spec com o
//! arquivo e sem nenhum `state` visível tem estado, sem fase, e não está
//! aprovada.
//!
//! Quem decide (o portão de escrita, a testemunha da aprovação, a cobrança das
//! pendências) recebe os valores já lidos e se testa sem disco, com uma
//! [`SpecState`] falsa.

use serde_json::Value;

use crate::domain::spec_events::{Block, BlockQuery, SpecEvent, SpecLog, PHASES};

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
fn original_of(log: &SpecLog, event: &SpecEvent) -> u64 {
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
    /// O estado da spec `spec`; `None` só quando ela não tem arquivo de
    /// eventos. Quem trava decide o que isso quer dizer: uma spec do
    /// `spec-draft` parada antes da execução conta como em plano.
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
}
