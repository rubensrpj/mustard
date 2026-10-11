//! Regression of the actual reported command, with synthetic credentials.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{io::Write, path::Path, process::{Command, Stdio}};
use serde_json::{json, Value};

const SECRET: &str = "synthetic-Jev-key-719235";
const CONFIG: &str = "{\n  \"git\": {\"flow\": {\"*\": \"develop\"}},\n  \"testCommand\": \"pnpm test --roots '<rootDir>/src'\",\n  \"lintCommand\": \"pnpm lint --ignore-pattern .claude/\",\n  \"jev\": {\"key\": \"synthetic-Jev-key-719235\"}\n}\n";

fn hook(root: &Path, command: &str) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt")).args(["on", "PreToolUse"])
        .current_dir(root).env_remove("CLAUDE_PROJECT_DIR").env_remove("MUSTARD_WORKSPACE_ROOT")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let request = json!({"hook_event_name":"PreToolUse","tool_name":"Bash","cwd":root,
        "session_id":"config-read-regression","tool_input":{"command":command,"timeout":3500,"description":"config and git"}});
    child.stdin.take().unwrap().write_all(request.to_string().as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

fn native(root: &Path, file: &str, reader: &str, args: Value) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "config-read", "--file", file, "--reader", reader, "--args-json", &args.to_string()])
        .current_dir(root).output().unwrap()
}

fn shell(root: &Path, command: &str) -> std::process::Output {
    mustard_rt::util::platform::build_shell_command(command).current_dir(root).output().unwrap()
}

#[test]
fn filtered_config_and_git_execute_once_without_a_secret_or_full_config_dump() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mustard.json"), CONFIG).unwrap();
    assert!(Command::new("git").args(["init", "-q"]).current_dir(dir.path()).status().unwrap().success());
    std::fs::write(dir.path().join("untracked.txt"), "test").unwrap();
    let original = r#"grep -n -E "ignore-pattern|--roots|taskset|\"enabled\"|\"rtk\"|flow|protected" mustard.json | cut -c1-200; echo "--- git"; git status --short | head -5; echo "fim""#;
    let result = hook(dir.path(), original);
    let specific = &result["hookSpecificOutput"];
    assert!(specific.get("permissionDecision").is_none(), "{result}");
    assert!(specific.get("additionalContext").is_none(), "no configuration dump: {result}");
    let updated = &specific["updatedInput"];
    assert_eq!(updated["timeout"], 3500);
    assert_eq!(updated["description"], "config and git");
    let command = updated["command"].as_str().unwrap();
    assert!(command.ends_with(r#" | cut -c1-200; echo "--- git"; git status --short | head -5; echo "fim""#), "{command}");
    assert!(!result.to_string().contains(SECRET));
    // The hook itself executed no part of the requested command.
    assert!(!result.to_string().contains("?? untracked.txt"));
    let out = shell(dir.path(), command);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("2:  \"git\"") && text.contains("3:  \"testCommand\"") && text.contains("4:  \"lintCommand\""), "{text}");
    assert!(text.contains("?? untracked.txt") && text.contains("--- git") && text.contains("fim"), "{text}");
    assert!(!text.contains("jev") && !text.contains(SECRET), "{text}");
    assert_eq!(std::fs::read_to_string(dir.path().join("mustard.json")).unwrap(), CONFIG);
    let result = hook(dir.path(), "grep -q flow mustard.json && printf tested >> run.count");
    assert!(!dir.path().join("run.count").exists(), "hook must never run the suffix");
    let out = shell(dir.path(), result["hookSpecificOutput"]["updatedInput"]["command"].as_str().unwrap());
    assert!(out.status.success());
    assert_eq!(std::fs::read_to_string(dir.path().join("run.count")).unwrap(), "tested");
}

#[test]
fn masked_reads_preserve_no_match_conditions_and_read_latest_bytes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mustard.json"), CONFIG).unwrap();
    for (original, expected) in [
        ("grep absent mustard.json && echo WRONG || echo fallback", "fallback\n"),
        ("grep absent mustard.json; echo continued", "continued\n"),
        ("grep -q flow mustard.json && echo found", "found\n"),
        ("echo 'início'; (grep -q flow mustard.json) && echo found", "início\nfound\n"),
        ("cat mustard.json | grep -n key mustard.json", "5:  \"jev\": {\"key\": \"***\"}\n"),
        ("cat mustard.json | grep key", "  \"jev\": {\"key\": \"***\"}\n"),
        ("grep -nH flow mustard.json", "mustard.json:2:  \"git\": {\"flow\": {\"*\": \"develop\"}},\n"),
    ] {
        let result = hook(dir.path(), original);
        let command = result["hookSpecificOutput"]["updatedInput"]["command"].as_str().unwrap();
        let out = shell(dir.path(), command);
        assert!(out.status.success(), "{original}: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8(out.stdout).unwrap(), expected, "{original}");
    }
    let result = hook(dir.path(), "grep absent mustard.json");
    assert_eq!(shell(dir.path(), result["hookSpecificOutput"]["updatedInput"]["command"].as_str().unwrap()).status.code(), Some(1));
    let result = hook(dir.path(), "cat mustard.json");
    std::fs::write(dir.path().join("mustard.json"), r#"{"jev":{"k\u0065y":"changed-secret-81"},"new":true}"#).unwrap();
    let out = shell(dir.path(), result["hookSpecificOutput"]["updatedInput"]["command"].as_str().unwrap());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), r#"{"jev":{"k\u0065y":"***"},"new":true}"#);
}

#[test]
fn unsupported_or_mixed_reads_do_not_execute_any_suffix_or_dump_the_config() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mustard.json"), CONFIG).unwrap();
    for original in [
        "cat mustard.json; cat *; touch wrong",
        "cat mustard.json; grep -r key .; touch wrong",
        "cat mustard.json; echo $(cat mustard.json); touch wrong",
        "jq . < mustard.json; touch wrong",
        "cat mustard.json; bash -c 'cat mustard.json'; touch wrong",
        "cat=unexpected cat mustard.json; touch wrong",
        "grep * mustard.json; touch wrong",
        "cat mustard.json; rm -rf work",
    ] {
        let result = hook(dir.path(), original);
        let specific = &result["hookSpecificOutput"];
        assert_eq!(specific["permissionDecision"], "deny", "{original}: {result}");
        assert!(specific.get("updatedInput").is_none(), "{result}");
        assert!(!result.to_string().contains(SECRET) && !result.to_string().contains("testCommand"), "{result}");
        assert!(!dir.path().join("wrong").exists());
    }
    let out = native(dir.path(), "mustard.json", "awk", json!(["{system(\"touch wrong\")}"]));
    assert!(!out.status.success() && out.stdout.is_empty());
    assert!(!dir.path().join("wrong").exists());
    let missing = native(dir.path(), "missing/mustard.json", "cat", json!([]));
    assert!(!missing.status.success() && missing.stdout.is_empty());
}
