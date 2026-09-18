// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! As páginas publicadas chegam ao projeto pela instalação.
//!
//! Cada página do Mustard nasce de um template publicado uma vez no claude.ai,
//! e o que muda depois vai para o banco de dados da página, pela ferramenta
//! `ArtifactData`. Dois defeitos param isso sem ninguém ver: a ferramenta
//! pedindo o sim da pessoa a cada marco, porque a atualização só levava as
//! liberações dos comandos do Mustard; e a página do projeto que só nascia
//! com a primeira spec. Os testes rodam o `upsert`, o gancho do início da
//! sessão, o `run write` e a barra de status como a pessoa roda, num
//! repositório temporário com uma pasta pessoal falsa.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::platform::page_templates::{project_page_template, spec_page_template, PROJECT_CAPABILITIES};
use mustard_core::platform::project_seed::{project_page_template_path, PAGE_DATABASE_TOOL};
use serde_json::{json, Value};

/// As liberações que a pessoa escreveu por conta própria.
const OWN_ALLOW: [&str; 3] = ["Bash(npm test:*)", "WebFetch(domain:example.com)", "mcp__github__get_issue"];

/// As perguntas que a pessoa escreveu por conta própria.
const OWN_ASK: [&str; 1] = ["Bash(git push:*)"];

/// Os bloqueios que a pessoa escreveu por conta própria.
const OWN_DENY: [&str; 1] = ["Bash(curl:*)"];

/// Roda o binário no projeto, com uma pasta pessoal falsa e sem `claude` à mão.
fn rt(root: &Path, home: &Path, args: &[&str], stdin: &str) -> Output {
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
    child.wait_with_output().expect("the binary finishes")
}

/// O `rt` que tem de dar certo, com a saída.
fn rt_ok(root: &Path, home: &Path, args: &[&str], stdin: &str) -> String {
    let out = rt(root, home, args, stdin);
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Um repositório git novo, ainda sem o Mustard, e a pasta pessoal falsa.
fn fresh_repo(dir: &Path) -> (PathBuf, PathBuf) {
    let root = dir.join("project");
    let home = dir.join("home");
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let ok = Command::new("git").args(["init", "-q"]).current_dir(&root).status().unwrap().success();
    assert!(ok, "git init");
    (root, home)
}

/// As configurações locais do projeto, como estão no disco.
fn local_settings(root: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(root.join(".claude/settings.local.json")).unwrap()).unwrap()
}

/// A lista `list` das liberações das configurações locais.
fn rules(settings: &Value, list: &str) -> Vec<String> {
    settings["permissions"][list]
        .as_array()
        .unwrap_or_else(|| panic!("no permissions.{list}: {settings}"))
        .iter()
        .map(|rule| rule.as_str().unwrap().to_string())
        .collect()
}

/// Confere o que a instalação deixou no projeto em `root`, que antes tinha as
/// listas `allow`, `ask` e `deny`: a ferramenta do banco de dados liberada uma
/// vez só; as liberações, perguntas e bloqueios da pessoa iguais, na mesma
/// ordem, com só as regras do próprio Mustard acrescentadas depois delas; e
/// os dois templates em `.claude/mustard/pages`, no idioma `text`.
fn assert_installed(root: &Path, allow: &[&str], ask: &[&str], deny: &[&str], text: Locale, case: &str) {
    let settings = local_settings(root);
    let now = rules(&settings, "allow");
    assert_eq!(
        now.iter().filter(|rule| *rule == PAGE_DATABASE_TOOL).count(),
        1,
        "{case}: the page database tool is allowed once: {now:?}",
    );
    assert_eq!(now[..allow.len()], *allow, "{case}: the person's allow rules changed: {now:?}");
    for added in &now[allow.len()..] {
        assert!(
            added.starts_with("Bash(mustard-rt run ") || added == PAGE_DATABASE_TOOL,
            "{case}: `{added}` is not one of Mustard's own rules: {now:?}",
        );
    }
    assert_eq!(rules(&settings, "ask"), ask, "{case}: the person's ask rules changed");
    let denied = rules(&settings, "deny");
    assert_eq!(denied[..deny.len()], *deny, "{case}: the person's deny rules changed: {denied:?}");
    assert!(!denied.iter().any(|rule| rule == PAGE_DATABASE_TOOL), "{case}: {denied:?}");

    let pages = root.join(".claude/mustard/pages");
    assert_eq!(
        std::fs::read_to_string(pages.join("spec.html")).ok(),
        Some(spec_page_template(text)),
        "{case}: the spec page template is not installed in the text language",
    );
    assert_eq!(
        std::fs::read_to_string(pages.join("project.html")).ok(),
        Some(project_page_template(text)),
        "{case}: the project page template is not installed in the text language",
    );
}

/// A instalação nova e a atualização liberam a ferramenta do banco de dados
/// das páginas e instalam os dois templates, sem mexer nas liberações que a
/// pessoa já tinha; a segunda volta não muda nada.
#[test]
fn the_install_and_the_update_allow_the_page_database() {
    let own = || json!({ "permissions": { "allow": OWN_ALLOW, "ask": OWN_ASK, "deny": OWN_DENY } });

    // A instalação nova num projeto que já tem configurações locais próprias.
    let dir = tempfile::tempdir().unwrap();
    let (root, home) = fresh_repo(dir.path());
    std::fs::write(root.join(".claude/settings.local.json"), serde_json::to_string_pretty(&own()).unwrap()).unwrap();
    assert!(!root.join("mustard.json").exists(), "a fresh install");
    rt_ok(&root, &home, &["run", "upsert"], "");
    assert_installed(&root, &OWN_ALLOW, &OWN_ASK, &OWN_DENY, Locale::PtBr, "fresh install");
    let settled = std::fs::read_to_string(root.join(".claude/settings.local.json")).unwrap();
    rt_ok(&root, &home, &["run", "upsert"], "");
    assert_eq!(
        std::fs::read_to_string(root.join(".claude/settings.local.json")).unwrap(),
        settled,
        "a second install changes nothing",
    );

    // A atualização de um projeto instalado antes da ferramenta e dos
    // templates, em inglês, com liberações próprias no meio das do Mustard.
    let dir = tempfile::tempdir().unwrap();
    let (root, home) = fresh_repo(dir.path());
    std::fs::write(root.join("mustard.json"), r#"{"version":"0.0.1","language":{"text":"en-US"}}"#).unwrap();
    rt_ok(&root, &home, &["run", "upsert"], "");
    assert!(
        rules(&local_settings(&root), "allow").iter().any(|rule| rule == PAGE_DATABASE_TOOL),
        "the seed of a bare install carries the page database tool",
    );
    let mut old = local_settings(&root);
    let mut allow: Vec<String> =
        rules(&old, "allow").into_iter().filter(|rule| rule != PAGE_DATABASE_TOOL).collect();
    allow.splice(1..1, OWN_ALLOW.iter().map(|rule| (*rule).to_string()));
    old["permissions"]["allow"] = json!(allow);
    old["permissions"]["ask"] = json!(OWN_ASK);
    let mut deny = rules(&old, "deny");
    deny.insert(0, OWN_DENY[0].to_string());
    old["permissions"]["deny"] = json!(deny);
    std::fs::write(root.join(".claude/settings.local.json"), serde_json::to_string_pretty(&old).unwrap()).unwrap();
    std::fs::remove_dir_all(root.join(".claude/mustard/pages")).unwrap();
    let before_allow: Vec<&str> = allow.iter().map(String::as_str).collect();
    let before_deny: Vec<&str> = deny.iter().map(String::as_str).collect();

    rt_ok(&root, &home, &["run", "upsert"], "");
    assert_installed(&root, &before_allow, &OWN_ASK, &before_deny, Locale::EnUs, "update");
    let settled = std::fs::read_to_string(root.join(".claude/settings.local.json")).unwrap();
    rt_ok(&root, &home, &["run", "upsert"], "");
    assert_eq!(
        std::fs::read_to_string(root.join(".claude/settings.local.json")).unwrap(),
        settled,
        "a second update changes nothing",
    );
}

/// O texto que o início da sessão coloca na janela, com a origem dada.
fn session_start(root: &Path, home: &Path, source: &str) -> String {
    let payload = json!({
        "hook_event_name": "SessionStart",
        "source": source,
        "session_id": "s-pagina",
        "cwd": root.to_string_lossy(),
    })
    .to_string();
    let out = rt_ok(root, home, &["on", "SessionStart"], &payload);
    if out.trim().is_empty() {
        return String::new();
    }
    let answer: Value = serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("not JSON ({e}): {out}"));
    answer["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or_default().to_string()
}

/// A gravação do endereço da página do projeto, sem spec, como o aviso manda.
fn record_project_page(root: &Path, home: &Path, publish: &Value) -> Output {
    rt(root, home, &["run", "write", "publish", "--json", &publish.to_string()], "")
}

/// Num projeto sem a página do projeto publicada, o início da sessão manda
/// publicar o template dela e gravar o endereço. O aviso fica enquanto o
/// endereço não é gravado, também com o índice das specs sem endereço e
/// depois de uma publicação que falhou; gravado o endereço, ele vira o link
/// da barra de status e o início da sessão não manda mais nada.
#[test]
fn a_session_without_project_page_asks_to_publish_it() {
    let dir = tempfile::tempdir().unwrap();
    let (root, home) = fresh_repo(dir.path());
    std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).unwrap();
    rt_ok(&root, &home, &["run", "upsert"], "");

    let template = project_page_template_path();
    assert!(root.join(&template).is_file(), "the template the notice names is installed");
    let notice = translate("session.project_page", Locale::PtBr)
        .replace("{template}", &template)
        .replace("{capabilities}", PROJECT_CAPABILITIES);
    for source in ["startup", "clear"] {
        let context = session_start(&root, &home, source);
        assert!(context.contains(&notice), "`{source}` does not ask to publish the project page: {context}");
    }

    // O índice das specs sem endereço na linha do projeto: ainda não há página.
    rt_ok(&root, &home, &["run", "index"], "");
    assert!(root.join(".claude/spec/index.ndjson").is_file());
    assert!(session_start(&root, &home, "startup").contains(&notice), "an index without an address has no page");

    // A publicação que falhou não tem endereço a gravar, e o aviso fica.
    let failed = record_project_page(&root, &home, &json!({"page": "project", "ok": false, "reason": "offline"}));
    assert!(!failed.status.success(), "a failed publication without a spec is refused");
    let no_url = record_project_page(&root, &home, &json!({"page": "project", "ok": true}));
    assert!(!no_url.status.success(), "a publication without an address is refused");
    assert!(session_start(&root, &home, "startup").contains(&notice), "nothing was recorded, so it still asks");

    // Gravado o endereço, ele vira o link da barra e o aviso não volta.
    let url = "https://claude.ai/code/artifact/pagina-do-projeto";
    let recorded = record_project_page(&root, &home, &json!({"page": "project", "ok": true, "url": url}));
    assert!(recorded.status.success(), "{}", String::from_utf8_lossy(&recorded.stdout));
    let report: Value = serde_json::from_slice(&recorded.stdout).unwrap();
    assert_eq!(report, json!({"ok": true, "type": "publish", "page": "project", "url": url}));
    let bar = rt_ok(&root, &home, &["run", "statusline"], &json!({"workspace": {"current_dir": root}}).to_string());
    assert!(bar.contains(url), "the address is the status line's link: {bar}");
    for source in ["startup", "clear", "compact"] {
        let context = session_start(&root, &home, source);
        assert!(!context.contains(&template), "`{source}` still asks to publish a published page: {context}");
    }
    // O índice refeito do zero guarda o endereço gravado sem spec.
    rt_ok(&root, &home, &["run", "index"], "");
    assert!(!session_start(&root, &home, "startup").contains(&template), "the rebuilt index lost the address");
}
