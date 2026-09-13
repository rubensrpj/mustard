//! `pr_detect` — DORA telemetry on `gh pr` commands (PostToolUse(Bash)).
//!
//! Classification plus a best-effort `pr.opened` / `pr.merged` harness event.
//! Never affects a verdict. Ported from `pr-detect.js`, since corrected where
//! the port faithfully carried the original's blind spots: a command chain hid
//! the PR verb, and spec attribution read a directory the harness stopped
//! writing.
//!
//! Um `gh pr merge` digitado no terminal também é um merge. Depois de um
//! comando com ele que terminou bem, em qualquer forma, o gancho não tenta
//! descobrir a branch pelo texto do comando (número, endereço, `-R`, `--auto`,
//! `--delete-branch`): pergunta ao `gh` o estado do pull request de cada spec
//! candidata, pela branch gravada no estado dela. Só com o estado `MERGED` e a
//! branch de origem igual à da spec, a ponte grava a fase `delivered` e arma a
//! cobrança das pendências, como o `pr-merge` faz ([`delivered_specs`]).

use mustard_core::domain::model::contract::HookInput;
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::domain::spec_state::{is_approved_phase, SpecState};
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::time::now_iso8601;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::lex::truncate;
use crate::shared::spec_state::DiskSpecState;

/// Classify a command as a PR event.
///
/// The predecessor read only the FIRST token of the whole command string, so it
/// saw `gh pr create` alone and nothing else: `git push && gh pr create --fill`,
/// a `cd`-prefixed invocation, or a merge on the second line of a multi-line
/// command all classified to `None` and vanished from the DORA report. Every
/// segment is now classified — the command is split on shell separators exactly
/// as bash reads them (via [`super::lex::is_cmd_separator`], with quoted
/// operators masked first so a `-m "a || b"` message cannot forge a boundary),
/// and each segment is judged on ITS first token.
///
/// Still conservative in the way that matters: the verb must be the segment's
/// leading word, so `echo gh pr create` is not a PR event.
pub(super) fn classify_pr(command: &str) -> Option<&'static str> {
    let masked = super::lex::mask_quoted_operators(command);
    masked
        .split(super::lex::is_cmd_separator)
        .find_map(classify_pr_segment)
}

/// Classify ONE shell segment (no separators inside). A leading `rtk ` wrapper
/// is transparent — the project routes every command through it.
fn classify_pr_segment(segment: &str) -> Option<&'static str> {
    let cleaned = super::lex::strip_leading_rtk(segment.trim()).trim_start();
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();
    if tokens.len() >= 3 && tokens[0].eq_ignore_ascii_case("gh") && tokens[1] == "pr" {
        match tokens[2] {
            "create" => return Some("pr.opened"),
            "merge" => return Some("pr.merged"),
            _ => {}
        }
    }
    None
}

/// O que o `gh` diz do pull request de uma branch: o estado (`OPEN`, `MERGED`
/// ou `CLOSED`) e a branch de origem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrOfBranch {
    pub(crate) state: String,
    pub(crate) head: String,
}

/// Pergunta ao `gh`, no repositório de `root`, pelo pull request da branch
/// `branch`, do mesmo jeito que o `pr-merge` pergunta. `None` quando o `gh`
/// não responde ou a branch não tem pull request.
fn ask_gh(root: &Path, branch: &str) -> Option<PrOfBranch> {
    let args = ["pr", "view", branch, "--json", "state,headRefName,mergedAt"];
    let value = crate::commands::review::pr_door::gh_json(root, &args).ok()?;
    Some(PrOfBranch {
        state: value.get("state")?.as_str()?.to_string(),
        head: value.get("headRefName")?.as_str()?.to_string(),
    })
}

/// Quantas specs candidatas, no máximo, são perguntadas ao `gh` depois de um
/// merge: as mais recentes. Cada pergunta é uma chamada ao `gh`.
const MAX_CANDIDATES: usize = 5;

/// As specs que podem ter acabado de entrar no merge, das mais recentes para
/// as mais antigas: as que têm arquivo de eventos, uma branch gravada no
/// estado e uma fase aprovada que ainda não é `delivered`. Os nomes saem do
/// índice das specs; sem índice, das pastas.
fn candidates(project: &Path) -> Vec<(String, String)> {
    let main = mustard_core::io::spec_events::spec_root(project);
    let Ok(paths) = ClaudePaths::for_project(&main) else {
        return Vec::new();
    };
    let names = names_from_index(&paths.spec_index_path()).unwrap_or_else(|| names_from_folders(&paths.spec_dir()));
    let disk = DiskSpecState::new(project);
    names
        .into_iter()
        .filter_map(|name| {
            let state = disk.state(&name)?;
            let phase = state.phase?;
            if !is_approved_phase(phase) || phase == "delivered" {
                return None;
            }
            Some((name, state.branch?))
        })
        .take(MAX_CANDIDATES)
        .collect()
}

/// Os nomes das specs do índice, da atualização mais nova para a mais velha,
/// já sem as que o índice mostra entregues ou não aprovadas. `None` quando o
/// índice não existe ou não se lê.
fn names_from_index(index: &Path) -> Option<Vec<String>> {
    let body = mustard_core::io::fs::read_to_string(index).ok()?;
    let mut lines: Vec<(String, String)> = body
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|line| {
            line.get("phase").and_then(Value::as_str).is_some_and(|p| is_approved_phase(p) && p != "delivered")
        })
        .filter_map(|line| {
            let name = line.get("name")?.as_str()?.to_string();
            let updated = line.get("updated").and_then(Value::as_str).unwrap_or_default().to_string();
            Some((updated, name))
        })
        .collect();
    lines.sort_by(|a, b| b.0.cmp(&a.0));
    Some(lines.into_iter().map(|(_, name)| name).collect())
}

/// Os nomes das pastas de spec que têm arquivo de eventos, do arquivo
/// mudado por último para o mais velho.
fn names_from_folders(spec_dir: &Path) -> Vec<String> {
    let Ok(entries) = mustard_core::io::fs::read_dir(spec_dir) else {
        return Vec::new();
    };
    let mut specs: Vec<(Option<std::time::SystemTime>, String)> = entries
        .into_iter()
        .filter_map(|entry| {
            let file: PathBuf = entry.path.join("spec.ndjson");
            file.is_file().then(|| (mustard_core::io::fs::modified(&file).ok(), entry.file_name.clone()))
        })
        .collect();
    specs.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    specs.into_iter().map(|(_, name)| name).collect()
}

/// As specs que um merge acabou de entregar: cada candidata cujo pull request,
/// perguntado por `ask` pela branch da spec, está `MERGED` e tem como branch
/// de origem a da spec. Um `--auto` ainda não mergeou; a promoção de `dev`
/// para `main` não mergeia a branch de spec nenhuma.
fn delivered_specs(project: &Path, ask: &dyn Fn(&Path, &str) -> Option<PrOfBranch>) -> Vec<String> {
    candidates(project)
        .into_iter()
        .filter(|(_, branch)| ask(project, branch).is_some_and(|pr| pr.state == "MERGED" && pr.head == *branch))
        .map(|(spec, _)| spec)
        .collect()
}

/// The git branch via `git rev-parse --abbrev-ref HEAD`. Fail-open `None`.
fn detect_branch(project_dir: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(project_dir)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() { None } else { Some(branch) }
}

/// The spec this PR belongs to, for the DORA pairing key: the one current-spec
/// ladder every door shares (the environment override, then the checkout's
/// branch, then the session binding). Fail-open `None` — a PR with no
/// resolvable spec still pairs by branch.
pub(crate) fn detect_recent_spec(project_dir: &str, session_id: Option<&str>) -> Option<String> {
    crate::shared::spec_state::active_spec(project_dir, session_id)
}

/// `true` when the Bash tool reported a non-zero exit code. Mirrors the
/// `tool_response.exit_code` check in `pr-detect.js` — permissive: a missing
/// exit code is treated as success.
pub(super) fn bash_failed(input: &HookInput) -> bool {
    input
        .raw
        .get("tool_response")
        .and_then(|r| r.get("exit_code"))
        .and_then(serde_json::Value::as_i64)
        .is_some_and(|code| code != 0)
}

/// Emit a `pr.opened` / `pr.merged` harness event. Best-effort telemetry.
pub(super) fn emit_pr_event(
    project_dir: &str,
    session_id: Option<&str>,
    event: &str,
    command: &str,
) {
    emit_pr_event_with(project_dir, session_id, event, command, &ask_gh);
}

/// [`emit_pr_event`] com a pergunta ao `gh` dada por quem chama: os testes
/// passam uma resposta pronta.
fn emit_pr_event_with(
    project_dir: &str,
    session_id: Option<&str>,
    event: &str,
    command: &str,
    ask: &dyn Fn(&Path, &str) -> Option<PrOfBranch>,
) {
    let branch = detect_branch(project_dir);
    let spec = detect_recent_spec(project_dir, session_id);
    let command_field = if command.len() > 200 {
        format!("{}...", truncate(command, 200))
    } else {
        command.to_string()
    };
    let harness_event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id.unwrap_or("unknown").to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("pr-detect".to_string()),
            actor_type: None,
        },
        event: event.to_string(),
        payload: json!({
            "branch": branch,
            "spec": spec,
            "command": command_field,
        }),
        spec: spec.clone(),
    };
    // `pr.detect` family events go to the per-spec NDJSON sink through the router.
    let _ = crate::shared::events::route::emit(project_dir, &harness_event);
    // A ponte do merge, como no `pr-merge`: a fase `delivered` em cada spec cujo
    // pull request o `gh` diz mergeado, e a cobrança das pendências armada para
    // a sessão que mergeou.
    if event == "pr.merged" {
        let project = Path::new(project_dir);
        for delivered in delivered_specs(project, ask) {
            let _ = crate::commands::spec_events::write::record_phase_by(project, &delivered, "delivered", session_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// `gh pr create` / `gh pr merge` classify to the right DORA events.
    #[test]
    fn pr_detect_classifies_pr_commands() {
        assert_eq!(classify_pr("gh pr create --fill"), Some("pr.opened"));
        assert_eq!(classify_pr("gh pr merge 42 --squash"), Some("pr.merged"));
        // Tolerates a leading `rtk` wrapper.
        assert_eq!(classify_pr("rtk gh pr create"), Some("pr.opened"));
    }

    /// A non-PR command classifies to nothing.
    #[test]
    fn pr_detect_ignores_non_pr_commands() {
        assert_eq!(classify_pr("gh pr view 42"), None);
        assert_eq!(classify_pr("git commit -m x"), None);
        assert_eq!(classify_pr("gh issue list"), None);
        assert_eq!(classify_pr("echo gh pr create"), None);
    }

    /// A PR command CHAINED after another command is still a PR event. The
    /// first-token-only reader saw none of these, which is how a report can
    /// under-count what opened and show nothing merged over a period that had
    /// merges — every `gh pr` issued as part of a chain was invisible.
    #[test]
    fn pr_detect_sees_through_command_chains() {
        assert_eq!(
            classify_pr("git push -u origin dev && gh pr create --fill"),
            Some("pr.opened"),
        );
        assert_eq!(classify_pr("cd apps/rt; gh pr merge 103 --squash"), Some("pr.merged"));
        // Multi-line commands: bash treats the newline like `;`, so must we.
        assert_eq!(
            classify_pr("echo opening\nrtk gh pr create --fill --base main"),
            Some("pr.opened"),
        );
        // A quoted operator inside a commit message is not a segment boundary,
        // and the quoted text is not a command.
        assert_eq!(classify_pr("git commit -m \"gh pr create || nope\""), None);
        // The verb must still LEAD its own segment — a mention is not an event.
        assert_eq!(classify_pr("echo run && echo gh pr create"), None);
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git").args(args).current_dir(dir).output().map(|o| o.status.success()).unwrap_or(false);
        assert!(ok, "git {args:?} failed in {}", dir.display());
    }

    /// Um projeto do fluxo `dev`/`main`, parado em `branch`, com a spec `trava`
    /// em andamento na branch `feature/trava`, uma pendência nascida nela e a
    /// sessão `s-pr` ligada a ela; e a spec `plano`, ainda em plano, com a
    /// branch `feature/plano`.
    fn project_on(branch: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#).expect("cfg");
        git(root, &["init", "-q"]);
        git(root, &["checkout", "-q", "-b", branch]);
        git(root, &["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", "root"]);
        let added = crate::commands::event::pending::pending_at(&crate::commands::event::pending::PendingOpts {
            root: root.to_path_buf(),
            add: true,
            title: Some("Humanize".into()),
            detail: Some("nasceu na spec".into()),
            ..crate::commands::event::pending::PendingOpts::default()
        });
        assert_eq!(added["ok"], json!(true), "{added}");
        crate::hooks::task::pending_gate::seed_spec(root, "trava", &[1], "s-pr");
        let state = |fields: Value| fields.as_object().cloned().expect("object");
        let trava = mustard_core::io::spec_events::spec_file(root, "trava").expect("spec file");
        mustard_core::io::spec_events::write(&trava, "state", state(json!({ "phase": "running", "branch": "feature/trava" })), &[])
            .expect("state");
        let plano = mustard_core::io::spec_events::spec_file(root, "plano").expect("spec file");
        std::fs::create_dir_all(plano.parent().expect("parent")).expect("spec folder");
        mustard_core::io::spec_events::write(&plano, "state", state(json!({ "phase": "plan", "branch": "feature/plano" })), &[])
            .expect("state");
        dir
    }

    fn phase(root: &Path) -> Option<&'static str> {
        DiskSpecState::new(root).state("trava").and_then(|state| state.phase)
    }

    /// Um `gh` falso que responde `state` para o pull request da branch da
    /// spec `trava` e anota cada branch perguntada.
    fn fake_gh<'a>(state: &'a str, asked: &'a RefCell<Vec<String>>) -> impl Fn(&Path, &str) -> Option<PrOfBranch> + 'a {
        move |_: &Path, branch: &str| {
            asked.borrow_mut().push(branch.to_string());
            (branch == "feature/trava").then(|| PrOfBranch { state: state.to_string(), head: branch.to_string() })
        }
    }

    /// O merge digitado confere o fato no `gh`, pela branch da spec, e não o
    /// texto do comando. Pelo número, pelo endereço e com `--delete-branch`
    /// sem nome, parado já na base, grava `delivered` e arma a cobrança para a
    /// sessão; com `--auto`, o pull request ainda não mergeou, e nada é
    /// gravado; na promoção de `dev` para `main`, o pull request da spec segue
    /// aberto, e nada é gravado. Só a spec aprovada e ainda não entregue é
    /// perguntada.
    #[test]
    fn a_typed_merge_is_checked_against_the_pull_request_of_the_spec() {
        let cases = [
            ("gh pr merge 42 --merge", "feature/trava", "MERGED", Some("delivered")),
            ("gh pr merge https://github.com/o/r/pull/42 --squash", "feature/trava", "MERGED", Some("delivered")),
            ("gh pr merge --auto --merge", "feature/trava", "OPEN", Some("running")),
            ("gh pr merge --merge --delete-branch", "dev", "MERGED", Some("delivered")),
            ("gh pr merge 50 --merge", "dev", "OPEN", Some("running")),
        ];
        for (command, checkout, answer, expected) in cases {
            let dir = project_on(checkout);
            let root = dir.path();
            let asked = RefCell::new(Vec::new());
            emit_pr_event_with(&root.to_string_lossy(), Some("s-pr"), "pr.merged", command, &fake_gh(answer, &asked));
            assert_eq!(phase(root), expected, "{command} on {checkout} answered {answer}");
            assert_eq!(*asked.borrow(), vec!["feature/trava".to_string()], "{command}: only the approved spec is asked");
            let armed = crate::commands::event::pending::armed_charges(root);
            if expected == Some("delivered") {
                assert_eq!(armed.len(), 1, "{command}: the charge is armed: {armed:?}");
                assert_eq!(armed[0].session.as_deref(), Some("s-pr"), "{command}: for the session that merged");
            } else {
                assert!(armed.is_empty(), "{command}: nothing is armed: {armed:?}");
            }
        }
    }

    /// Uma spec já entregue não é perguntada de novo, e um `gh pr create` não
    /// pergunta nada.
    #[test]
    fn a_delivered_spec_is_not_asked_again_and_an_opened_pr_asks_nothing() {
        let dir = project_on("feature/trava");
        let root = dir.path();
        let asked = RefCell::new(Vec::new());
        let gh = fake_gh("MERGED", &asked);
        emit_pr_event_with(&root.to_string_lossy(), Some("s-pr"), "pr.opened", "gh pr create --fill", &gh);
        assert!(asked.borrow().is_empty(), "opening a pull request asks nothing");
        emit_pr_event_with(&root.to_string_lossy(), Some("s-pr"), "pr.merged", "gh pr merge 42", &gh);
        assert_eq!(phase(root), Some("delivered"));
        asked.borrow_mut().clear();
        emit_pr_event_with(&root.to_string_lossy(), Some("s-pr"), "pr.merged", "gh pr merge 43", &gh);
        assert!(asked.borrow().is_empty(), "a delivered spec is no candidate: {:?}", asked.borrow());
    }
}
