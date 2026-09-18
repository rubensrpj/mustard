// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! As duas portas antigas da aprovação não aprovam nada.
//!
//! Aceitar um plano no modo de plano (`ExitPlanMode`) e digitar o comando de
//! barra da spec (`/mustard:spec a`, `/mustard:spec ar`, ou `/mustard:spec`
//! dentro da branch da spec) continuam chegando ao binário, e nenhum dos dois
//! grava a aprovação: a spec continua em plano. A aprovação tem uma porta só,
//! a escolha de "Aprovar" na pergunta.
//!
//! Roda `mustard-rt on` como processo, porque os ganchos são privados da
//! biblioteca.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use mustard_core::io::spec_events as store;
use serde_json::{json, Value};

fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(ok, "git {args:?} failed");
}

/// Roda o gancho `event` com `payload` na pasta `cwd`, e confere a saída 0.
fn fire(cwd: &Path, event: &str, payload: Value) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["on", event])
        .current_dir(cwd)
        .env("CLAUDE_PROJECT_DIR", cwd)
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .env_remove("MUSTARD_SESSION_ID")
        .env_remove("CLAUDE_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mustard-rt");
    if let Some(mut stdin) = child.stdin.take() {
        let _ = write!(stdin, "{payload}");
    }
    let status = child.wait().expect("wait");
    assert_eq!(status.code(), Some(0), "a hook always exits 0");
}

/// A spec `epic` em plano, do jeito que o fluxo de hoje a deixa: o estado no
/// `spec.ndjson`, o `spec.md` e o `meta.json` do rascunho, a ligação da
/// sessão e o checkout na branch dela.
fn spec_in_plan(project: &Path, session: &str) {
    git(project, &["init", "-q"]);
    git(project, &["config", "user.email", "t@example.com"]);
    git(project, &["config", "user.name", "t"]);
    git(project, &["checkout", "-q", "-b", "feature/epic"]);
    let dir = project.join(".claude").join("spec").join("epic");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("spec.md"), "# epic\n\n## Resumo\n\nlinha.\n").unwrap();
    fs::write(dir.join("meta.json"), r#"{"scope":"full (wave plan)","stage":"Plan","outcome":"Active"}"#)
        .unwrap();
    let plan = json!({ "phase": "plan", "branch": "feature/epic" });
    store::write(&dir.join("spec.ndjson"), "state", plan.as_object().cloned().unwrap(), &[]).unwrap();
    let session_dir = project.join(".claude").join(".session").join(session);
    fs::create_dir_all(&session_dir).unwrap();
    fs::write(session_dir.join("active-spec"), "epic").unwrap();
}

/// A fase da spec e se ainda sobra alguma marca de aprovação na pasta dela.
fn standing(project: &Path) -> (String, bool) {
    let dir = project.join(".claude").join("spec").join("epic");
    let body = fs::read_to_string(dir.join("spec.ndjson")).unwrap();
    let phase = body
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v["type"] == "state")
        .and_then(|v| v["phase"].as_str().map(str::to_string))
        .unwrap_or_default();
    (phase, dir.join(".approved-by-user").exists())
}

#[test]
fn accepting_plan_mode_approves_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    spec_in_plan(project, "s-plan");

    fire(
        project,
        "PostToolUse",
        json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "ExitPlanMode",
            "tool_input": { "plan": "# The plan" },
            "tool_response": { "plan": "# Approved plan body" },
            "session_id": "s-plan",
            "cwd": project.to_str().unwrap()
        }),
    );

    assert_eq!(standing(project), ("plan".to_string(), false), "plan mode approves nothing");
}

#[test]
fn the_slash_command_letter_approves_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    spec_in_plan(project, "s-picker");

    for prompt in ["/mustard:spec a", "/mustard:spec ar", "/mustard:spec r", "/mustard:spec"] {
        fire(
            project,
            "UserPromptSubmit",
            json!({
                "hook_event_name": "UserPromptSubmit",
                "prompt": prompt,
                "session_id": "s-picker",
                "cwd": project.to_str().unwrap()
            }),
        );
        assert_eq!(standing(project), ("plan".to_string(), false), "{prompt} approves nothing");
    }
}
