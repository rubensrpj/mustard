//! `pr_detect` — DORA telemetry on `gh pr` commands (PostToolUse(Bash)).
//!
//! Pure telemetry — classification plus a best-effort `pr.opened` /
//! `pr.merged` harness event. Never affects a verdict. 1:1 port of
//! `pr-detect.js`.

use mustard_core::ClaudePaths;
use mustard_core::domain::model::contract::HookInput;
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::time::now_iso8601;
use serde_json::json;
use std::path::Path;
use std::process::{Command, Stdio};

use super::lex::truncate;

/// Classify a command as a PR event. Mirrors `classify` in `pr-detect.js`:
/// a conservative match at the start of the token sequence, tolerating a
/// leading `rtk` wrapper.
pub(super) fn classify_pr(command: &str) -> Option<&'static str> {
    let cleaned = command.trim();
    // Strip a leading `rtk ` wrapper (case-insensitive).
    // .get() keeps this panic-proof on multi-byte UTF-8 near the boundary.
    let cleaned = if cleaned.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("rtk ")) {
        cleaned[4..].trim_start()
    } else {
        cleaned
    };
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

/// The most recently modified `.pipeline-states/*.json` (excluding
/// `*.metrics.json`), by mtime. Mirrors `detectMostRecentSpec` in
/// `pr-detect.js`. Fail-open `None`.
fn detect_recent_spec(project_dir: &str) -> Option<String> {
    let paths = ClaudePaths::for_project(Path::new(project_dir)).ok()?;
    let dir = paths.pipeline_states_dir();
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for entry in entries.filter_map(std::result::Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !std::path::Path::new(&name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json")) || name.ends_with(".metrics.json") {
            continue;
        }
        let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            let spec = name.trim_end_matches(".json").to_string();
            best = Some((mtime, spec));
        }
    }
    best.map(|(_, spec)| spec)
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
    let spec = detect_recent_spec(project_dir);
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
    // `pr.detect` family events are non-pipeline → NDJSON via W5 router.
    let _ = crate::shared::events::route::emit(project_dir, &harness_event);
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
}
