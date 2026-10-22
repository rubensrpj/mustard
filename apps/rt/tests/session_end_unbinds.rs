// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A conversa que acaba, por qualquer motivo, solta a ligação com a spec.
//!
//! A sessão A abre a spec e fala; ela termina sem `/clear` — o usuário saiu,
//! a janela fechou. A sessão B retoma a spec e grava: a gravação não avisa
//! que outra conversa está na obra, porque a sessão A já não está ligada a
//! ela. A mesma sessão A, retomada pelo mesmo id, volta sem ligação e o início
//! dela manda rodar o `resume`. O gancho e o comando sem sessão nenhuma não
//! decidem spec pela branch. Tudo pelo binário, com o JSON dos ganchos e os
//! comandos do uso real.

#[path = "support/binary.rs"]
mod binary;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const SPEC: &str = "fim";
const FIRST: &str = "s-primeira";
const SECOND: &str = "s-segunda";

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git").args(args).current_dir(root).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// O repositório de teste parado em `dev`, com o Mustard fora do git.
fn project() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("projeto");
    let home = dir.path().join("casa");
    std::fs::create_dir_all(&root).expect("root");
    std::fs::create_dir_all(&home).expect("home");
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "t@example.com"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    git(&root, &["checkout", "-q", "-b", "main"]);
    std::fs::write(root.join(".git/info/exclude"), ".claude/\nmustard.json\n").expect("exclude");
    std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"},"git":{"flow":{"*":"dev","dev":"main"}}}"#)
        .expect("config");
    std::fs::write(root.join("README.md"), "oi\n").expect("readme");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "init"]);
    git(&root, &["checkout", "-q", "-b", "dev"]);
    (dir, root, home)
}

/// O binário com `args`, no projeto, com a sessão `session` no ambiente, como
/// o gancho do terminal a passa ao comando numa conversa. A saída padrão.
fn binary(root: &Path, home: &Path, session: Option<&str>, args: &[&str], stdin: &str) -> String {
    let mut child = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(args)
        .current_dir(root)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("CLAUDE_PROJECT_DIR", root)
        .env("MUSTARD_CLAUDE_BIN", home.join("sem-claude"))
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .envs(session.map(|session| ("MUSTARD_SESSION_ID", session)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the binary runs");
    if let Some(mut pipe) = child.stdin.take() {
        let _ = pipe.write_all(stdin.as_bytes());
    }
    String::from_utf8_lossy(&child.wait_with_output().expect("the binary finishes").stdout).to_string()
}

/// O gancho `event` com o JSON que o Claude Code entrega. Devolve o que ele
/// disse ao assistente, ou vazio.
fn hook(root: &Path, home: &Path, event: &str, payload: &Value) -> String {
    let out = binary(root, home, None, &["on", event], &payload.to_string());
    serde_json::from_str::<Value>(out.trim())
        .ok()
        .and_then(|v| v.pointer("/hookSpecificOutput/additionalContext").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default()
}

/// Um comando `run` da conversa da sessão `session`, que responde JSON.
fn run(root: &Path, home: &Path, session: &str, args: &[&str]) -> Value {
    let mut all = vec!["run"];
    all.extend_from_slice(args);
    let out = binary(root, home, Some(session), &all, "");
    serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("{args:?} did not answer JSON ({e}): {out}"))
}

/// A sessão que terminou sem `/clear` não fica ligada à spec: a sessão que
/// a retoma grava sem o aviso de outra conversa na obra.
#[test]
fn a_session_ended_without_clear_no_longer_counts_as_another_conversation() {
    let (_dir, root, home) = project();
    let cwd = root.to_string_lossy().to_string();
    let opened = run(&root, &home, FIRST, &["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    assert_eq!(opened["ok"], json!(true), "{opened}");
    hook(&root, &home, "UserPromptSubmit",
        &json!({"hook_event_name": "UserPromptSubmit", "prompt": "quero que a leitura custe menos", "session_id": FIRST, "cwd": cwd}));
    hook(&root, &home, "SessionEnd", &json!({"hook_event_name": "SessionEnd", "reason": "logout", "session_id": FIRST, "cwd": cwd}));

    let resumed = run(&root, &home, SECOND, &["resume", "--spec", SPEC]);
    assert_eq!(resumed["ok"], json!(true), "{resumed}");
    hook(&root, &home, "UserPromptSubmit",
        &json!({"hook_event_name": "UserPromptSubmit", "prompt": "Reduzir o custo da leitura do mapa.", "session_id": SECOND, "cwd": cwd}));
    let events = std::fs::read_to_string(root.join(".claude/spec").join(SPEC).join("spec.ndjson")).expect("the spec file");
    let said = events
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .rfind(|event| event["type"] == "message" && event["session"] == SECOND)
        .expect("the resumed conversation records its message")["id"]
        .clone();
    let goal = json!({"title": "Leitura", "agent": "- conferir a leitura", "text": "Reduzir o custo da leitura do mapa.", "origin": said});
    let written = run(&root, &home, SECOND, &["write", "context", "--spec", SPEC, "--json", &goal.to_string()]);

    assert_eq!(written["ok"], json!(true), "{written}");
    assert!(!written.to_string().contains(FIRST), "the ended session is not another conversation: {written}");
}

/// O arquivo de eventos da spec [`SPEC`], linha a linha.
fn spec_events(root: &Path) -> Vec<String> {
    let file = root.join(".claude/spec").join(SPEC).join("spec.ndjson");
    std::fs::read_to_string(file).expect("the spec file").lines().map(str::to_string).collect()
}

/// O arquivo de operações da spec [`SPEC`], onde moram as chamadas; vazio
/// enquanto nenhuma foi gravada.
fn spec_operations(root: &Path) -> String {
    std::fs::read_to_string(root.join(".claude/mustard/operations").join(SPEC).with_extension("ndjson")).unwrap_or_default()
}

/// O gancho e o comando que chegam sem sessão nenhuma não decidem a spec pela
/// branch: a fala não entra na spec do checkout e a chamada não é gravada nas
/// operações dela. A conversa ligada, no mesmo projeto, grava as duas.
#[test]
fn a_hook_and_a_command_without_a_session_decide_no_spec_by_the_branch() {
    let (_dir, root, home) = project();
    let cwd = root.to_string_lossy().to_string();
    let opened = run(&root, &home, FIRST, &["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    assert_eq!(opened["ok"], json!(true), "{opened}");
    let events = spec_events(&root);
    let operations = spec_operations(&root);

    hook(&root, &home, "UserPromptSubmit",
        &json!({"hook_event_name": "UserPromptSubmit", "prompt": "uma fala sem sessão nenhuma", "cwd": cwd}));
    binary(&root, &home, None, &["run", "find", "leitura"], "");

    assert_eq!(spec_events(&root), events, "the message of a hook without a session stays out of the branch spec");
    assert_eq!(spec_operations(&root), operations, "the call of a command without a session stays out of the branch spec");

    hook(&root, &home, "UserPromptSubmit",
        &json!({"hook_event_name": "UserPromptSubmit", "prompt": "uma fala da conversa ligada", "session_id": FIRST, "cwd": cwd}));
    binary(&root, &home, Some(FIRST), &["run", "find", "leitura"], "");

    assert_eq!(spec_events(&root).len(), events.len() + 1, "the bound conversation records its message");
    assert_ne!(spec_operations(&root), operations, "the bound conversation records its call");
}

/// A conversa que acabou e volta pelo mesmo id (`source: resume`) está sem
/// ligação: o início dela manda rodar o `resume`, e depois de rodá-lo o aviso
/// some.
#[test]
fn a_resumed_conversation_without_the_binding_is_told_to_run_resume() {
    let (_dir, root, home) = project();
    let cwd = root.to_string_lossy().to_string();
    let opened = run(&root, &home, FIRST, &["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    assert_eq!(opened["ok"], json!(true), "{opened}");
    let start = json!({"hook_event_name": "SessionStart", "source": "resume", "session_id": FIRST, "cwd": cwd});

    let bound = hook(&root, &home, "SessionStart", &start);
    assert!(bound.contains("Retomada: spec fim"), "the bound conversation still gets the resume line:\n{bound}");
    assert!(!bound.contains("não está ligada"), "a bound conversation is not told to bind:\n{bound}");

    hook(&root, &home, "SessionEnd", &json!({"hook_event_name": "SessionEnd", "reason": "logout", "session_id": FIRST, "cwd": cwd}));
    let unbound = hook(&root, &home, "SessionStart", &start);
    assert!(unbound.contains("Retomada: spec fim"), "{unbound}");
    assert!(unbound.contains("não está ligada"), "the unbound conversation is told so:\n{unbound}");
    assert!(unbound.contains("mustard-rt run resume"), "and told the command that binds it:\n{unbound}");

    let resumed = run(&root, &home, FIRST, &["resume"]);
    assert_eq!(resumed["ok"], json!(true), "{resumed}");
    let again = hook(&root, &home, "SessionStart", &start);
    assert!(!again.contains("não está ligada"), "after the resume the notice is gone:\n{again}");
}
