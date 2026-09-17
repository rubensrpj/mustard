//! `spec_state` — the ONE ladder that names the current spec, read from disk.
//!
//! The ladder itself is pure and lives in the core
//! (`mustard_core::domain::spec_state::resolve`): the `MUSTARD_ACTIVE_SPEC`
//! override, then the spec of the branch the checkout stands on, then the spec
//! the session is bound to. This module only reads each rung — the environment,
//! `.git/HEAD` and the session's `active-spec` marker — and hands them over.
//!
//! Every door that asks "which spec is this" goes through [`active_spec`], so
//! two doors can never name different specs for the same session. A leftover
//! `.pipeline-states/` file names nothing.
//!
//! [`DiskSpecState`] is the disk side of the core's `SpecState` port: that
//! ladder, and each spec's state folded from its `spec.ndjson`.

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::{approval_event, resolve, SpecState, State};
use mustard_core::io::spec_events as store;
use serde_json::Value;

use crate::shared::context::checkout::spec_of_checkout_branch;
use crate::shared::context::session::spec_for_session;

/// As variáveis de ambiente que dizem a sessão, na ordem em que valem: a do
/// Mustard, a que o Claude Code põe no ambiente dos comandos, e a antiga. O
/// único lugar que as lê.
const SESSION_VARS: &[&str] = &["MUSTARD_SESSION_ID", "CLAUDE_CODE_SESSION_ID", "CLAUDE_SESSION_ID"];

/// The session id the `run` face was handed through the environment, by the
/// order of [`SESSION_VARS`]. Never a guess from the newest session folder,
/// which could name another session's spec. Só as entradas `run` perguntam
/// aqui e passam a sessão para baixo; o que elas chamam recebe a sessão como
/// argumento, e os testes passam a deles.
#[must_use]
pub(crate) fn session_from_env() -> Option<String> {
    session_from(|key| std::env::var(key).ok())
}

/// A sessão que `var` diz, pela ordem de [`SESSION_VARS`]; uma variável em
/// branco não conta.
fn session_from(var: impl Fn(&str) -> Option<String>) -> Option<String> {
    SESSION_VARS
        .iter()
        .find_map(|key| var(key).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
}

/// The current spec for `session` in the project at `root`, fail-open `None`:
/// the environment override, then the spec of the checkout's branch, then the
/// spec the session is bound to.
#[must_use]
pub(crate) fn active_spec(root: &str, session: Option<&str>) -> Option<String> {
    let env = std::env::var("MUSTARD_ACTIVE_SPEC").ok();
    let branch = spec_of_checkout_branch(root);
    let bound = session.and_then(|sid| spec_for_session(root, sid));
    resolve(env.as_deref(), branch, bound)
}

/// The disk side of the core's [`SpecState`] port: the ladder of
/// [`active_spec`] over the checkout at `root`, and each spec's state folded
/// from its `spec.ndjson` (the main checkout's, when `root` is a linked
/// worktree).
pub(crate) struct DiskSpecState {
    root: PathBuf,
}

impl DiskSpecState {
    /// The port over the checkout at `root`.
    #[must_use]
    pub(crate) fn new(root: &Path) -> Self {
        Self { root: root.to_path_buf() }
    }
}

impl SpecState for DiskSpecState {
    fn active(&self, session: Option<&str>) -> Option<String> {
        active_spec(&self.root.to_string_lossy(), session)
    }

    fn state(&self, spec: &str) -> Option<State> {
        self.log(spec).map(|log| State::from_log(&log))
    }

    fn log(&self, spec: &str) -> Option<SpecLog> {
        let path = store::spec_file(&store::spec_root(&self.root), spec).ok()?;
        store::read(&path).ok().flatten()
    }
}

/// A spec `spec` do projeto em `root` foi aprovada pelo usuário: o estado
/// que a trava lê ([`lock_state`]) está numa fase de spec aprovada. A única
/// resposta a "está aprovada?": o `approve-spec`, a retomada, a página da
/// spec e o `wave-scaffold` perguntam aqui, e o `status` pergunta pela
/// [`approval`], que passa por aqui antes de ler a testemunha.
#[must_use]
pub(crate) fn approved(root: &Path, spec: &str) -> bool {
    lock_state(root, spec).is_some_and(|state| state.approved)
}

/// O estado que a trava da aprovação lê, pela regra única do núcleo
/// ([`mustard_core::domain::spec_state::lock_state_of`]), com o arquivo de
/// eventos da spec, o do checkout principal num worktree. O portão, a
/// testemunha e [`approved`] passam por aqui.
///
/// Só o arquivo de eventos conta: uma pasta de spec antiga, só com o
/// `meta.json`, fica livre, e um `meta.json` ao lado de um arquivo de eventos
/// nunca muda o que o estado diz.
#[must_use]
pub(crate) fn lock_state(root: &Path, spec: &str) -> Option<State> {
    mustard_core::domain::spec_state::lock_state_of(DiskSpecState::new(root).log(spec).as_ref())
}

/// A aprovação que vale de uma spec: a pergunta, a opção que o usuário
/// escolheu e a hora em que a testemunha gravou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Approval {
    pub(crate) question: String,
    pub(crate) answer: String,
    pub(crate) at: String,
}

/// A aprovação da spec `spec`: a que vale pelo núcleo
/// ([`mustard_core::domain::spec_state::approval_event`]), a mesma que a
/// página e o aviso de crescimento das ondas leem, enquanto a trava a lê
/// aprovada ([`approved`]). `None` numa spec que não está aprovada ou que não
/// tem arquivo de eventos.
#[must_use]
pub(crate) fn approval(root: &Path, spec: &str) -> Option<Approval> {
    if !approved(root, spec) {
        return None;
    }
    let log = DiskSpecState::new(root).log(spec)?;
    let event = approval_event(&log)?;
    let witness = event.fields.get("witness").filter(|witness| witness.is_object())?;
    let text = |key: &str| witness.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    Some(Approval { question: text("question"), answer: text("answer"), at: event.at().to_string() })
}

/// Grava na pasta `spec_dir` a spec em plano e, em seguida, aprovada pelo
/// usuário, como a testemunha grava.
#[cfg(test)]
pub(crate) fn approve_in(spec_dir: &Path) {
    std::fs::create_dir_all(spec_dir).unwrap();
    let path = spec_dir.join("spec.ndjson");
    for fields in [
        serde_json::json!({ "phase": "plan" }),
        serde_json::json!({
            "phase": "approved",
            "author": "user",
            "witness": { "question": "Aprovar esta spec?", "answer": "Aprovar" }
        }),
    ] {
        store::write(&path, "state", fields.as_object().cloned().unwrap(), &[]).unwrap();
    }
}

/// O `spec.ndjson` da spec `spec` do projeto em `root`, com a pasta criada.
#[cfg(test)]
fn seed_file(root: &Path, spec: &str) -> PathBuf {
    let path = store::spec_file(&store::spec_root(root), spec).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    path
}

/// Grava um evento `event_type` com `fields` na spec `spec` do projeto em
/// `root` e devolve o número dele. O critério e o pedido apontam para uma
/// mensagem gravada antes, de onde vieram.
#[cfg(test)]
pub(crate) fn seed_event(root: &Path, spec: &str, event_type: &str, fields: Value) -> u64 {
    let path = seed_file(root, spec);
    let mut draft = fields.as_object().cloned().unwrap();
    if matches!(event_type, "criterion" | "request") {
        let message = serde_json::json!({ "author": "user", "text": "combinado" });
        let origin = store::write(&path, "message", message.as_object().cloned().unwrap(), &[]).unwrap();
        draft.insert("origin".to_string(), serde_json::json!(origin.id));
    }
    store::write(&path, event_type, draft, &[]).unwrap().id
}

/// Grava na spec `spec` um critério por item de `results` e, para cada
/// `Some`, uma execução com esse resultado; `None` deixa o critério sem
/// execução. Devolve os números dos critérios, na ordem.
#[cfg(test)]
pub(crate) fn seed_runs(root: &Path, spec: &str, results: &[Option<&str>]) -> Vec<u64> {
    let mut criteria = Vec::new();
    for result in results {
        let criterion = seed_event(
            root,
            spec,
            "criterion",
            serde_json::json!({ "when": "a obra roda", "then": "o critério confere", "proof": "cargo test" }),
        );
        if let Some(result) = result {
            seed_run(root, spec, criterion, result);
        }
        criteria.push(criterion);
    }
    criteria
}

/// Grava uma execução do critério `criterion` com o resultado `result`.
#[cfg(test)]
pub(crate) fn seed_run(root: &Path, spec: &str, criterion: u64, result: &str) -> u64 {
    let exit = u64::from(result != "pass");
    let run = serde_json::json!({ "criterion": criterion, "result": result, "exit": exit, "ms": 5 });
    seed_event(root, spec, "criterion_run", run)
}

/// Grava o veredito `result` da onda `wave`, conferindo o critério
/// `criterion`.
#[cfg(test)]
pub(crate) fn seed_verdict(root: &Path, spec: &str, wave: u64, result: &str, criterion: u64) -> u64 {
    let verdict = serde_json::json!({
        "wave": wave,
        "result": result,
        "text": "revisão da onda",
        "criteria": [{ "criterion": criterion, "tests_rule": "confere a regra" }],
    });
    seed_event(root, spec, "verdict", verdict)
}

/// Grava um pedido de mudança com o texto `text`.
#[cfg(test)]
pub(crate) fn seed_request(root: &Path, spec: &str, text: &str) -> u64 {
    let request = serde_json::json!({ "text": text, "keys": ["pedido"], "effect": "adjust_waves" });
    seed_event(root, spec, "request", request)
}

/// Stand the checkout at `root` on the branch of `spec` — a `.git/HEAD` naming
/// `feature/<spec>` and the spec's folder — so the branch rung answers `spec`
/// without touching the process environment.
#[cfg(test)]
pub(crate) fn stand_on_spec_branch(root: &std::path::Path, spec: &str) {
    let git = root.join(".git");
    std::fs::create_dir_all(&git).unwrap();
    std::fs::write(git.join("HEAD"), format!("ref: refs/heads/feature/{spec}\n")).unwrap();
    std::fs::create_dir_all(root.join(".claude").join("spec").join(spec)).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::context::session::bind_session_spec;
    use mustard_core::domain::model::contract::HookInput;
    use mustard_core::platform::git;
    use tempfile::tempdir;

    const SESSION: &str = "s-lado-a-lado";


    /// A spec folder with a `spec.md`, which the checklist door also needs.
    fn spec_with_md(root: &std::path::Path, spec: &str) {
        let dir = root.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("spec.md"), "# Spec\n").unwrap();
    }



    /// A sessão do ambiente vem da primeira variável dita, na ordem: a do
    /// Mustard, a que o Claude Code põe no ambiente dos comandos, e a antiga.
    /// Uma variável em branco não conta.
    #[test]
    fn the_session_comes_from_the_first_variable_set() {
        fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
            move |key| pairs.iter().find(|(k, _)| *k == key).map(|(_, v)| (*v).to_string())
        }
        assert_eq!(session_from(env(&[("MUSTARD_SESSION_ID", "m")])).as_deref(), Some("m"));
        assert_eq!(session_from(env(&[("CLAUDE_CODE_SESSION_ID", "cc")])).as_deref(), Some("cc"));
        assert_eq!(session_from(env(&[("CLAUDE_SESSION_ID", "old")])).as_deref(), Some("old"));
        let all = env(&[("MUSTARD_SESSION_ID", "m"), ("CLAUDE_CODE_SESSION_ID", "cc"), ("CLAUDE_SESSION_ID", "old")]);
        assert_eq!(session_from(all).as_deref(), Some("m"));
        let claude = env(&[("CLAUDE_CODE_SESSION_ID", "cc"), ("CLAUDE_SESSION_ID", "old")]);
        assert_eq!(session_from(claude).as_deref(), Some("cc"));
        let blank = env(&[("MUSTARD_SESSION_ID", "  "), ("CLAUDE_CODE_SESSION_ID", "cc")]);
        assert_eq!(session_from(blank).as_deref(), Some("cc"));
        assert_eq!(session_from(env(&[])), None);
    }

    /// As variáveis de sessão são lidas num lugar só: nenhum outro arquivo de
    /// produção as nomeia fora de comentário.
    #[test]
    fn every_session_variable_is_read_in_one_place() {
        fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    sources(&path, out);
                } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    out.push(path);
                }
            }
        }
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut files = Vec::new();
        for dir in ["apps/rt/src", "apps/cli/src", "packages/core/src", "apps/dashboard/server/src"] {
            sources(&repo.join(dir), &mut files);
        }
        let home = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("shared").join("spec_state.rs");
        let mut hits = Vec::new();
        for path in files.iter().filter(|p| std::fs::canonicalize(p).ok() != std::fs::canonicalize(&home).ok()) {
            let body = std::fs::read_to_string(path).unwrap_or_default();
            let production = body.split("#[cfg(test)]").next().unwrap_or_default();
            for line in production.lines().filter(|l| !l.trim_start().starts_with("//")) {
                if SESSION_VARS.iter().any(|var| line.contains(var)) {
                    hits.push(format!("{}: {}", path.display(), line.trim()));
                }
            }
        }
        assert!(hits.is_empty(), "a session variable is read outside its one reader:\n{}", hits.join("\n"));
    }

    #[test]
    fn the_session_rung_answers_only_for_the_session_it_is_given() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        bind_session_spec(root, SESSION, "da-sessao");
        assert_eq!(active_spec(root, Some(SESSION)).as_deref(), Some("da-sessao"));
        assert_eq!(active_spec(root, Some("outra-sessao")), None);
        assert_eq!(active_spec(root, None), None);
    }


    fn git(dir: &Path, args: &[&str]) {
        assert!(git::run(dir, args).ok, "git {args:?} failed");
    }

    /// Um repositório com um commit na `dev` e, fora do git como o Mustard
    /// fica, o `mustard.json` e a pasta da spec `spec`; e um worktree ligado,
    /// parado na branch `feature/<spec>`, sem nada do Mustard.
    fn main_and_worktree(tmp: &Path, spec: &str) -> (PathBuf, PathBuf) {
        let main = tmp.join("repo");
        std::fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q"]);
        git(&main, &["config", "user.email", "t@example.com"]);
        git(&main, &["config", "user.name", "t"]);
        git(&main, &["checkout", "-q", "-b", "dev"]);
        std::fs::write(main.join("README.md"), "oi\n").unwrap();
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-q", "-m", "init"]);
        std::fs::write(main.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#).unwrap();
        std::fs::create_dir_all(main.join(".claude").join("spec").join(spec)).unwrap();
        let wt = tmp.join("wt");
        let branch = format!("feature/{spec}");
        git(&main, &["worktree", "add", "-q", &wt.to_string_lossy(), "-b", &branch]);
        (main, wt)
    }

    /// Num worktree ligado, o `.git` é um arquivo que aponta a pasta do git
    /// dele: o degrau da branch lê o `HEAD` por ali, e a pasta da spec no
    /// checkout principal, sem rodar o git.
    #[test]
    fn a_linked_worktree_names_its_spec_by_the_branch() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let tmp = tempdir().unwrap();
        let (_main, wt) = main_and_worktree(tmp.path(), "x");
        assert!(wt.join(".git").is_file(), "the fixture is a real linked worktree");
        assert_eq!(active_spec(&wt.to_string_lossy(), None).as_deref(), Some("x"));
    }



    #[test]
    fn a_spec_without_its_event_file_has_no_state() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude").join("spec").join("sem-arquivo")).unwrap();
        let disk = DiskSpecState::new(dir.path());
        assert_eq!(disk.state("sem-arquivo"), None, "no event file, no state");
        assert!(disk.log("sem-arquivo").is_none());
    }

    /// Tirar o único `state` do arquivo não faz da spec uma branch que o
    /// Mustard não abriu: o estado continua lá, sem fase e sem aprovação.
    #[test]
    fn a_spec_file_whose_only_state_was_removed_still_has_an_unapproved_state() {
        let dir = tempdir().unwrap();
        let path = store::spec_file(dir.path(), "sem-state").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let draft = |v: serde_json::Value| v.as_object().cloned().unwrap();
        let first = store::write(&path, "state", draft(serde_json::json!({"phase": "plan"})), &[]).unwrap();
        store::write(
            &path,
            "remove",
            draft(serde_json::json!({"targets": [first.id], "reason": "engano"})),
            &[],
        )
        .unwrap();

        let state = DiskSpecState::new(dir.path()).state("sem-state").expect("the file is there");
        assert_eq!(state.phase, None);
        assert!(!state.approved);
    }

    #[test]
    fn the_disk_state_folds_the_state_events_of_the_spec_file() {
        let dir = tempdir().unwrap();
        let path = store::spec_file(dir.path(), "com-arquivo").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let draft = |v: serde_json::Value| v.as_object().cloned().unwrap();
        store::write(
            &path,
            "state",
            draft(serde_json::json!({"phase": "plan", "branch": "feature/com-arquivo", "base": "dev"})),
            &[],
        )
        .unwrap();
        store::write(
            &path,
            "state",
            draft(serde_json::json!({
                "phase": "approved",
                "witness": {"question": "Aprova?", "answer": "Aprovar"}
            })),
            &[],
        )
        .unwrap();

        let state = DiskSpecState::new(dir.path()).state("com-arquivo").expect("the file is there");
        assert_eq!(state.phase, Some("approved"));
        assert!(state.approved);
        assert_eq!(state.branch.as_deref(), Some("feature/com-arquivo"), "the branch is inherited");
        assert_eq!(state.base.as_deref(), Some("dev"));
    }

    /// O leitor da aprovação do rt e o do núcleo veem a mesma aprovação, lado
    /// a lado: nenhuma em plano, a primeira, nenhuma de volta ao plano e a
    /// última depois de reaprovada.
    #[test]
    fn the_approval_reader_and_the_core_see_the_same_approval() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let path = store::spec_file(root, "epic").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let state = |fields: serde_json::Value, at: &str| {
            store::write_at(&path, "state", fields.as_object().cloned().unwrap(), &[], at).unwrap();
        };
        let approve = |answer: &str, at: &str| {
            state(
                serde_json::json!({
                    "phase": "approved",
                    "author": "user",
                    "witness": { "question": "Aprovar esta spec?", "answer": answer }
                }),
                at,
            );
        };
        let both = || {
            let log = DiskSpecState::new(root).log("epic").unwrap();
            let core = approval_event(&log).map(|event| event.at().to_string());
            (approval(root, "epic").map(|a| (a.answer, a.at)), core)
        };

        state(serde_json::json!({ "phase": "plan" }), "2026-09-12T09:00:00-03:00");
        assert_eq!(both(), (None, None), "in plan");
        approve("Aprovar", "2026-09-12T09:03:00-03:00");
        let first = "2026-09-12T09:03:00-03:00".to_string();
        assert_eq!(both(), (Some(("Aprovar".into(), first.clone())), Some(first)));
        state(serde_json::json!({ "phase": "plan" }), "2026-09-12T10:00:00-03:00");
        assert_eq!(both(), (None, None), "back in plan");
        approve("Aprovar de novo", "2026-09-12T11:00:00-03:00");
        let last = "2026-09-12T11:00:00-03:00".to_string();
        assert_eq!(both(), (Some(("Aprovar de novo".into(), last.clone())), Some(last)));
    }

    /// A página e o leitor da aprovação veem a mesma aprovação, lado a lado:
    /// depois de reaprovada, a página marca só o que veio depois da aprovação
    /// que o leitor devolve.
    #[test]
    fn the_page_and_the_approval_reader_see_the_same_approval() {
        use mustard_core::platform::i18n::Locale;
        use mustard_core::view::document::{spec_document, Node, WavePrompts};
        use serde_json::json;
        let dir = tempdir().unwrap();
        let root = dir.path();
        let path = store::spec_file(root, "epic").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let write = |event_type: &str, fields: Value, at: &str| {
            store::write_at(&path, event_type, fields.as_object().cloned().unwrap(), &[], at).unwrap().id
        };
        let at = |time: &str| format!("2026-09-12T{time}:00-03:00");
        let msg = write("message", json!({ "author": "user", "text": "combine" }), &at("08:00"));
        let rule = |text: &str, time: &str| {
            write("rule", json!({ "text": text, "keys": ["k"], "example": "e", "origin": msg }), &at(time));
        };
        let approve = |answer: &str, time: &str| {
            let witness = json!({ "question": "Aprovar esta spec?", "answer": answer });
            write("state", json!({ "phase": "approved", "author": "user", "witness": witness }), &at(time));
        };
        write("state", json!({ "phase": "plan" }), &at("08:01"));
        rule("Antes de tudo.", "08:02");
        approve("Aprovar", "09:00");
        rule("Entre as aprovações.", "09:30");
        write("state", json!({ "phase": "plan" }), &at("10:00"));
        approve("Aprovar de novo", "11:00");
        rule("Depois da última.", "11:30");

        let reader = approval(root, "epic").expect("the spec is approved");
        let log = DiskSpecState::new(root).log("epic").unwrap();
        let boundary = log
            .visible()
            .into_iter()
            .find(|event| event.event_type == "state" && event.at() == reader.at)
            .map(|event| event.id)
            .unwrap();
        let doc = spec_document("epic", &log, &WavePrompts::new(), Locale::PtBr);
        let agreed = doc
            .body
            .iter()
            .find_map(|node| match node {
                Node::Section(section) if section.anchor.as_deref() == Some("agreed") => Some(section),
                _ => None,
            })
            .unwrap();
        let marked: Vec<(String, bool)> = agreed
            .body
            .iter()
            .filter_map(|node| match node {
                Node::Item(item) => Some((item.text.clone(), item.note.is_some())),
                _ => None,
            })
            .collect();
        assert_eq!(
            marked,
            [
                ("Antes de tudo.".to_string(), false),
                ("Entre as aprovações.".to_string(), false),
                ("Depois da última.".to_string(), true),
            ]
        );
        for event in log.visible().into_iter().filter(|event| event.event_type == "rule") {
            let text = event.str_field("text").unwrap_or_default();
            let on_page = marked.iter().find(|(shown, _)| shown == text).map(|(_, mark)| *mark);
            assert_eq!(on_page, Some(event.id > boundary), "{text}: the page and the reader disagree");
        }
    }
}
