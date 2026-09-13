// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The command guard from the outside: the binary answers a `PreToolUse` of
//! the Bash tool the way the Claude Code sends it, always in a temporary
//! project, never in the real one.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;
use tempfile::TempDir;

/// Run `mustard-rt on PreToolUse` for one Bash command in `dir` and return
/// what it printed. The commit gate is off so it never joins the answer.
fn run_guard(dir: &Path, command: &str) -> String {
    let input = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "cwd": dir.to_str().expect("utf-8 path"),
        "session_id": "command-guard-test",
        "tool_input": { "command": command }
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["on", "PreToolUse"])
        .current_dir(dir)
        .env("MUSTARD_COMMIT_GATE_MODE", "off")
        .env_remove("CLAUDE_PROJECT_DIR")
        .env_remove("MUSTARD_WORKSPACE_ROOT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mustard-rt");
    child
        .stdin
        .take()
        .expect("stdin pipe")
        .write_all(input.to_string().as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait mustard-rt");
    assert!(
        output.status.success(),
        "mustard-rt exited {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The refusal the hook printed, when it refused.
fn refusal(stdout: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(stdout.trim()).ok()?;
    if parsed.pointer("/hookSpecificOutput/permissionDecision")?.as_str()? != "deny" {
        return None;
    }
    let reason = parsed.pointer("/hookSpecificOutput/permissionDecisionReason").and_then(Value::as_str);
    Some(reason.unwrap_or_default().to_string())
}

fn project_with(config: &str) -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("mustard.json"), config).expect("write mustard.json");
    dir
}

/// Every line of every `.ndjson` file under `root`.
fn ndjson_lines(root: &Path) -> Vec<String> {
    let mut lines = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return lines;
    };
    for path in entries.flatten().map(|e| e.path()) {
        if path.is_dir() {
            lines.extend(ndjson_lines(&path));
        } else if path.extension().is_some_and(|e| e == "ndjson") {
            let body = std::fs::read_to_string(&path).unwrap_or_default();
            lines.extend(body.lines().map(str::to_string));
        }
    }
    lines
}

/// A commit message and a pending-item title that only name a dangerous
/// command pass; the real command is refused.
#[test]
fn quoted_text_passes_and_a_real_delete_is_blocked() {
    let dir = TempDir::new().expect("tempdir");
    for command in [
        r#"git commit -m "limpa: rm -rf build antigo""#,
        r#"mustard-rt run pending --add --title "rodar rm -rf target antes do build""#,
    ] {
        let out = run_guard(dir.path(), command);
        assert!(refusal(&out).is_none(), "{command} must pass: {out}");
    }
    let out = run_guard(dir.path(), "rm -rf pasta");
    let reason = refusal(&out).unwrap_or_else(|| panic!("rm -rf pasta must be refused: {out}"));
    assert!(reason.starts_with("Comando barrado: apagar pasta à força"), "{reason}");
}

/// The rewrite to `rtk` belongs to rtk's own hook: the guard hands the
/// command back untouched.
#[test]
fn an_unprefixed_command_is_not_rewritten() {
    let dir = TempDir::new().expect("tempdir");
    let out = run_guard(dir.path(), "git status");
    assert!(!out.contains("updatedInput"), "{out}");
    assert!(refusal(&out).is_none(), "{out}");
}

#[test]
fn the_guard_leaves_no_rewrite_event_behind() {
    let dir = TempDir::new().expect("tempdir");
    for command in ["git status", "rm -rf pasta", "cargo build"] {
        run_guard(dir.path(), command);
    }
    let rewrites: Vec<String> =
        ndjson_lines(dir.path()).into_iter().filter(|line| line.contains("rtk-rewrite")).collect();
    assert!(rewrites.is_empty(), "{rewrites:?}");
}

#[test]
fn a_flow_base_is_protected_and_other_branches_are_not() {
    let dir = project_with(r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#);
    let out = run_guard(dir.path(), "git branch -D dev");
    let reason = refusal(&out).unwrap_or_else(|| panic!("deleting dev must be refused: {out}"));
    assert!(reason.contains("`dev`"), "{reason}");
    let out = run_guard(dir.path(), "git branch -D feature/x");
    assert!(refusal(&out).is_none(), "{out}");
}

#[test]
fn the_refusal_follows_the_project_language() {
    let dir = project_with(r#"{"language":{"text":"en-US"}}"#);
    let out = run_guard(dir.path(), "rm -rf pasta");
    let reason = refusal(&out).unwrap_or_else(|| panic!("rm -rf pasta must be refused: {out}"));
    assert!(reason.starts_with("Command blocked: deleting a folder by force"), "{reason}");
}
