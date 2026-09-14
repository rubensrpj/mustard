// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(unix)]

//! Um `gh pr merge` digitado no terminal só vira o evento `pr.merged` no log
//! velho. O gancho não pergunta nada ao GitHub, não entrega a spec e não arma
//! a cobrança das pendências: o `spec.ndjson` e o `charges.json` ficam com os
//! mesmos bytes.
//!
//! Roda `mustard-rt on PostToolUse` como processo, com um `gh` falso na frente
//! do PATH que anota se foi chamado e responde que a branch da spec acabou de
//! entrar por merge. O `CLAUDE_PROJECT_DIR` fica na pasta temporária, para o
//! gancho nunca gravar no projeto de verdade.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

const SESSION: &str = "s-pr";

fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git").args(args).current_dir(root).output().map(|o| o.status.success()).unwrap_or(false);
    assert!(ok, "git {args:?} failed");
}

/// Toda linha de todo arquivo debaixo de `dir`, menos o `spec.ndjson`.
fn event_lines(dir: &Path) -> Vec<String> {
    let mut lines = Vec::new();
    let mut stack: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(entries) = fs::read_dir(&next) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n != "spec.ndjson") {
                lines.extend(fs::read_to_string(&path).unwrap_or_default().lines().map(str::to_string));
            }
        }
    }
    lines
}

#[test]
fn a_typed_merge_records_only_the_event() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::write(root.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["checkout", "-q", "-b", "feature/trava"]);
    git(root, &["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", "root"]);
    let spec_file = mustard_core::io::spec_events::spec_file(root, "trava").unwrap();
    fs::create_dir_all(spec_file.parent().unwrap()).unwrap();
    let running = json!({ "phase": "running", "branch": "feature/trava" });
    mustard_core::io::spec_events::write(&spec_file, "state", running.as_object().cloned().unwrap(), &[]).unwrap();

    // O `gh` falso: anota a chamada e responde que a branch da spec entrou
    // por merge agora há pouco.
    let bin = root.join("fake-bin");
    fs::create_dir_all(&bin).unwrap();
    let called = root.join("gh-called");
    let merged = r#"[{"number":7,"head":{"ref":"feature/trava","repo":{"full_name":"o/r"}},"base":{"repo":{"full_name":"o/r"}},"merged_at":"2999-01-01T00:00:00Z"}]"#;
    let script = format!("#!/bin/sh\ntouch '{}'\necho '{merged}'\n", called.display());
    fs::write(bin.join("gh"), script).unwrap();
    fs::set_permissions(bin.join("gh"), fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());

    let charges = root.join(".claude").join("pending").join("charges.json");
    let spec_before = fs::read(&spec_file).unwrap();
    let charges_before = fs::read(&charges).ok();

    let input = json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_input": { "command": "gh pr merge 42 --merge" },
        "tool_response": { "exit_code": 0 },
        "session_id": SESSION,
        "cwd": root
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["on", "PostToolUse"])
        .current_dir(root)
        .env("PATH", path)
        .env("CLAUDE_PROJECT_DIR", root)
        .env_remove("MUSTARD_PROJECT_ROOT")
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
        let _ = write!(stdin, "{input}");
    }
    assert_eq!(child.wait().expect("wait").code(), Some(0), "a hook always exits 0");

    let merges: Vec<Value> = event_lines(&root.join(".claude"))
        .iter()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|event| event.to_string().contains("pr.merged"))
        .collect();
    assert_eq!(merges.len(), 1, "the typed merge is recorded once in the old log: {merges:?}");
    assert!(!called.exists(), "the hook never asks gh");
    assert_eq!(fs::read(&spec_file).unwrap(), spec_before, "the spec file stays the same");
    assert_eq!(fs::read(&charges).ok(), charges_before, "the pending charges stay the same");
}
