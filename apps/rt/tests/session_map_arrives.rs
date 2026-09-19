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

/// Uma atualização num projeto instalado com o mapa de nome antigo deixa o
/// mapa com o nome novo: o arquivo `session-map.md` existe, o
/// `mapa-inicio-sessao.md` sumiu, e o `mustard.json` aponta para o nome novo,
/// em qualquer grafia com que declarava o antigo, com o resto dele igual. O
/// início da sessão entrega o mapa pelo caminho novo, o arquivo novo continua
/// fora do git do projeto, e a atualização seguinte não muda nada.
///
/// O projeto de partida é uma instalação de verdade, feita pelo `upsert`, com
/// o nome do mapa voltado ao antigo no disco, na declaração e na lista do que
/// o git não vê, como uma versão anterior deixava. A declaração de um arquivo
/// da pessoa com o mesmo nome em outra pasta é o caso vizinho, e não muda.
#[test]
fn an_update_renames_the_session_map_and_its_declaration() {
    const OLD: &str = ".claude/mustard/mapa-inicio-sessao.md";
    const NEW: &str = ".claude/mustard/session-map.md";
    const NEIGHBOUR: &str = "docs/mapa-inicio-sessao.md";
    for (spelling, on, once) in [
        (OLD, "sessionStart", false),
        // Prova que a troca preserva o evento e o `once`, e não os reseta ao
        // padrão: só o campo `file` muda, o resto da declaração continua
        // igual, mesmo quando a declaração antiga não entrega nada hoje
        // (`userPromptSubmit` nunca entrega injetável).
        ("./.claude/mustard/mapa-inicio-sessao.md", "userPromptSubmit", true),
        (".claude\\mustard\\mapa-inicio-sessao.md", "sessionStart", false),
        (".claude/mustard/Mapa-Inicio-Sessao.md", "sessionStart", false),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (root, home) = installed(dir.path(), r#"{"version":"1.0.0","buildCommand":"make","customKey":{"kept":true}}"#);

        // A instalação de antes da troca do nome.
        let map = mustard_core::session_map(Locale::PtBr);
        std::fs::remove_file(root.join(NEW)).unwrap();
        std::fs::write(root.join(OLD), map).unwrap();
        let exclude = root.join(".git/info/exclude");
        let rules = std::fs::read_to_string(&exclude).unwrap();
        assert!(rules.contains(NEW), "the install hid the map: {rules}");
        std::fs::write(&exclude, rules.replace("session-map.md", "mapa-inicio-sessao.md")).unwrap();
        let mut config: Value = serde_json::from_str(&std::fs::read_to_string(root.join("mustard.json")).unwrap()).unwrap();
        config["inject"] = json!([
            {"on": on, "file": spelling, "once": once},
            {"on": "sessionStart", "file": NEIGHBOUR, "once": true},
        ]);
        std::fs::write(root.join("mustard.json"), serde_json::to_string_pretty(&config).unwrap()).unwrap();

        // O vizinho é um arquivo de verdade da pessoa, não só uma declaração:
        // a troca de nome mexe só em `mapa-inicio-sessao.md`, e o vizinho de
        // mesmo nome numa pasta diferente fica no disco, intocado.
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join(NEIGHBOUR), "# a pagina da pessoa\n").unwrap();
        Command::new("git").args(["add", "docs"]).current_dir(&root).status().unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", "docs"])
            .current_dir(&root)
            .status()
            .unwrap();

        let report: Value = serde_json::from_str(rt(&root, &home, &["run", "upsert"], "").trim()).unwrap();

        assert_eq!(std::fs::read_to_string(root.join(NEW)).unwrap(), map, "`{spelling}`: the new map");
        let mut left: Vec<String> = std::fs::read_dir(root.join(".claude/mustard"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        // Ao lado do mapa, só a pasta dos templates das páginas.
        assert_eq!(left, ["pages", "session-map.md"], "`{spelling}`: the old map is still on disk");
        assert_eq!(
            std::fs::read_to_string(root.join(NEIGHBOUR)).unwrap(),
            "# a pagina da pessoa\n",
            "`{spelling}`: the neighbour file was touched",
        );
        let mut expected = config.clone();
        expected["inject"][0]["file"] = json!(NEW);
        let after: Value = serde_json::from_str(&std::fs::read_to_string(root.join("mustard.json")).unwrap()).unwrap();
        assert_eq!(after, expected, "`{spelling}`: only the old path changes in mustard.json");
        assert_eq!(
            report["migrated"],
            json!(["session map (mapa-inicio-sessao.md → session-map.md)"]),
            "`{spelling}`: {report}",
        );

        let context = session_start(&root, &home, "startup");
        if on == "sessionStart" {
            assert!(context.contains(map.trim()), "`{spelling}`: the session start misses the map: {context}");
        } else {
            // O evento continua o que a pessoa escreveu: `userPromptSubmit`
            // nunca entrega injetável nenhum, antes ou depois da troca.
            assert!(!context.contains(map.trim()), "`{spelling}`: the map reached a `{on}` declaration: {context}");
        }
        let status = Command::new("git")
            .args(["status", "--porcelain", "--untracked-files=all"])
            .current_dir(&root)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&status.stdout), "", "`{spelling}`: the project's git sees the install");

        let bytes = std::fs::read(root.join("mustard.json")).unwrap();
        let again: Value = serde_json::from_str(rt(&root, &home, &["run", "upsert"], "").trim()).unwrap();
        assert_eq!(again["migrated"], json!([]), "`{spelling}`: the rename converges");
        assert_eq!(std::fs::read(root.join("mustard.json")).unwrap(), bytes, "`{spelling}`: a second update rewrote it");
    }
}
