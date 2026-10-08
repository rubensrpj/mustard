// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A instalação entrega acompanhamento local e preserva decisões pessoais.
//! Nenhum template ou permissão de publicação externa é criado por instalar
//! ou atualizar. Os testes rodam upsert, início de sessão, gravação histórica
//! e statusline em repositório temporário com pasta pessoal falsa.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use mustard_core::platform::i18n::Locale;
use serde_json::{json, Value};

/// As liberações que a pessoa escreveu por conta própria.
const OWN_ALLOW: [&str; 3] = ["Bash(npm test:*)", "WebFetch(domain:example.com)", "mcp__github__get_issue"];

/// As perguntas que a pessoa escreveu por conta própria.
const OWN_ASK: [&str; 1] = ["Bash(git push:*)"];

/// Os bloqueios que a pessoa escreveu por conta própria.
const OWN_DENY: [&str; 1] = ["Bash(curl:*)"];

/// Roda o binário no projeto, com uma pasta pessoal falsa e sem `claude` à mão.
fn rt(root: &Path, home: &Path, args: &[&str], stdin: &str) -> Output {
    rt_with_env(root, home, args, stdin, &[])
}

/// O `rt`, com as variáveis de ambiente `envs` a mais no processo.
fn rt_with_env(root: &Path, home: &Path, args: &[&str], stdin: &str, envs: &[(&str, &str)]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(args)
        .envs(envs.iter().copied())
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
/// listas `allow`, `ask` e `deny`: nenhuma permissão de publicação adicionada;
/// as liberações, perguntas e bloqueios da pessoa iguais, na mesma ordem,
/// com só as regras dos comandos nativos acrescentadas depois delas; e
/// os textos locais no idioma `text`, sem templates de publicação automática.
fn assert_installed(root: &Path, allow: &[&str], ask: &[&str], deny: &[&str], text: Locale, case: &str) {
    let settings = local_settings(root);
    let now = rules(&settings, "allow");
    assert_eq!(
        now.iter().filter(|rule| *rule == "ArtifactData").count(),
        allow.iter().filter(|rule| **rule == "ArtifactData").count(),
        "{case}: publication permissions are personal decisions: {now:?}",
    );
    assert_eq!(now[..allow.len()], *allow, "{case}: the person's allow rules changed: {now:?}");
    for added in &now[allow.len()..] {
        assert!(
            added.starts_with("Bash(mustard-rt run "),
            "{case}: `{added}` is not one of Mustard's own rules: {now:?}",
        );
    }
    assert_eq!(rules(&settings, "ask"), ask, "{case}: the person's ask rules changed");
    let denied = rules(&settings, "deny");
    assert_eq!(denied[..deny.len()], *deny, "{case}: the person's deny rules changed: {denied:?}");

    let pages = root.join(".claude/mustard/pages");
    assert!(!pages.join("spec.html").exists() && !pages.join("project.html").exists(), "{case}: no automatic public resources");
    let map = std::fs::read_to_string(root.join(".claude/mustard/session-map.md")).unwrap();
    assert!(map.contains("/mustard-panel") && map.contains("/mustard-publish"), "{case}: local tracking and explicit export");
    assert!(map.contains(if text == Locale::PtBr { "Mustard neste projeto" } else { "Mustard in this project" }), "{case}: text language");

}

/// Instalar e atualizar não libera ferramentas de publicação para o modelo.
/// Uma permissão pessoal já existente é preservada; repetir não muda nada.
#[test]
fn the_install_and_the_update_keep_publication_permissions_personal() {
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

    // Atualização em inglês: permissões pessoais no meio das do Mustard,
    // inclusive uma ferramenta externa que a pessoa decidiu liberar.
    let dir = tempfile::tempdir().unwrap();
    let (root, home) = fresh_repo(dir.path());
    std::fs::write(root.join("mustard.json"), r#"{"version":"0.0.1","language":{"text":"en-US"}}"#).unwrap();
    rt_ok(&root, &home, &["run", "upsert"], "");
    assert!(
        !rules(&local_settings(&root), "allow").iter().any(|rule| rule == "ArtifactData"),
        "a bare install adds no publication-tool permission",
    );
    let mut old = local_settings(&root);
    let mut allow = rules(&old, "allow");
    allow.splice(1..1, OWN_ALLOW.iter().map(|rule| (*rule).to_string()));
    allow.insert(2, "ArtifactData".to_string());
    old["permissions"]["allow"] = json!(allow);
    old["permissions"]["ask"] = json!(OWN_ASK);
    let mut deny = rules(&old, "deny");
    deny.insert(0, OWN_DENY[0].to_string());
    old["permissions"]["deny"] = json!(deny);
    std::fs::write(root.join(".claude/settings.local.json"), serde_json::to_string_pretty(&old).unwrap()).unwrap();
    assert!(!root.join(".claude/mustard/pages").exists());
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

/// O início da sessão aponta para o painel local, sem pedir publicação.
/// Um endereço histórico confirmado continua disponível na barra de status.
#[test]
fn a_session_without_project_page_tracks_locally_and_preserves_a_legacy_link() {
    let dir = tempfile::tempdir().unwrap();
    let (root, home) = fresh_repo(dir.path());
    std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).unwrap();
    rt_ok(&root, &home, &["run", "upsert"], "");
    let template = ".claude/mustard/pages/project.html";
    assert!(!root.join(template).exists());
    for source in ["startup", "clear", "compact"] {
        let context = session_start(&root, &home, source);
        assert!(context.contains("/mustard-panel") && context.contains("/mustard-publish"), "{context}");
        assert!(!context.contains(template), "no automatic publication instruction: {context}");
    }
    let failed = record_project_page(&root, &home, &json!({"page":"project","ok":false,"reason":"offline"}));
    assert!(!failed.status.success());
    let no_url = record_project_page(&root, &home, &json!({"page":"project","ok":true}));
    assert!(!no_url.status.success());
    let url = "https://claude.ai/code/artifact/pagina-do-projeto";
    let recorded = record_project_page(&root, &home, &json!({"page":"project","ok":true,"url":url}));
    assert!(recorded.status.success(), "{}", String::from_utf8_lossy(&recorded.stdout));
    rt_ok(&root, &home, &["run", "index"], "");
    let bar = rt_ok(&root, &home, &["run", "statusline"], &json!({"workspace":{"current_dir":root}}).to_string());
    assert!(bar.contains(url) && bar.contains("/mustard-panel"), "legacy links and local tracking coexist: {bar}");
    assert!(!session_start(&root,&home,"startup").contains(template));
}

/// A instalação local preenche o idioma de resposta do Claude Code e desliga a
/// compactação automática só no projeto, pelo `upsert` que a pessoa roda: o
/// idioma da conversa vira o valor da chave, trocar o idioma e instalar de novo
/// troca o valor, e o que a pessoa já tinha nas duas chaves fica. A compactação
/// pedida à mão não é tocada, e o resto do `env` da pessoa segue igual.
#[test]
fn the_install_fills_the_response_language_and_turns_off_the_automatic_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let (root, home) = fresh_repo(dir.path());
    std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).unwrap();
    std::fs::write(root.join(".claude/settings.local.json"), r#"{"env":{"MY_OWN":"1"}}"#).unwrap();
    rt_ok(&root, &home, &["run", "upsert"], "");
    let settings = local_settings(&root);
    assert_eq!(settings["language"], json!("português do Brasil"), "{settings}");
    assert_eq!(settings["env"]["DISABLE_AUTO_COMPACT"], json!("1"), "{settings}");
    assert!(settings["env"].get("DISABLE_COMPACT").is_none(), "the manual /compact stays on: {settings}");
    assert_eq!(settings["env"]["MY_OWN"], json!("1"), "the person's own variable stays: {settings}");
    let settled = std::fs::read_to_string(root.join(".claude/settings.local.json")).unwrap();
    rt_ok(&root, &home, &["run", "upsert"], "");
    assert_eq!(
        std::fs::read_to_string(root.join(".claude/settings.local.json")).unwrap(),
        settled,
        "a second install changes nothing",
    );

    // Trocar o idioma e instalar de novo troca o valor do idioma de resposta.
    std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"en-US"}}"#).unwrap();
    rt_ok(&root, &home, &["run", "upsert"], "");
    assert_eq!(local_settings(&root)["language"], json!("English"), "a language change swaps the value");

    // O que a pessoa escolheu por conta própria nas duas chaves fica.
    let mut own = local_settings(&root);
    own["language"] = json!("japanese");
    own["env"]["DISABLE_AUTO_COMPACT"] = json!("0");
    std::fs::write(root.join(".claude/settings.local.json"), serde_json::to_string_pretty(&own).unwrap()).unwrap();
    rt_ok(&root, &home, &["run", "upsert"], "");
    let kept = local_settings(&root);
    assert_eq!(kept["language"], json!("japanese"), "the person's own language stays: {kept}");
    assert_eq!(kept["env"]["DISABLE_AUTO_COMPACT"], json!("0"), "the person's own value stays: {kept}");
}

/// A barra de status não traz o ponto em que a conversa seria compactada, nem
/// com a variável da fatia de compactação definida no ambiente de quem a roda.
#[test]
fn the_status_line_has_no_compaction_point_even_with_the_variable_set() {
    let dir = tempfile::tempdir().unwrap();
    let (root, home) = fresh_repo(dir.path());
    std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).unwrap();
    rt_ok(&root, &home, &["run", "upsert"], "");
    let payload = json!({
        "workspace": {"current_dir": root},
        "model": {"display_name": "Opus 5 (1M context)"},
        "context_window": {"total_input_tokens": 230_000, "total_output_tokens": 10_000},
    })
    .to_string();
    for lang in ["pt-BR", "en-US"] {
        std::fs::write(root.join("mustard.json"), format!(r#"{{"language":{{"text":"{lang}"}}}}"#)).unwrap();
        let out = rt_with_env(
            &root,
            &home,
            &["run", "statusline"],
            &payload,
            &[("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE", "25")],
        );
        assert!(out.status.success(), "{lang}: {}", String::from_utf8_lossy(&out.stderr));
        let bar = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(bar.contains("Opus 5"), "{lang}: the bar still draws: {bar}");
        for part in ["compacta em", "compacts at", "faltam", "left"] {
            assert!(!bar.contains(part), "{lang}: the bar shows the compaction point (`{part}`): {bar}");
        }
    }
}
