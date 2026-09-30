// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Um gancho que falha por dentro não derruba a sessão.
//!
//! O Claude Code chama `mustard-rt on <evento>` a cada mensagem, ferramenta e
//! resposta. Se o gancho sai com código diferente de zero, entra em pânico ou
//! barra por um erro que é dele, e não do usuário, a sessão trava num projeto
//! que só quis trabalhar. O contrato tem três partes, e este arquivo prova as
//! três pelo binário, do jeito que o Claude Code o chama:
//!
//! - **Entrada ruim ou estado quebrado.** Entrada que não é JSON, vazia ou de
//!   outra forma, e um projeto cujo `mustard.json`, cuja spec ou cuja pasta
//!   `.claude` estão quebrados, terminam em saída 0 e numa resposta que deixa a
//!   sessão seguir: vazia, ou um JSON que não barra nada.
//! - **Barrar é o JSON.** O gancho que barra de verdade, também num projeto
//!   quebrado, diz `deny` no JSON e sai com 0. O código de saída nunca é o
//!   veredito.
//! - **O comando `run` não lê o stdin.** Um comando que esperasse um JSON que
//!   nunca vem deixaria a chamada pendurada; o teste segura o stdin aberto e
//!   confere que o comando termina sozinho.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Os eventos que o Claude Code chama, mais um que o Mustard não conhece.
const EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PreCompact",
    "SubagentStart",
    "SubagentStop",
    "Stop",
    "SessionEnd",
    "Notification",
    "EventoQueAindaNaoExiste",
];

/// O que o gancho respondeu.
struct Fired {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Roda `mustard-rt on <event>` em `cwd` com `stdin`, numa pasta pessoal falsa
/// e sem a variável de uma spec ativa.
fn fire(cwd: &Path, home: &Path, event: &str, stdin: &[u8]) -> Fired {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["on", event])
        .current_dir(cwd)
        .env("CLAUDE_PROJECT_DIR", cwd)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("MUSTARD_WORKSPACE_ROOT")
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .env_remove("MUSTARD_SESSION_ID")
        .env_remove("CLAUDE_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mustard-rt");
    if let Some(mut pipe) = child.stdin.take() {
        let _ = pipe.write_all(stdin);
    }
    let out = child.wait_with_output().expect("wait mustard-rt");
    Fired {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// A sessão segue: saída 0 e uma resposta vazia ou um JSON que não barra. O
/// pânico do Rust sai com 101, então um gancho que entrou em pânico cai aqui.
fn assert_session_goes_on(fired: &Fired, what: &str) {
    assert_eq!(fired.code, Some(0), "{what}: um gancho sempre sai com 0\nstderr: {}", fired.stderr);
    assert!(!fired.stderr.contains("panicked"), "{what}: o gancho entrou em pânico\n{}", fired.stderr);
    let text = fired.stdout.trim();
    if text.is_empty() {
        return;
    }
    let answer: Value = serde_json::from_str(text).unwrap_or_else(|e| panic!("{what}: a resposta não é um JSON ({e}): {text}"));
    assert_ne!(
        answer.pointer("/hookSpecificOutput/permissionDecision").and_then(Value::as_str),
        Some("deny"),
        "{what}: o gancho barrou por um erro que não é do usuário: {text}"
    );
    assert_ne!(answer.get("decision").and_then(Value::as_str), Some("block"), "{what}: o gancho barrou a resposta: {text}");
}

/// Um repositório git na branch `feature/epic`, para o checkout estar na spec
/// que o teste estraga.
fn git_project(dir: &Path) -> PathBuf {
    let root = dir.join("project");
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.email", "t@example.com"],
        &["config", "user.name", "t"],
        &["checkout", "-q", "-b", "feature/epic"],
    ] {
        let ok = Command::new("git").args(args).current_dir(&root).status().unwrap().success();
        assert!(ok, "git {args:?}");
    }
    root
}

/// A chamada de cada evento como o Claude Code a monta, uma por ferramenta que
/// tem gancho. Nenhuma é uma ação que o Mustard barre de propósito.
fn realistic_calls(root: &Path) -> Vec<(&'static str, Value)> {
    let cwd = root.display().to_string();
    let notes = root.join("notas.txt").display().to_string();
    let base = |event: &str| json!({ "hook_event_name": event, "session_id": "s1", "cwd": cwd });
    let with = |event: &str, extra: Value| {
        let mut call = base(event);
        call.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        call
    };
    vec![
        ("SessionStart", with("SessionStart", json!({ "source": "startup" }))),
        ("UserPromptSubmit", with("UserPromptSubmit", json!({ "prompt": "conserta o botão da tela inicial" }))),
        ("PreToolUse", with("PreToolUse", json!({ "tool_name": "Bash", "tool_input": { "command": "ls" } }))),
        ("PreToolUse", with("PreToolUse", json!({ "tool_name": "Write", "tool_input": { "file_path": notes, "content": "oi" } }))),
        ("PreToolUse", with("PreToolUse", json!({ "tool_name": "Read", "tool_input": { "file_path": notes } }))),
        ("PreToolUse", with("PreToolUse", json!({ "tool_name": "Task", "tool_input": { "prompt": "explore a pasta", "subagent_type": "Explore" } }))),
        (
            "PostToolUse",
            with(
                "PostToolUse",
                json!({
                    "tool_name": "AskUserQuestion",
                    "tool_input": { "questions": [] },
                    "tool_response": { "answers": { "Aprovar o plano?": "Aprovar" } }
                }),
            ),
        ),
        ("PostToolUse", with("PostToolUse", json!({ "tool_name": "Edit", "tool_input": { "file_path": notes }, "tool_response": {} }))),
        ("PostToolUse", with("PostToolUse", json!({ "tool_name": "Bash", "tool_input": { "command": "ls" }, "tool_response": {} }))),
        ("PreCompact", with("PreCompact", json!({ "trigger": "auto" }))),
        ("SubagentStart", with("SubagentStart", json!({ "agent_id": "a1", "agent_type": "Explore" }))),
        ("SubagentStop", with("SubagentStop", json!({ "agent_id": "a1", "last_assistant_message": "feito" }))),
        ("Stop", with("Stop", json!({ "last_assistant_message": "Pronto, o botão foi consertado." }))),
        ("SessionEnd", with("SessionEnd", json!({ "reason": "clear" }))),
        ("Notification", with("Notification", json!({ "message": "esperando" }))),
    ]
}

/// Entrada que o Claude Code nunca mandaria, ou mandaria torta, para cada
/// evento: o gancho a trata como entrada vazia e deixa passar.
#[test]
fn a_hook_given_input_it_cannot_read_lets_the_session_go_on() {
    let tmp = tempfile::tempdir().unwrap();
    let root = git_project(tmp.path());
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let hostile: Vec<(&str, Vec<u8>)> = vec![
        ("stdin vazio", Vec::new()),
        ("texto que não é JSON", b"isto { nao e json".to_vec()),
        ("JSON que não é um objeto", b"[1, 2, 3]".to_vec()),
        ("JSON nulo", b"null".to_vec()),
        ("campos do tipo errado", br#"{"session_id": 7, "tool_name": ["Bash"], "tool_input": "ls", "cwd": 3}"#.to_vec()),
        ("bytes que não são texto", vec![0xff, 0xfe, 0x00, 0xc3, 0x28]),
        ("entrada enorme", vec![b'a'; 300_000]),
    ];
    for event in EVENTS {
        for (what, stdin) in &hostile {
            let fired = fire(&root, &home, event, stdin);
            assert_session_goes_on(&fired, &format!("{event} com {what}"));
        }
    }
}

/// O projeto quebrado por dentro: o `mustard.json` que não é JSON, o arquivo
/// de eventos da spec da branch cheio de lixo, e, em outro projeto, a pasta
/// `.claude` que é um arquivo, onde nada se lê nem se grava. Os ganchos que
/// leem e gravam ali falham, e a sessão segue.
#[test]
fn a_hook_that_fails_inside_a_broken_project_lets_the_session_go_on() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    let corrupt = git_project(&tmp.path().join("corrupt"));
    std::fs::write(corrupt.join("mustard.json"), "{ isto nao e json").unwrap();
    let spec = corrupt.join(".claude").join("spec").join("epic");
    std::fs::create_dir_all(&spec).unwrap();
    std::fs::write(spec.join("spec.ndjson"), b"{\"v\":1,\"id\":1,\"type\":\n\xff\xfe nada\n{}\n[]\n").unwrap();
    std::fs::write(spec.join("meta.json"), "nao e json").unwrap();
    let session = corrupt.join(".claude").join(".session").join("s1");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::write(session.join("active-spec"), "epic").unwrap();

    let no_claude_dir = git_project(&tmp.path().join("no-claude-dir"));
    std::fs::write(no_claude_dir.join("mustard.json"), "{}").unwrap();
    std::fs::write(no_claude_dir.join(".claude"), "isto e um arquivo, nao uma pasta").unwrap();

    for (name, root) in [("mustard.json e spec quebrados", &corrupt), (".claude que é um arquivo", &no_claude_dir)] {
        for (event, call) in realistic_calls(root) {
            let tool = call.get("tool_name").and_then(Value::as_str).unwrap_or("-");
            // Numa spec sem estado que se leia, a aprovação não se confirma, e
            // o portão de escrita barra código de propósito: isso é o
            // veredito do portão, e não um erro dele.
            if *root == corrupt && tool == "Write" {
                continue;
            }
            let fired = fire(root, &home, event, call.to_string().as_bytes());
            assert_session_goes_on(&fired, &format!("{event} ({tool}) com {name}"));
        }
    }
}

/// O gancho que barra de verdade continua barrando num projeto quebrado, e o
/// veredito é o JSON: a saída é 0 do mesmo jeito. Sem isto, o teste acima
/// passaria também com um gancho que nunca barra nada.
#[test]
fn a_real_block_is_the_json_and_the_exit_code_stays_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let root = git_project(tmp.path());
    std::fs::write(root.join("mustard.json"), "{ isto nao e json").unwrap();
    let call = json!({
        "hook_event_name": "PreToolUse",
        "session_id": "s1",
        "cwd": root.display().to_string(),
        "tool_name": "Bash",
        "tool_input": { "command": "rm -rf ./pasta-que-o-gancho-nunca-deixa-apagar" }
    });
    let fired = fire(&root, &home, "PreToolUse", call.to_string().as_bytes());
    assert_eq!(fired.code, Some(0), "barrar é o JSON, nunca o código de saída\nstderr: {}", fired.stderr);
    let answer: Value = serde_json::from_str(fired.stdout.trim()).unwrap_or_else(|e| panic!("sem JSON ({e}): {}", fired.stdout));
    assert_eq!(answer.pointer("/hookSpecificOutput/permissionDecision").and_then(Value::as_str), Some("deny"), "{answer}");
    assert_eq!(answer.pointer("/hookSpecificOutput/hookEventName").and_then(Value::as_str), Some("PreToolUse"), "{answer}");
}

/// Espera o filho terminar por até `limit`; devolve `None` se ele segue vivo.
fn wait_up_to(child: &mut Child, limit: Duration) -> Option<std::process::ExitStatus> {
    let started = Instant::now();
    while started.elapsed() < limit {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

/// Um comando `run` não lê o stdin: com o stdin aberto e sem nenhum byte, ele
/// termina sozinho. O `on` é que lê o JSON do evento.
#[test]
fn a_run_command_never_waits_for_the_stdin() {
    let tmp = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "read", "state"])
        .arg("--root")
        .arg(tmp.path())
        .current_dir(tmp.path())
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mustard-rt");
    // O cano fica aberto até o fim do teste: só um comando que espera o stdin
    // ficaria preso.
    let _open_stdin = child.stdin.take();
    let finished = wait_up_to(&mut child, Duration::from_secs(20));
    if finished.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    assert!(finished.is_some(), "o comando `run` ficou esperando o stdin");
}
