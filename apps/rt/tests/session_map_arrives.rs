// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! O mapa do início da sessão chega à sessão.
//!
//! Instalar deixa o mapa no disco e declarado no `mustard.json`; o que prova
//! que ele serve é o início da sessão entregá-lo inteiro, no idioma do
//! projeto. Um texto semeado e declarado que o gancho não entrega fica no
//! disco sem chegar a ninguém, e nada avisa: é esse defeito que esta guarda
//! pega, tanto numa instalação nova quanto numa que ainda declarava as três
//! partes do roteador antigo.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mustard_core::platform::i18n::Locale;
use serde_json::{json, Value};

/// Roda o binário no projeto, com uma pasta pessoal falsa e sem `claude` à mão.
fn rt(root: &Path, home: &Path, args: &[&str], stdin: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(args)
        .current_dir(root)
        .env("CLAUDE_PROJECT_DIR", root)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CLAUDE_PLUGIN_ROOT")
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .env("MUSTARD_CLAUDE_BIN", home.join("no-claude-here"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    if let Some(mut pipe) = child.stdin.take() {
        let _ = pipe.write_all(stdin.as_bytes());
    }
    let out = child.wait_with_output().expect("the binary finishes");
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Um repositório git com o `mustard.json` dado, instalado pelo `upsert`.
fn installed(dir: &Path, config: &str) -> (PathBuf, PathBuf) {
    let root = dir.join("project");
    let home = dir.join("home");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let ok = Command::new("git").args(["init", "-q"]).current_dir(&root).status().unwrap().success();
    assert!(ok, "git init");
    std::fs::write(root.join("mustard.json"), config).unwrap();
    rt(&root, &home, &["run", "upsert"], "");
    (root, home)
}

/// O texto que o início da sessão coloca na janela, com a origem dada
/// (`startup`, `clear`, `compact`, `resume`).
fn session_start(root: &Path, home: &Path, source: &str) -> String {
    let payload = json!({
        "hook_event_name": "SessionStart",
        "source": source,
        "session_id": "s-mapa",
        "cwd": root.to_string_lossy(),
    })
    .to_string();
    let out = rt(root, home, &["on", "SessionStart"], &payload);
    if out.trim().is_empty() {
        return String::new();
    }
    let answer: Value = serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("not JSON ({e}): {out}"));
    answer["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or_default().to_string()
}

/// Numa instalação nova, o início da sessão entrega o mapa inteiro, no
/// idioma do projeto, em toda janela nova — a abertura, depois de `/clear`,
/// da compactação e da retomada.
#[test]
fn a_fresh_install_delivers_the_whole_map_at_every_session_start() {
    for (lang, text) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
        let dir = tempfile::tempdir().unwrap();
        let (root, home) = installed(dir.path(), &format!(r#"{{"version":"1.0.0","language":{{"text":"{lang}"}}}}"#));
        let map = mustard_core::session_map(text).trim();
        for source in ["startup", "clear", "compact", "resume", "startup"] {
            let context = session_start(&root, &home, source);
            assert!(
                context.contains(map),
                "the {lang} map did not reach the session on `{source}`: {context}",
            );
        }
    }
}

/// Uma instalação antiga, que declarava as três partes do roteador na
/// mensagem do usuário, sai do `upsert` recebendo o mapa no início da sessão,
/// sem as partes antigas no disco.
#[test]
fn an_old_router_install_gets_the_map_after_the_upsert() {
    let dir = tempfile::tempdir().unwrap();
    let old = r#"{"version":"0.0.1","inject":[
        {"on":"userPromptSubmit","file":".claude/mustard/orchestrator.md","once":true},
        {"on":"userPromptSubmit","file":".claude/mustard/dispatch.md","once":true},
        {"on":"userPromptSubmit","file":".claude/mustard/material.md","once":true}
    ]}"#;
    let parts = dir.path().join("project/.claude/mustard");
    std::fs::create_dir_all(&parts).unwrap();
    for name in ["orchestrator.md", "dispatch.md", "material.md"] {
        std::fs::write(parts.join(name), "# old router part\n").unwrap();
    }
    let (root, home) = installed(dir.path(), old);

    let context = session_start(&root, &home, "startup");
    assert!(
        context.contains(mustard_core::session_map(Locale::PtBr).trim()),
        "the upgraded project never gets the map: {context}",
    );
    for name in ["orchestrator.md", "dispatch.md", "material.md"] {
        assert!(!root.join(".claude/mustard").join(name).exists(), "{name} is still on disk");
    }
}
