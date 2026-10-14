//! Regression of the actual reported command, with synthetic credentials.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{io::Write, path::Path, process::{Command, Stdio}};
use serde_json::{json, Value};

const SECRET: &str = "synthetic-Jev-key-719235";
const CONFIG: &str = "{\n  \"git\": {\"flow\": {\"*\": \"develop\"}},\n  \"testCommand\": \"pnpm test --roots '<rootDir>/src'\",\n  \"lintCommand\": \"pnpm lint --ignore-pattern .claude/\",\n  \"jev\": {\"key\": \"synthetic-Jev-key-719235\"}\n}\n";

#[test]
fn private_host_environment_keys_are_masked_without_changing_the_file() {
    let dir=tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".claude")).unwrap();
    std::fs::write(dir.path().join("mustard.json"),"{}").unwrap();
    let private=json!({"env":{"TYPESAFE_API_KEY":SECRET},"rtk":true}).to_string();
    let file=".claude/settings.local.json";
    std::fs::write(dir.path().join(file),&private).unwrap();
    let command=masked_command(dir.path(),"cat .claude/settings.local.json; echo finished");
    assert!(command.ends_with("; echo finished"), "{command}");
    let output=native(dir.path(),file,"cat",json!([]));
    assert!(output.status.success());
    let shown=String::from_utf8(output.stdout).unwrap();
    assert!(!shown.contains(SECRET));
    assert_eq!(serde_json::from_str::<Value>(&shown).unwrap()["env"]["TYPESAFE_API_KEY"],"***");
    assert_eq!(std::fs::read_to_string(dir.path().join(file)).unwrap(),private);
}

/// The hook's answer for `command`; a hook that lets it pass in silence
/// answers `null`.
fn hook(root: &Path, command: &str) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt")).args(["on", "PreToolUse"])
        .current_dir(root).env_remove("CLAUDE_PROJECT_DIR").env_remove("MUSTARD_WORKSPACE_ROOT")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let request = json!({"hook_event_name":"PreToolUse","tool_name":"Bash","cwd":root,
        "session_id":"config-read-regression","tool_input":{"command":command,"timeout":3500,"description":"config and git"}});
    child.stdin.take().unwrap().write_all(request.to_string().as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    if output.stdout.is_empty() {
        return Value::Null;
    }
    serde_json::from_slice(&output.stdout).unwrap()
}

/// The command the refusal of `command` hands back, alone on the last line of
/// the reason. The refusal changes no command and never carries the key: a
/// refusal beats any other hook that would rewrite the same command.
fn masked_command(root: &Path, command: &str) -> String {
    let result = hook(root, command);
    let specific = &result["hookSpecificOutput"];
    assert_eq!(specific["permissionDecision"], "deny", "{command}: {result}");
    assert!(specific.get("updatedInput").is_none(), "{command}: {result}");
    assert!(!result.to_string().contains(SECRET), "{command}: {result}");
    let reason = specific["permissionDecisionReason"].as_str().unwrap();
    let ready = reason.lines().last().unwrap();
    assert!(ready.contains(" run config-read --file "), "{command}: {reason}");
    ready.to_string()
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
    assert!(result["hookSpecificOutput"].get("additionalContext").is_none(), "no configuration dump: {result}");
    // The hook itself executed no part of the requested command.
    assert!(!result.to_string().contains("?? untracked.txt"));
    let command = masked_command(dir.path(), original);
    assert!(command.ends_with(r#" | cut -c1-200; echo "--- git"; git status --short | head -5; echo "fim""#), "{command}");
    let out = shell(dir.path(), &command);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("2:  \"git\"") && text.contains("3:  \"testCommand\"") && text.contains("4:  \"lintCommand\""), "{text}");
    assert!(text.contains("?? untracked.txt") && text.contains("--- git") && text.contains("fim"), "{text}");
    assert!(!text.contains("jev") && !text.contains(SECRET), "{text}");
    assert_eq!(std::fs::read_to_string(dir.path().join("mustard.json")).unwrap(), CONFIG);
    let command = masked_command(dir.path(), "grep -q flow mustard.json && printf tested >> run.count");
    assert!(!dir.path().join("run.count").exists(), "hook must never run the suffix");
    let out = shell(dir.path(), &command);
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
        let out = shell(dir.path(), &masked_command(dir.path(), original));
        assert!(out.status.success(), "{original}: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8(out.stdout).unwrap(), expected, "{original}");
    }
    assert_eq!(shell(dir.path(), &masked_command(dir.path(), "grep absent mustard.json")).status.code(), Some(1));
    let command = masked_command(dir.path(), "cat mustard.json");
    std::fs::write(dir.path().join("mustard.json"), r#"{"jev":{"key":"changed-secret-81"},"new":true}"#).unwrap();
    let out = shell(dir.path(), &command);
    assert_eq!(String::from_utf8(out.stdout).unwrap(), r#"{"jev":{"key":"***"},"new":true}"#);
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
        assert!(!specific["permissionDecisionReason"].as_str().unwrap().contains("--reader"), "no masked command: {result}");
        assert!(!result.to_string().contains(SECRET) && !result.to_string().contains("testCommand"), "{result}");
        assert!(!dir.path().join("wrong").exists());
    }
    let out = native(dir.path(), "mustard.json", "awk", json!(["{system(\"touch wrong\")}"]));
    assert!(!out.status.success() && out.stdout.is_empty());
    assert!(!dir.path().join("wrong").exists());
    let missing = native(dir.path(), "missing/mustard.json", "cat", json!([]));
    assert!(!missing.status.success() && missing.stdout.is_empty());
}

/// The read the session reported: the plain read, the same read after
/// entering the project folder, and a cut read. Each is refused; the command
/// the refusal hands back passes the hook and shows the file with `***`.
#[test]
fn a_config_read_is_refused_with_the_masked_command_ready() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::write(project.join(".claude/mustard.json"), CONFIG).unwrap();
    std::fs::write(project.join("mustard.json"), CONFIG).unwrap();
    let entering = format!("cd {} && cat .claude/mustard.json", project.display());
    for (at, original) in [(project.as_path(), "cat .claude/mustard.json"), (dir.path(), entering.as_str()), (project.as_path(), "head -5 mustard.json")] {
        let ready = masked_command(at, original);
        let again = hook(at, &ready);
        assert_ne!(again["hookSpecificOutput"]["permissionDecision"], "deny", "{original}: {again}");
        let out = shell(at, &ready);
        assert!(out.status.success(), "{original}: {}", String::from_utf8_lossy(&out.stderr));
        let shown = String::from_utf8(out.stdout).unwrap();
        assert!(shown.contains("\"jev\": {\"key\": \"***\"}") && !shown.contains(SECRET), "{original}: {shown}");
    }
}

/// The rtk readers show the file like `cat`: the hook refuses them too, with
/// no rewrite and no key.
#[test]
fn rtk_readers_of_the_config_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".claude")).unwrap();
    std::fs::write(dir.path().join(".claude/mustard.json"), CONFIG).unwrap();
    std::fs::write(dir.path().join("mustard.json"), CONFIG).unwrap();
    for original in ["rtk read .claude/mustard.json", "rtk read mustard.json --max-lines 5", "rtk json mustard.json"] {
        let result = hook(dir.path(), original);
        let specific = &result["hookSpecificOutput"];
        assert_eq!(specific["permissionDecision"], "deny", "{original}: {result}");
        assert!(specific.get("updatedInput").is_none(), "{original}: {result}");
        assert!(!result.to_string().contains(SECRET), "{original}: {result}");
    }
}
