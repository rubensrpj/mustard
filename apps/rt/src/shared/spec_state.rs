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

use mustard_core::domain::spec_events::{Block, BlockQuery, SpecLog};
use mustard_core::domain::spec_state::{resolve, SpecState, State};
use mustard_core::io::spec_events as store;
use serde_json::Value;

use crate::shared::context;

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
    let branch = context::spec_of_checkout_branch(root);
    let bound = session.and_then(|sid| context::spec_for_session(root, sid));
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
/// resposta a "está aprovada?": o `approve-spec`, a retomada, o `status`, a
/// página da spec e o `wave-scaffold` perguntam aqui.
#[must_use]
pub(crate) fn approved(root: &Path, spec: &str) -> bool {
    lock_state(root, spec).is_some_and(|state| state.approved)
}

/// O estado que a trava da aprovação lê, pela regra única do núcleo
/// ([`mustard_core::domain::spec_state::lock_state_of`]), com o arquivo de
/// eventos e o `meta.json` da spec, os do checkout principal num worktree. O
/// portão, a testemunha, o nascimento antes do avanço e [`approved`] passam
/// por aqui.
///
/// Uma spec sem `state` só segue o `meta.json` ou o arquivo até nascer: toda
/// porta do binário que avança o estágio dela a faz nascer em plano antes
/// ([`crate::commands::spec_events::write::birth_before_advance`]). Daí em
/// diante a trava lê o estado, e a execução só vem depois do "Aprovar".
#[must_use]
pub(crate) fn lock_state(root: &Path, spec: &str) -> Option<State> {
    let meta = mustard_core::ClaudePaths::for_project(store::spec_root(root))
        .and_then(|paths| paths.for_spec(spec.trim()))
        .ok()
        .and_then(|paths| mustard_core::read_meta(&paths.meta_json_path()));
    mustard_core::domain::spec_state::lock_state_of(DiskSpecState::new(root).log(spec).as_ref(), meta.as_ref())
}

/// A spec ainda não nasceu no arquivo de eventos: não há nenhum `state`
/// visível, com ou sem arquivo.
#[must_use]
pub(crate) fn unborn(root: &Path, spec: &str) -> bool {
    DiskSpecState::new(root)
        .log(spec)
        .is_none_or(|log| mustard_core::domain::spec_state::birth_event(&log).is_none())
}

/// A aprovação que vale de uma spec: a pergunta, a opção que o usuário
/// escolheu e a hora em que a testemunha gravou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Approval {
    pub(crate) question: String,
    pub(crate) answer: String,
    pub(crate) at: String,
}

/// A aprovação da spec `spec`: o `state` visível mais novo que traz a
/// testemunha, enquanto a spec continua aprovada. `None` numa spec que não
/// está aprovada ou que não tem arquivo de eventos.
#[must_use]
pub(crate) fn approval(root: &Path, spec: &str) -> Option<Approval> {
    let log = DiskSpecState::new(root).log(spec)?;
    if !State::from_log(&log).approved {
        return None;
    }
    let event = log
        .block(BlockQuery::Block(Block::State))
        .into_iter()
        .filter(|event| event.event_type == "state")
        .filter(|event| event.fields.get("witness").is_some_and(Value::is_object))
        .max_by_key(|event| event.id)?;
    let witness = &event.fields["witness"];
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
    use mustard_core::domain::model::contract::HookInput;
    use tempfile::tempdir;

    const SESSION: &str = "s-lado-a-lado";

    /// Every door's own resolver, called the way the door calls it.
    fn every_door(root: &str) -> Vec<(&'static str, Option<String>)> {
        let input = HookInput { session_id: Some(SESSION.to_string()), ..HookInput::default() };
        vec![
            (
                "clarification_observer",
                crate::hooks::observe::clarification_observer::active_unit(root, &input),
            ),
            (
                "change_request",
                crate::commands::spec::change_request::resolve_spec(root, None, Some(SESSION)),
            ),
            (
                "change_request_log",
                crate::hooks::observe::change_request_log::resolve_spec(root, Some(SESSION)),
            ),
            (
                "boundary_gate",
                crate::hooks::write::boundary_gate::resolve_boundary_spec(root, Some(SESSION)),
            ),
            ("subagent_inject", crate::hooks::task::subagent_inject::capture_spec(root, SESSION)),
            ("pr_detect", crate::hooks::bash::pr_detect::detect_recent_spec(root, Some(SESSION))),
            ("grill_capture", crate::commands::grill_capture::finalize_spec(root, "", Some(SESSION))),
            ("route", crate::shared::events::route::spec_of_event(None, root, Some(SESSION))),
            (
                "post_edit",
                crate::hooks::write::post_edit::find_active_spec(root, Some(SESSION))
                    .map(|(_, name)| name),
            ),
            ("read", DiskSpecState::new(Path::new(root)).active(Some(SESSION))),
        ]
    }

    /// A spec folder with a `spec.md`, which the checklist door also needs.
    fn spec_with_md(root: &std::path::Path, spec: &str) {
        let dir = root.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("spec.md"), "# Spec\n").unwrap();
    }

    fn assert_all_name(root: &str, expected: &str) {
        for (door, got) in every_door(root) {
            assert_eq!(got.as_deref(), Some(expected), "the {door} door named another spec");
        }
    }

    #[test]
    fn every_door_names_the_same_spec_for_the_same_session() {
        // An inherited override answers first at every door by design; the
        // rungs below it are what is under test.
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        // The checkout stands on one spec's branch while the session is bound
        // to another: the branch wins at every door.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        spec_with_md(root, "da-branch");
        spec_with_md(root, "da-sessao");
        stand_on_spec_branch(root, "da-branch");
        context::bind_session_spec(root_str, SESSION, "da-sessao");
        assert_all_name(root_str, "da-branch");

        // Off any spec branch, the session binding answers at every door.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        spec_with_md(root, "da-sessao");
        context::bind_session_spec(root_str, SESSION, "da-sessao");
        assert_all_name(root_str, "da-sessao");
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
        context::bind_session_spec(root, SESSION, "da-sessao");
        assert_eq!(active_spec(root, Some(SESSION)).as_deref(), Some("da-sessao"));
        assert_eq!(active_spec(root, Some("outra-sessao")), None);
        assert_eq!(active_spec(root, None), None);
    }

    /// Cada leitor de "está aprovada?", chamado como ele se chama, sobre o
    /// checkout em `root` e a pasta `spec_dir` da spec `epic` vista dali.
    fn readers(root: &Path, spec_dir: &Path) -> Vec<(&'static str, bool)> {
        let root_str = root.to_string_lossy();
        vec![
            ("approve-spec", !crate::commands::spec::approve_spec::approval_missing(&root_str, "epic")),
            (
                "resume-bootstrap",
                crate::commands::pipeline::resume_bootstrap::bootstrap(root, "epic").approved_by_user,
            ),
            ("status", crate::commands::pipeline::status::approval_of(root, "epic").is_some()),
            ("spec-doc", crate::commands::spec::spec_doc::is_approved(root, "epic")),
            ("wave-scaffold", crate::commands::wave::wave_scaffold::is_approved(spec_dir)),
        ]
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git {args:?} failed");
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

    /// Vistos de um worktree ligado, os leitores respondem pelo estado do
    /// checkout principal: uma spec aprovada ali é aprovada para todos.
    #[test]
    fn every_reader_agrees_from_a_linked_worktree() {
        let tmp = tempdir().unwrap();
        let (main, wt) = main_and_worktree(tmp.path(), "epic");
        approve_in(&main.join(".claude").join("spec").join("epic"));
        let seen_from = wt.join(".claude").join("spec").join("epic");
        for (reader, approved) in readers(&wt, &seen_from) {
            assert!(approved, "{reader} misses the approval from the worktree");
        }
    }

    /// Os leitores de "está aprovada?" dão a mesma resposta que o estado, lado
    /// a lado: o `approve-spec`, a retomada, o `status`, a página da spec e o
    /// `wave-scaffold`, com a spec em plano e depois de aprovada.
    #[test]
    fn every_reader_of_the_approval_agrees_with_the_state() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec_dir = root.join(".claude").join("spec").join("epic");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let events = spec_dir.join("spec.ndjson");

        // Lado a lado, as specs sem `state`: sem arquivo nem `meta.json`, com
        // um recado só e com o `meta.json` só. Nenhum leitor lê aprovada, e a
        // trava diz o que a regra diz.
        let unapproved = |case: &str| {
            for (reader, approved) in readers(root, &spec_dir) {
                assert!(!approved, "{reader} reads {case} as approved");
            }
            assert!(!super::approved(root, "epic"), "{case}");
        };
        unapproved("no file and no meta");
        assert_eq!(lock_state(root, "epic"), None, "the branch the Mustard did not open is free");
        let note = serde_json::json!({ "author": "user", "text": "oi" });
        store::write(&events, "message", note.as_object().cloned().unwrap(), &[]).unwrap();
        unapproved("a note");
        assert_eq!(lock_state(root, "epic").and_then(|state| state.phase), Some("plan"), "a note alone is a plan");
        std::fs::remove_file(&events).unwrap();
        std::fs::write(spec_dir.join("meta.json"), r#"{"scope":"light","stage":"Plan"}"#).unwrap();
        unapproved("a meta");
        assert_eq!(lock_state(root, "epic").and_then(|state| state.phase), Some("plan"), "a draft is a plan");

        let plan = serde_json::json!({ "phase": "plan" });
        store::write(&events, "state", plan.as_object().cloned().unwrap(), &[]).unwrap();

        let readers = || readers(root, &spec_dir);
        let disk = DiskSpecState::new(root);
        for (reader, approved) in readers() {
            assert_eq!(approved, disk.state("epic").unwrap().approved, "{reader} disagrees in plan");
            assert!(!approved, "{reader} reads a plan as approved");
        }

        let approve = serde_json::json!({
            "phase": "approved",
            "author": "user",
            "witness": { "question": "Aprovar esta spec?", "answer": "Aprovar" }
        });
        store::write(&spec_dir.join("spec.ndjson"), "state", approve.as_object().cloned().unwrap(), &[])
            .unwrap();
        for (reader, approved) in readers() {
            assert_eq!(approved, disk.state("epic").unwrap().approved, "{reader} disagrees once approved");
            assert!(approved, "{reader} misses the approval");
        }
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
}
