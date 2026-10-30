//! As duas chaves que o `mustard.json` guarda para o projeto, pelo binário de
//! verdade: `enabled`, que liga e desliga o Mustard, e `rtk`, que põe e tira o
//! gancho do rtk das configurações locais.
//!
//! Cada teste roda o `upsert`, o gancho e o `doctor` como a pessoa roda, num
//! repositório temporário com um `.claude/settings.json` da equipe e com uma
//! pasta pessoal falsa. O que se afirma é o que sobra no disco: o arquivo da
//! equipe byte a byte igual, nenhum `disableAllHooks` escrito, e a pasta
//! pessoal vazia do começo ao fim.

#[path = "support/binary.rs"]
mod binary;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

/// O arquivo da equipe, que nenhuma das chaves pode tocar.
const TEAM_SETTINGS: &str = "{\n  \"env\": {\n    \"TEAM_ONLY\": \"1\"\n  },\n  \"hooks\": {\n    \"Stop\": []\n  }\n}\n";

/// Um projeto instalado: repositório git, o arquivo da equipe e a primeira
/// rodada do `upsert`. Devolve a pasta do projeto e a pasta pessoal falsa.
fn installed(dir: &Path) -> (PathBuf, PathBuf) {
    let root = dir.join("project");
    let home = dir.join("home");
    std::fs::create_dir_all(root.join(".claude")).expect("project dir");
    std::fs::create_dir_all(&home).expect("home dir");
    let git = Command::new("git").args(["init", "-q"]).current_dir(&root).output().expect("git init");
    assert!(git.status.success(), "git init");
    std::fs::write(root.join(".claude/settings.json"), TEAM_SETTINGS).expect("team settings");
    let first = upsert(&root, &home);
    assert_eq!(first["private"], json!(true), "{first}");
    (root, home)
}

/// Roda o binário no projeto, com a pasta pessoal falsa e sem `claude` à mão.
fn rt(root: &Path, home: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut cmd = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"));
    cmd.args(args)
        .current_dir(root)
        .env("CLAUDE_PROJECT_DIR", root)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CLAUDE_PLUGIN_ROOT")
        .env("MUSTARD_CLAUDE_BIN", home.join("no-claude-here"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("the binary runs");
    if let Some(mut pipe) = child.stdin.take() {
        let _ = pipe.write_all(stdin.unwrap_or_default().as_bytes());
    }
    child.wait_with_output().expect("the binary finishes")
}

fn upsert(root: &Path, home: &Path) -> Value {
    let out = rt(root, home, &["run", "upsert"], None);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).expect("the upsert answers JSON")
}

/// O que o `doctor` diz das chaves.
fn doctor(root: &Path, home: &Path) -> String {
    let out = rt(root, home, &["run", "doctor", "--check", "switches"], None);
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Muda uma chave do `mustard.json`, como a pessoa muda.
fn set(root: &Path, key: &str, value: bool) {
    let path = root.join("mustard.json");
    let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("config")).expect("json");
    config[key] = json!(value);
    std::fs::write(&path, serde_json::to_string_pretty(&config).expect("json")).expect("write config");
}

fn rtk_hook_in_local_settings(root: &Path) -> bool {
    let raw = std::fs::read_to_string(root.join(".claude/settings.local.json")).expect("local settings");
    let settings: Value = serde_json::from_str(&raw).expect("local settings are JSON");
    mustard_core::platform::project_seed::settings::rtk_hook_present(settings.as_object().expect("object"))
}

/// Nada foi escrito fora do projeto, e nenhum arquivo do projeto ganhou o
/// interruptor que cala todos os ganchos.
fn nothing_outside_and_no_silencer(root: &Path, home: &Path) {
    let entries: Vec<String> = std::fs::read_dir(home)
        .expect("home")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(entries.is_empty(), "something was written in the personal folder: {entries:?}");
    assert_eq!(
        std::fs::read_to_string(root.join(".claude/settings.json")).expect("team settings"),
        TEAM_SETTINGS,
        "the team's settings changed",
    );
    for name in ["settings.json", "settings.local.json"] {
        let raw = std::fs::read_to_string(root.join(".claude").join(name)).unwrap_or_default();
        assert!(!raw.contains("disableAllHooks"), "{name} carries disableAllHooks");
    }
}

/// Um `spec.html` de uma instalação de 12/09, editado por fora: o `upsert`
/// seguinte o troca pelo texto do binário de hoje, mesmo sem mudar nenhuma
/// chave — os dois modelos de página são texto do Mustard, nunca do projeto.
#[test]
fn upsert_preserves_a_historical_page_template_without_automatic_publication() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (root, home) = installed(dir.path());
    let page = root.join(".claude/mustard/pages/spec.html");
    let stale = "<html><!-- installed 12/09, never touched since --></html>";
    assert!(!page.exists(), "fresh installs create no automatic public page");
    std::fs::create_dir_all(page.parent().expect("page parent")).expect("legacy page directory");
    std::fs::write(&page, stale).expect("plant a historical template");

    let report = upsert(&root, &home);

    let updated = report["updated"].as_array().expect("updated is a list");
    let names: Vec<&str> = updated.iter().filter_map(Value::as_str).collect();
    assert!(!names.contains(&".claude/mustard/pages/spec.html"), "historical export resources are preserved: {names:?}");
    let rewritten = std::fs::read_to_string(&page).expect("the page still exists");
    assert_eq!(rewritten, stale, "updating local tracking does not rewrite historical published resources");
}

/// O gancho do próprio rtk (`rtk hook claude`) deixado no
/// `.claude/settings.local.json`, com a opção `rtk` ligada ou desligada: o
/// `doctor` avisa que ele disputa a troca com o gancho do Mustard, a
/// reinstalação o tira, e nada muda na pasta pessoal nem no arquivo da equipe.
#[test]
fn the_install_takes_rtk_own_hook_out_whatever_the_option() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (root, home) = installed(dir.path());
    assert!(!rtk_hook_in_local_settings(&root), "the install never writes rtk's own hook");
    assert!(doctor(&root, &home).contains("OK    switches"), "{}", doctor(&root, &home));

    for option in [true, false] {
        set(&root, "rtk", option);
        let path = root.join(".claude/settings.local.json");
        let mut settings: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("local")).expect("json");
        settings["hooks"] = json!({"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"rtk hook claude"}]}]});
        std::fs::write(&path, serde_json::to_string_pretty(&settings).expect("json")).expect("write local");
        let left = doctor(&root, &home);
        assert!(left.contains("WARN  switches") && left.contains("continua no"), "rtk {option}: {left}");
        upsert(&root, &home);
        assert!(!rtk_hook_in_local_settings(&root), "rtk {option}: the hook leaves");
        assert!(doctor(&root, &home).contains("OK    switches"), "{}", doctor(&root, &home));
        nothing_outside_and_no_silencer(&root, &home);
    }
}

/// A chave `enabled` do `mustard.json` desligada e religada num projeto com
/// um `settings.json` da equipe: desligada, nenhum gancho age (o comando que a
/// trava barra passa) e o `doctor` avisa; o `upsert` não escreve
/// `disableAllHooks`, não toca no arquivo da equipe nem na pasta pessoal; e,
/// religada, a trava volta a barrar e o aviso some.
#[test]
fn the_enabled_key_turns_every_hook_off_and_back_on() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (root, home) = installed(dir.path());
    let dangerous = json!({
        "tool_name": "Bash",
        "tool_input": {"command": "mustard-rt run read backlog | head -5"},
        "hook_event_name": "PreToolUse",
        "cwd": root.to_string_lossy(),
    })
    .to_string();
    let hook = |label: &str| {
        let out = rt(&root, &home, &["on", "PreToolUse"], Some(&dangerous));
        assert!(out.status.success(), "{label}: a hook never fails the call");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    assert!(hook("on").contains("\"deny\""), "the guard acts while Mustard is on");

    set(&root, "enabled", false);
    assert!(!hook("off").contains("\"deny\""), "no hook acts while Mustard is off");
    let off = doctor(&root, &home);
    assert!(off.contains("WARN  switches") && off.contains("desligado neste projeto"), "{off}");
    upsert(&root, &home);
    let config: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("mustard.json")).expect("config")).expect("json");
    assert_eq!(config["enabled"], json!(false), "the choice stays where the person wrote it");
    nothing_outside_and_no_silencer(&root, &home);

    set(&root, "enabled", true);
    assert!(hook("back on").contains("\"deny\""), "the guard acts again");
    assert!(!doctor(&root, &home).contains("desligado"), "{}", doctor(&root, &home));
    nothing_outside_and_no_silencer(&root, &home);
}
