//! `pr_detect` — DORA telemetry on `gh pr` commands (PostToolUse(Bash)).
//!
//! Classification plus a best-effort `pr.opened` / `pr.merged` harness event.
//! Never affects a verdict. Ported from `pr-detect.js`, since corrected where
//! the port faithfully carried the original's blind spots: a command chain hid
//! the PR verb, and spec attribution read a directory the harness stopped
//! writing.
//!
//! Um `gh pr merge` digitado no terminal também é um merge: quando a branch de
//! origem do pull request é a branch gravada no estado de uma spec, a ponte
//! grava a fase `delivered` nela e arma a cobrança das pendências, como o
//! `pr-merge` faz ([`delivered_spec`]).

use mustard_core::domain::model::contract::HookInput;
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::domain::spec_state::SpecState;
use mustard_core::time::now_iso8601;
use serde_json::json;
use std::path::Path;
use std::process::{Command, Stdio};

use super::lex::truncate;

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

/// As flags do `gh pr merge` que levam um valor: o valor não é o pull request.
const MERGE_VALUED_FLAGS: &[&str] = &[
    "-b",
    "--body",
    "-F",
    "--body-file",
    "-t",
    "--subject",
    "--match-head-commit",
    "-A",
    "--author-email",
    "-R",
    "--repo",
];

/// A branch que um `gh pr merge` nomeia, quando nomeia uma: o primeiro
/// argumento solto depois de `merge`. `None` quando o comando escolhe o pull
/// request pelo número ou pelo endereço, ou não escolhe (é o da branch atual).
fn merge_selector(command: &str) -> Option<String> {
    let masked = super::lex::mask_quoted_operators(command);
    let segment = masked
        .split(super::lex::is_cmd_separator)
        .find(|segment| classify_pr_segment(segment) == Some("pr.merged"))?;
    let cleaned = super::lex::strip_leading_rtk(segment.trim()).trim_start();
    let mut tokens = cleaned.split_whitespace().skip(3);
    while let Some(token) = tokens.next() {
        if MERGE_VALUED_FLAGS.contains(&token) {
            tokens.next();
            continue;
        }
        if token.starts_with('-') {
            continue;
        }
        let number = token.trim_start_matches('#');
        if (!number.is_empty() && number.chars().all(|c| c.is_ascii_digit())) || token.contains("://") {
            return None;
        }
        return Some(token.trim_matches(['"', '\'']).to_string());
    }
    None
}

/// A spec que um `gh pr merge` entregou: a da branch de origem do pull
/// request — a nomeada no comando ou, sem nome, a do checkout —, e só quando
/// essa é a branch gravada no estado da spec. A promoção de `dev` para `main`
/// sai de uma base, que não é a branch de spec nenhuma, então nada é gravado
/// na spec que a sessão estiver seguindo.
///
/// Um `gh pr merge --delete-branch` sem nome de branch volta o checkout para a
/// base antes de o gancho rodar: a branch de origem não é mais vista, e nada é
/// gravado. A porta que sempre grava é o `mustard-rt run pr-merge`.
fn delivered_spec(
    project_dir: &str,
    session_id: Option<&str>,
    command: &str,
    branch: Option<&str>,
) -> Option<String> {
    let head = merge_selector(command).or_else(|| branch.map(str::to_string))?;
    let project = Path::new(project_dir);
    let config = mustard_core::ProjectConfig::load(project);
    let spec = crate::commands::event::work_branch::slug_of_work_branch(&head, &config)
        .or_else(|| detect_recent_spec(project_dir, session_id))?;
    let state = crate::shared::spec_state::DiskSpecState::new(project).state(&spec)?;
    (state.branch.as_deref() == Some(head.as_str())).then_some(spec)
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
    // A ponte do merge, como no `pr-merge`: a fase `delivered` na spec cuja
    // branch foi mergeada, e a cobrança das pendências armada.
    if event == "pr.merged"
        && let Some(delivered) = delivered_spec(project_dir, session_id, command, branch.as_deref())
    {
        let _ = crate::commands::spec_events::write::record_phase(Path::new(project_dir), &delivered, "delivered");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// O argumento solto do `gh pr merge` é a branch só quando não é número
    /// nem endereço, e o valor de uma flag não conta.
    #[test]
    fn the_merge_selector_is_a_branch_only_when_it_names_one() {
        assert_eq!(merge_selector("gh pr merge feature/trava --merge").as_deref(), Some("feature/trava"));
        assert_eq!(merge_selector("cd x && rtk gh pr merge --squash -t \"titulo\" fix/y").as_deref(), Some("fix/y"));
        assert_eq!(merge_selector("gh pr merge 42 --squash"), None);
        assert_eq!(merge_selector("gh pr merge #42"), None);
        assert_eq!(merge_selector("gh pr merge https://github.com/o/r/pull/42"), None);
        assert_eq!(merge_selector("gh pr merge --merge --delete-branch"), None);
    }

    fn git(dir: &std::path::Path, args: &[&str]) {
        let ok = Command::new("git").args(args).current_dir(dir).output().map(|o| o.status.success()).unwrap_or(false);
        assert!(ok, "git {args:?} failed in {}", dir.display());
    }

    /// Um projeto do fluxo `dev`/`main`, parado em `branch`, com a spec `trava`
    /// gravada na branch `feature/trava`, uma pendência nascida nela e a sessão
    /// `s-pr` ligada a ela.
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
        let path = mustard_core::io::spec_events::spec_file(root, "trava").expect("spec file");
        let branch = json!({ "phase": "running", "branch": "feature/trava" }).as_object().cloned().expect("object");
        mustard_core::io::spec_events::write(&path, "state", branch, &[]).expect("state");
        dir
    }

    fn phase(root: &std::path::Path) -> Option<&'static str> {
        crate::shared::spec_state::DiskSpecState::new(root).state("trava").and_then(|state| state.phase)
    }

    /// Um `gh pr merge` digitado na branch da spec grava a fase `delivered`
    /// e arma a cobrança das pendências nascidas nela; nomear a branch da spec
    /// no comando vale do mesmo jeito, mesmo parado na base.
    #[test]
    fn a_merge_typed_for_the_spec_branch_records_delivered_and_arms_the_charge() {
        let on_spec = project_on("feature/trava");
        let root = on_spec.path();
        emit_pr_event(&root.to_string_lossy(), Some("s-pr"), "pr.merged", "gh pr merge 42 --merge");
        assert_eq!(phase(root), Some("delivered"), "the merge of the spec branch is recorded");
        let armed = crate::commands::event::pending::armed_charges(root);
        assert_eq!(armed.iter().map(|c| c.spec.as_str()).collect::<Vec<_>>(), vec!["trava"], "{armed:?}");

        let named = project_on("dev");
        emit_pr_event(&named.path().to_string_lossy(), None, "pr.merged", "gh pr merge feature/trava --merge");
        assert_eq!(phase(named.path()), Some("delivered"), "the named branch is the spec's");
    }

    /// A promoção de `dev` para `main`, digitada na base com a sessão ainda
    /// ligada à spec, não grava nada na spec: a branch de origem é uma base.
    #[test]
    fn a_promotion_typed_on_a_base_records_nothing_on_the_spec() {
        let dir = project_on("dev");
        let root = dir.path();
        assert_eq!(detect_recent_spec(&root.to_string_lossy(), Some("s-pr")).as_deref(), Some("trava"));
        emit_pr_event(&root.to_string_lossy(), Some("s-pr"), "pr.merged", "gh pr merge 50 --merge");
        assert_eq!(phase(root), Some("running"), "the promotion leaves the spec as it was");
        assert!(crate::commands::event::pending::armed_charges(root).is_empty(), "nothing is armed");
    }
}
