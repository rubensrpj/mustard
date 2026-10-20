// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! O diagnóstico da instalação confere o plugin pelo registro que mora na
//! pasta de configuração do Claude Code. Quem mudou essa pasta de lugar com
//! `CLAUDE_CONFIG_DIR` tem o registro lá, e não em `~/.claude`: o diagnóstico
//! segue a variável e confere a instalação mais nova que o registro guarda.
//!
//! A pasta pessoal do teste é vazia: um diagnóstico que montasse o caminho do
//! registro a partir dela não acharia registro nenhum e responderia que não
//! mediu nada — o mesmo silêncio que esconde, no campo, o plugin cujo binário
//! nunca baixou.

#[path = "support/binary.rs"]
mod binary;

use std::fs;
use std::path::Path;

#[test]
fn explicit_project_diagnosis_works_from_another_directory_without_installing_anything() {
    let dir=tempfile::tempdir().unwrap();
    let project=dir.path().join("project");let caller=dir.path().join("caller");
    fs::create_dir(&project).unwrap();fs::create_dir(&caller).unwrap();fs::write(project.join("mustard.json"),"{}").unwrap();
    let result=binary::isolated(env!("CARGO_BIN_EXE_mustard-rt")).current_dir(&caller)
        .env("MUSTARD_RT_DELEGATED","1").env("CLAUDE_CONFIG_DIR",dir.path().join("host"))
        .args(["run","doctor","--root"]).arg(&project).args(["--check","session-plugins","--json"]).output().unwrap();
    assert!(result.status.success(),"{}",String::from_utf8_lossy(&result.stderr));
    let report:serde_json::Value=serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["checks"][0]["name"],"session-plugins");
    assert_eq!(report["checks"][0]["status"],"skip");
    assert!(!project.join(".claude").exists());assert!(!caller.join(".claude").exists());
}

/// Grava o registro dos plugins em `config`, com uma instalação por par
/// `(pasta, versão)`, na ordem dada.
fn seed_registry(config: &Path, installs: &[(&Path, &str)]) {
    let records: Vec<serde_json::Value> = installs
        .iter()
        .map(|(dir, version)| {
            serde_json::json!({"scope": "user", "installPath": dir.to_string_lossy(), "version": version})
        })
        .collect();
    let registry = serde_json::json!({"version": 2, "plugins": {"mustard@mustard-local": records}});
    fs::create_dir_all(config.join("plugins")).unwrap();
    fs::write(config.join("plugins").join("installed_plugins.json"), registry.to_string()).unwrap();
}

/// Uma instalação do plugin em `dir`; com `binary`, o `bin/` traz o
/// `mustard-rt`, e sem ele traz só o script de arranque, como o plugin chega
/// antes de o binário baixar.
fn seed_install(dir: &Path, binary: bool) {
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let boot = if cfg!(windows) { "mustard-boot.cmd" } else { "mustard-boot" };
    fs::write(bin.join(boot), "#!/bin/sh\n").unwrap();
    if binary {
        let exe = if cfg!(windows) { "mustard-rt.exe" } else { "mustard-rt" };
        fs::write(bin.join(exe), "").unwrap();
    }
}

/// Com o registro na pasta de configuração movida, o diagnóstico acha a
/// instalação mais nova e acusa o binário que falta nela. A mais antiga,
/// primeira da lista, tem o binário: um diagnóstico que lesse o primeiro
/// registro, ou que procurasse o registro na pasta pessoal, sairia sem falha.
#[test]
fn the_doctor_follows_the_moved_config_folder_to_the_newest_install() {
    let base = tempfile::tempdir().unwrap();
    let home = base.path().join("pessoal");
    let config = base.path().join("configuracao-movida");
    let project = base.path().join("projeto");
    let older = base.path().join("instalacoes").join("0.1.9");
    let newer = base.path().join("instalacoes").join("0.1.10");
    for dir in [&home, &config, &project] {
        fs::create_dir_all(dir).unwrap();
    }
    seed_install(&older, true);
    seed_install(&newer, false);
    seed_registry(&config, &[(&older, "0.1.9"), (&newer, "0.1.10")]);

    let out = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "doctor", "--json"])
        .current_dir(&project)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("CLAUDE_CONFIG_DIR", &config)
        .env("CLAUDE_PROJECT_DIR", &project)
        .output()
        .expect("o binário roda");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let report: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("o diagnóstico imprime um relatório JSON ({e}): {stdout}"));
    let bootstrap = report["checks"]
        .as_array()
        .and_then(|checks| checks.iter().find(|check| check["name"] == "bootstrap"))
        .unwrap_or_else(|| panic!("o diagnóstico traz a conferência do arranque: {stdout}"));
    let details = bootstrap["details"].to_string();

    assert_eq!(bootstrap["status"], "fail", "o binário que falta na instalação mais nova é falha: {bootstrap}");
    assert!(details.contains("binary-missing"), "a falha diz que o binário falta: {details}");
    assert!(
        details.contains("0.1.10") && !details.contains("0.1.9"),
        "a instalação conferida é a mais nova do registro: {details}"
    );
}

/// O diagnóstico diz o commit do programa em uso, do `mustard-rt` do plugin e
/// do primeiro `mustard-rt` do `PATH`, e avisa quando eles divergem. Com os
/// três no mesmo carimbo, não há aviso; com o do plugin num commit diferente,
/// há, e o aviso cita os dois carimbos.
#[cfg(unix)]
#[test]
fn the_doctor_warns_when_the_plugin_binary_and_the_running_one_carry_different_commits() {
    use std::os::unix::fs::PermissionsExt;
    let base = tempfile::tempdir().unwrap();
    let (home, config, project) = (base.path().join("pessoal"), base.path().join("config"), base.path().join("projeto"));
    let (install, system) = (base.path().join("instalacoes").join("0.2.21"), base.path().join("sistema"));
    for dir in [&home, &config, &project, &system] {
        fs::create_dir_all(dir).unwrap();
    }
    seed_install(&install, false);
    seed_registry(&config, &[(&install, "0.2.21")]);
    let answering = |path: &Path, line: &str| {
        fs::write(path, format!("#!/bin/sh\necho '{line}'\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    };
    let running = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt")).arg("--version").env("MUSTARD_RT_DELEGATED", "1").output().unwrap();
    let running = String::from_utf8_lossy(&running.stdout).trim().to_string();
    let running_stamp = running.split_once(", ").map(|(_, rest)| rest.trim_end_matches(')').to_string()).unwrap_or_default();
    assert!(running_stamp.starts_with('g'), "o programa do teste não diz o commit, e nada se compara: {running}");
    answering(&system.join("mustard-rt"), &running);
    let path = std::env::join_paths(std::iter::once(system.clone()).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
    let doctor = || {
        let out = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(["run", "doctor", "--check", "binaries", "--json"])
            .current_dir(&project)
            .env("MUSTARD_RT_DELEGATED", "1")
            .env("HOME", &home)
            .env("CLAUDE_CONFIG_DIR", &config)
            .env("CLAUDE_PROJECT_DIR", &project)
            .env("PATH", &path)
            .output()
            .unwrap();
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        report["checks"][0].clone()
    };

    answering(&install.join("bin").join("mustard-rt"), &running);
    let same = doctor();
    assert_eq!(same["status"], "ok", "os três no mesmo commit não divergem: {same}");
    assert!(same["details"][0].as_str().unwrap().contains(&format!("plugin {running_stamp} · system {running_stamp}")), "{same}");

    answering(&install.join("bin").join("mustard-rt"), "mustard-rt 0.2.21 (build 7, g0123456789ab 2026-01-02)");
    let diverged = doctor();
    let details = diverged["details"].to_string();
    assert_eq!(diverged["status"], "warn", "o plugin noutro commit diverge: {diverged}");
    assert!(details.contains("g0123456789ab 2026-01-02") && details.contains(&running_stamp), "o aviso cita os dois carimbos: {details}");
}
