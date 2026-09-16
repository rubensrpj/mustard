//! `mustard-rt run reopen --reason <motivo> [--spec <nome>]` — leva a spec de
//! volta ao levantamento.
//!
//! O caminho de volta. Uma spec já em plano, aprovada ou em execução volta à
//! fase de levantamento por este comando, e daí o `grill` roda de novo: os
//! pontos novos convivem com o que já foi decidido. Nada do que está gravado é
//! apagado — a volta é mais um evento no arquivo, um `state` com a fase
//! `survey`, e ele guarda quem pediu, quando e por quê.
//!
//! O motivo é obrigatório: é ele que explica, daqui a um mês, por que o
//! levantamento recomeçou — e é ele que vira a consulta. O levantamento
//! seguinte traz os itens que o motivo toca, para o usuário dizer se cada um
//! fica, muda ou sai; o que o motivo não toca fica como está, e nada é
//! perguntado de novo. Um `--reason` em branco é recusado.
//!
//! A spec fechada, com o pull request aberto, entregue ou descartada não
//! volta: o que ela decidiu já saiu, e o caminho é uma spec nova pelo `open`.
//! A spec que já está em levantamento não grava nada e responde o mesmo passo.
//!
//! ```text
//! {"ok": true, "spec": "x", "phase": "survey", "from": "running", "id": 42,
//!  "reason": "O pedido mudou de alvo.", "next": "A spec x voltou ao levantamento…"}
//! ```
//!
//! Recusa sai com exit 1 e `ok: false`, com a razão curta em `reason` e a
//! mensagem no idioma do projeto em `hint`.

use std::path::PathBuf;

use mustard_core::domain::spec_events::Refusal;
use mustard_core::domain::spec_state::{reopenable, PhaseWriter, SpecState, State};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use crate::commands::spec_events::{self, read::checkout, write::record};
use crate::shared::spec_state::{session_from_env, DiskSpecState};

/// Options for `mustard-rt run reopen`.
pub struct ReopenOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec que volta ao levantamento; sem ela, a spec atual.
    pub spec: Option<String>,
    /// Por que a spec volta, palavra por palavra de quem pediu.
    pub reason: String,
}

/// Por que a volta não aconteceu.
enum ReopenRefusal {
    /// O `--reason` veio em branco.
    ReasonMissing,
    /// A spec já fechou, foi para o pull request, foi entregue ou descartada.
    Settled { spec: String, phase: String },
    /// Uma recusa da leitura ou da gravação do arquivo de eventos.
    Spec(Refusal),
}

impl ReopenRefusal {
    /// A razão curta, estável, para quem lê a saída por máquina.
    fn reason(&self) -> &'static str {
        match self {
            Self::ReasonMissing => "reason-missing",
            Self::Settled { .. } => "spec-settled",
            Self::Spec(refusal) => refusal.reason(),
        }
    }

    /// A mensagem exata, no idioma pedido.
    fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, &str)]| {
            slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        match self {
            Self::ReasonMissing => fill("reopen.reason_missing", &[]),
            Self::Settled { spec, phase } => fill("reopen.settled", &[("{spec}", spec), ("{phase}", phase)]),
            Self::Spec(refusal) => refusal.message(lang),
        }
    }

    fn report(&self, lang: Locale) -> Value {
        json!({ "ok": false, "reason": self.reason(), "hint": self.message(lang) })
    }
}

/// O núcleo testável de [`run`]. A sessão vem do ambiente. Nunca entra em
/// pânico.
pub(crate) fn reopen_at(opts: &ReopenOpts) -> Value {
    reopen_for(opts, session_from_env().as_deref())
}

/// [`reopen_at`] com a sessão recebida, que é como um teste a escolhe.
pub(crate) fn reopen_for(opts: &ReopenOpts, session: Option<&str>) -> Value {
    let project = spec_events::project(&opts.root);
    let lang = project.lang;
    let refuse = |refusal: ReopenRefusal| refusal.report(lang);

    let reason = opts.reason.trim();
    if reason.is_empty() {
        return refuse(ReopenRefusal::ReasonMissing);
    }
    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => match DiskSpecState::new(&checkout(&opts.root)).active(session) {
            Some(spec) => spec,
            None => return refuse(ReopenRefusal::Spec(Refusal::NoCurrentSpec)),
        },
    };
    let path = match store::spec_file(&project.root, &spec) {
        Ok(path) => path,
        Err(refusal) => return refuse(ReopenRefusal::Spec(refusal)),
    };
    let log = match store::read(&path) {
        Ok(Some(log)) => log,
        Ok(None) => return refuse(ReopenRefusal::Spec(Refusal::NoSpecFile { spec })),
        Err(refusal) => return refuse(ReopenRefusal::Spec(refusal)),
    };
    let from = State::from_log(&log).phase.unwrap_or("-");
    if from == "survey" {
        return json!({
            "ok": true, "spec": spec, "phase": "survey", "from": from, "recorded": false,
            "next": say("reopen.already", lang, &spec),
        });
    }
    if !reopenable(from) {
        return refuse(ReopenRefusal::Settled { spec, phase: from.to_string() });
    }

    let mut draft = Map::new();
    draft.insert("phase".to_string(), json!("survey"));
    draft.insert("author".to_string(), json!("binary"));
    draft.insert("reason".to_string(), json!(reason));
    match record(&opts.root, &spec, "state", draft, PhaseWriter::Binary) {
        Ok(recorded) => json!({
            "ok": true, "spec": spec, "phase": "survey", "from": from, "recorded": true,
            "id": recorded.written.id, "reason": reason,
            "next": say("reopen.next", lang, &spec),
        }),
        Err(refusal) => refuse(ReopenRefusal::Spec(refusal)),
    }
}

/// Um texto do catálogo com o nome da spec preenchido.
fn say(key: &str, lang: Locale, spec: &str) -> String {
    translate(key, lang).replace("{spec}", spec)
}

/// Roda o `reopen` e imprime o relatório em JSON; sai com 1 na recusa.
pub fn run(opts: &ReopenOpts) {
    let report = reopen_at(opts);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    if report["ok"] != json!(true) {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::{write_at, WriteOpts};
    use std::path::Path;
    use tempfile::tempdir;

    /// Uma spec aberta e levada até a fase `phase`, um `state` por fase, pela
    /// gravação do arquivo: o caminho inteiro do fluxo passaria por portas que
    /// não são o assunto deste comando.
    fn spec_in(root: &Path, spec: &str, phase: &str) {
        const STEPS: &[&str] = &["survey", "plan", "approved", "running", "closed", "pr_open", "delivered"];
        let path = store::spec_file(root, spec).expect("spec file");
        std::fs::create_dir_all(path.parent().expect("spec folder")).expect("spec folder");
        for step in STEPS {
            let mut fields = json!({"phase": step, "author": "binary"});
            if *step == "survey" {
                fields["branch"] = json!(format!("feature/{spec}"));
                fields["base"] = json!("dev");
            }
            if *step == "approved" {
                fields["witness"] = json!({"question": "Aprovar?", "answer": "Aprovar"});
            }
            if *step == "pr_open" {
                fields["pr"] = json!({"number": 1, "url": "https://exemplo/1"});
            }
            store::write(&path, "state", fields.as_object().cloned().expect("an object"), &[]).expect("state");
            if *step == phase {
                return;
            }
        }
        panic!("{phase} is not a phase of the flow");
    }

    fn reopen(root: &Path, spec: &str, reason: &str) -> Value {
        reopen_for(
            &ReopenOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), reason: reason.to_string() },
            None,
        )
    }

    fn phase_of(root: &Path, spec: &str) -> Option<&'static str> {
        State::from_log(&DiskSpecState::new(root).log(spec).expect("the event file")).phase
    }

    /// Uma spec em execução volta ao levantamento: a fase é a de levantamento
    /// de novo, o motivo fica gravado no evento da volta, e nada do que estava
    /// no arquivo sai.
    #[test]
    fn a_running_spec_goes_back_to_the_survey_with_the_reason_on_the_record() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "running");
        let before = DiskSpecState::new(root).log("epico").expect("the event file").events.len();

        let out = reopen(root, "epico", "  O pedido mudou de alvo.  ");
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["from"], json!("running"), "{out}");
        assert_eq!(out["phase"], json!("survey"), "{out}");
        assert_eq!(out["reason"], json!("O pedido mudou de alvo."), "the reason is trimmed: {out}");
        assert!(out["next"].as_str().unwrap().contains("grill"), "{out}");
        assert_eq!(phase_of(root, "epico"), Some("survey"));

        let log = DiskSpecState::new(root).log("epico").expect("the event file");
        assert_eq!(log.events.len(), before + 1, "one event more, and not one less");
        let back = log.get(out["id"].as_u64().unwrap()).expect("the event of the return");
        assert_eq!(back.str_field("reason").map(str::trim), Some("O pedido mudou de alvo."));
        assert_eq!(back.str_field("phase"), Some("survey"));
    }

    /// Depois da volta, a branch e a base que a spec tinha continuam no
    /// estado: a volta não desfaz o que estava gravado.
    #[test]
    fn the_branch_and_the_base_survive_the_return() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "approved");
        assert_eq!(reopen(root, "epico", "Faltou levantar o limite.")["ok"], json!(true));
        let state = State::from_log(&DiskSpecState::new(root).log("epico").expect("the event file"));
        assert_eq!(state.phase, Some("survey"));
        assert_eq!(state.branch.as_deref(), Some("feature/epico"));
        assert_eq!(state.base.as_deref(), Some("dev"));
        assert!(!state.approved, "the spec is under survey again");
    }

    /// Uma spec fechada, com o pull request aberto ou entregue não volta, e a
    /// recusa diz a fase em que ela está; nada é gravado.
    #[test]
    fn a_settled_spec_is_refused_and_nothing_is_written() {
        for phase in ["closed", "pr_open", "delivered"] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            spec_in(root, "epico", phase);
            let before = std::fs::read(store::spec_file(root, "epico").unwrap()).unwrap();

            let out = reopen(root, "epico", "Quero levantar de novo.");
            assert_eq!(out["reason"], json!("spec-settled"), "{phase}: {out}");
            let hint = out["hint"].as_str().unwrap();
            assert!(hint.contains("epico") && hint.contains(phase), "{hint}");
            assert_eq!(std::fs::read(store::spec_file(root, "epico").unwrap()).unwrap(), before);
            assert_eq!(phase_of(root, "epico"), Some(phase));
        }
    }

    /// O motivo é obrigatório: em branco, a volta é recusada e nada é
    /// gravado.
    #[test]
    fn a_blank_reason_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "running");
        let before = std::fs::read(store::spec_file(root, "epico").unwrap()).unwrap();

        let out = reopen(root, "epico", "   ");
        assert_eq!(out["reason"], json!("reason-missing"), "{out}");
        assert!(out["hint"].as_str().unwrap().contains("--reason"), "{out}");
        assert_eq!(std::fs::read(store::spec_file(root, "epico").unwrap()).unwrap(), before);
        assert_eq!(phase_of(root, "epico"), Some("running"));
    }

    /// Uma spec que já está em levantamento não grava nada e responde o mesmo
    /// passo; uma spec sem arquivo de eventos é recusada pelo nome.
    #[test]
    fn a_spec_already_under_survey_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "survey");
        let before = std::fs::read(store::spec_file(root, "epico").unwrap()).unwrap();

        let out = reopen(root, "epico", "Levantar de novo.");
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["recorded"], json!(false), "{out}");
        assert!(out["next"].as_str().unwrap().contains("grill"), "{out}");
        assert_eq!(std::fs::read(store::spec_file(root, "epico").unwrap()).unwrap(), before);

        let missing = reopen(root, "nunca-aberta", "Levantar de novo.");
        assert_eq!(missing["reason"], json!("no-spec-file"), "{missing}");
    }

    /// Depois da volta, o `grill` roda de novo na mesma spec: é a recusa que
    /// ele dava fora do levantamento que a volta desfaz.
    #[test]
    fn after_the_return_the_survey_runs_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        spec_in(root, "epico", "running");
        let said = write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("epico".into()),
            event_type: "message".into(),
            json: json!({"author": "user", "text": "Travar o merge."}).to_string(),
        });
        let said = said["id"].as_u64().unwrap_or_else(|| panic!("{said}"));

        let refused = crate::commands::flow::grill::grill_for(
            &crate::commands::flow::grill::GrillOpts {
                root: root.to_path_buf(),
                spec: Some("epico".into()),
                kinds: Some("fix".into()),
                condensed: false,
            },
            None,
        );
        assert_eq!(refused["reason"], json!("not-in-survey"), "{refused}");

        assert_eq!(reopen(root, "epico", "Faltou levantar.")["ok"], json!(true));
        let goal = write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("epico".into()),
            event_type: "context".into(),
            json: json!({"text": "Travar o merge.", "origin": said}).to_string(),
        });
        assert_eq!(goal["ok"], json!(true), "{goal}");
        let after = crate::commands::flow::grill::grill_for(
            &crate::commands::flow::grill::GrillOpts {
                root: root.to_path_buf(),
                spec: Some("epico".into()),
                kinds: Some("fix".into()),
                condensed: false,
            },
            None,
        );
        assert_eq!(after["ok"], json!(true), "the survey runs again: {after}");
    }
}
