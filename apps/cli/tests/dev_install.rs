//! `scripts/dev-install.sh` na cópia do plugin e numa cópia de sistema de
//! teste: os três programas, os moldes, o estilo de resposta e os comandos
//! trocam no lugar, o selo de versão do plugin (`bin/.version`) nunca é
//! tocado, e `--restore` devolve tudo ao que estava antes.
//!
//! O `cargo build --release` de verdade é o único passo simulado — um
//! `cargo` falso no `PATH` grava um binário reconhecível em
//! `$CARGO_TARGET_DIR/release/<nome>`, para o teste não pagar uma compilação
//! de entrega inteira a cada rodada. O resto — a versão lida do manifesto do
//! plugin, os moldes, os comandos, os ganchos e o estilo de resposta — vem
//! do próprio repositório, não de um resumo escrito à mão.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A raiz do repositório, a partir deste crate (`apps/cli`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

/// A versão gravada no manifesto do plugin desta branch — a mesma que o
/// script lê para achar a cópia certa no cache do Claude Code.
fn plugin_version() -> String {
    let manifest = repo_root().join("plugin/.claude-plugin/plugin.json");
    let raw = fs::read_to_string(&manifest).expect("plugin.json readable");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("plugin.json is JSON");
    value["version"].as_str().expect("plugin.json has a version").to_string()
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().expect("has a parent")).expect("mkdir -p");
    fs::write(path, content).expect("write file");
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Um `cargo` de mentira: em vez de compilar, grava um arquivo reconhecível
/// para cada `--bin` pedido, em `$CARGO_TARGET_DIR/release/`. É contra esse
/// conteúdo que o teste confere se o script copiou o binário CERTO, e não um
/// arquivo qualquer que já estivesse lá.
fn shim_cargo(dir: &Path) {
    let script = "#!/bin/sh\nset -e\nmkdir -p \"$CARGO_TARGET_DIR/release\"\nfor b in scan mustard-rt mustard; do\n  printf 'built-%s' \"$b\" > \"$CARGO_TARGET_DIR/release/$b\"\n  chmod +x \"$CARGO_TARGET_DIR/release/$b\"\ndone\n";
    let path = dir.join("cargo");
    write(&path, script);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod shim");
    }
}

/// Roda `scripts/dev-install.sh` de dentro do repositório de verdade (é dali
/// que ele lê a versão do manifesto, os moldes, os comandos, os ganchos e o
/// estilo de resposta), contra uma pasta de plugin e uma de sistema de
/// teste.
fn run_script(args: &[&str], shim: &Path, home: &Path, system_dir: &Path, cargo_target: &Path, backup_dir: &Path) -> Output {
    let real_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{real_path}", shim.display());
    Command::new("sh")
        .arg(repo_root().join("scripts/dev-install.sh"))
        .args(args)
        .env_clear()
        .env("PATH", path)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("CARGO_TARGET_DIR", cargo_target)
        .env("MUSTARD_DEV_INSTALL_SYSTEM_DIR", system_dir)
        .env("MUSTARD_DEV_INSTALL_BACKUP_DIR", backup_dir)
        .output()
        .expect("the script runs")
}

/// A única pasta datada que nasceu dentro de `backup_root`.
fn the_dated_backup(backup_root: &Path) -> PathBuf {
    let mut entries: Vec<_> = fs::read_dir(backup_root)
        .expect("backup root exists")
        .map(|e| e.expect("dir entry").path())
        .collect();
    assert_eq!(entries.len(), 1, "exactly one dated backup folder: {entries:?}");
    entries.remove(0)
}

/// Todo arquivo sob `dir`, com o caminho relativo a `dir`, em ordem.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("read_dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push(path.strip_prefix(root).expect("under root").to_path_buf());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// `a` e `b` têm exatamente os mesmos arquivos (mesmos caminhos relativos),
/// com o mesmo conteúdo byte a byte.
fn assert_trees_equal(a: &Path, b: &Path) {
    let fa = files_under(a);
    let fb = files_under(b);
    assert_eq!(fa, fb, "listas de arquivo diferentes entre {} e {}", a.display(), b.display());
    for rel in fa {
        let ca = fs::read(a.join(&rel)).expect("read a");
        let cb = fs::read(b.join(&rel)).expect("read b");
        assert_eq!(ca, cb, "conteúdo diferente em {}", rel.display());
    }
}

/// Semeia uma cópia do plugin (versão do manifesto) com conteúdo antigo,
/// reconhecível, em tudo que o script troca — inclusive o selo de versão,
/// que tem de sobreviver sem ser tocado.
fn seed_plugin_copy(claude_dir: &Path, version: &str) -> PathBuf {
    let copy = claude_dir.join(format!("plugins/cache/mustard-local/mustard/{version}"));
    write(&copy.join("bin/mustard"), "old-mustard");
    write(&copy.join("bin/mustard-rt"), "old-mustard-rt");
    write(&copy.join("bin/scan"), "old-scan");
    write(&copy.join("bin/.version"), version);
    write(&copy.join("bin/templates/OLD.txt"), "old templates");
    write(&copy.join("commands/OLD.md"), "old command");
    write(&copy.join("hooks/hooks.json"), "{\"old\":true}");
    write(&copy.join("output-styles/OLD.md"), "old style");
    copy
}

fn seed_system_copy(system_dir: &Path) {
    write(&system_dir.join("bin/mustard"), "old-system-mustard");
    write(&system_dir.join("bin/mustard-rt"), "old-system-mustard-rt");
    write(&system_dir.join("bin/scan"), "old-system-scan");
    write(&system_dir.join("templates/OLD.txt"), "old system templates");
}

/// `id -u` de verdade — decide qual dos dois ramos da cópia do sistema o
/// script toma: a máquina de teste normalmente não é root.
fn running_as_root() -> bool {
    Command::new("id").arg("-u").output().map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0").unwrap_or(false)
}

#[test]
#[cfg_attr(not(unix), ignore = "o script é sh")]
fn the_dev_install_script_swaps_files_in_place() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let claude_dir = home.join(".claude");
    let system_dir = tmp.path().join("system");
    let cargo_target = tmp.path().join("cargo-target");
    let backup_root = tmp.path().join("backups");
    let shim = tmp.path().join("shim");
    fs::create_dir_all(&shim).expect("mkdir shim");
    shim_cargo(&shim);

    let version = plugin_version();
    let plugin_copy = seed_plugin_copy(&claude_dir, &version);
    seed_system_copy(&system_dir);

    // --- a troca -------------------------------------------------------
    let out = run_script(&[], &shim, &home, &system_dir, &cargo_target, &backup_root);
    assert!(out.status.success(), "stdout={}\nstderr={}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));

    // Os três programas: o conteúdo é o que o `cargo` falso gravou, não mais
    // o antigo — a prova de que o script troca, e não só confere.
    assert_eq!(read(&plugin_copy.join("bin/mustard")), "built-mustard");
    assert_eq!(read(&plugin_copy.join("bin/mustard-rt")), "built-mustard-rt");
    assert_eq!(read(&plugin_copy.join("bin/scan")), "built-scan");

    // O selo de versão nunca é tocado.
    assert_eq!(read(&plugin_copy.join("bin/.version")), version, "o selo de versão mudou");

    // Os moldes, o estilo de resposta e os comandos vêm do repositório de
    // verdade, não de um resumo escrito à mão para o teste.
    assert_trees_equal(&plugin_copy.join("bin/templates"), &repo_root().join("apps/cli/templates"));
    assert_trees_equal(&plugin_copy.join("commands"), &repo_root().join("plugin/commands"));
    assert_trees_equal(&plugin_copy.join("hooks"), &repo_root().join("plugin/hooks"));
    assert_trees_equal(&plugin_copy.join("output-styles"), &repo_root().join("plugin/output-styles"));

    // A pasta datada guarda os originais — inclusive os moldes, os comandos
    // e os ganchos antigos — mas nunca o selo de versão.
    let backup_dir = the_dated_backup(&backup_root);
    assert_eq!(read(&backup_dir.join("plugin/bin/mustard")), "old-mustard");
    assert_eq!(read(&backup_dir.join("plugin/bin/mustard-rt")), "old-mustard-rt");
    assert_eq!(read(&backup_dir.join("plugin/bin/scan")), "old-scan");
    assert_eq!(read(&backup_dir.join("plugin/bin/templates/OLD.txt")), "old templates");
    assert_eq!(read(&backup_dir.join("plugin/commands/OLD.md")), "old command");
    assert_eq!(read(&backup_dir.join("plugin/hooks/hooks.json")), "{\"old\":true}");
    assert_eq!(read(&backup_dir.join("plugin/output-styles/OLD.md")), "old style");
    assert!(!backup_dir.join("plugin/bin/.version").exists(), "o selo de versão não deveria nem ir para o backup");

    // A máquina de teste não é root: a cópia do sistema fica intocada, e o
    // comando pronto (com sudo) aparece na saída.
    let stdout = String::from_utf8_lossy(&out.stdout);
    if running_as_root() {
        assert_eq!(read(&system_dir.join("bin/mustard")), "built-mustard");
        assert_eq!(read(&system_dir.join("bin/mustard-rt")), "built-mustard-rt");
        assert_eq!(read(&system_dir.join("bin/scan")), "built-scan");
        assert_trees_equal(&system_dir.join("templates"), &repo_root().join("apps/cli/templates"));
    } else {
        assert_eq!(read(&system_dir.join("bin/mustard")), "old-system-mustard");
        assert_eq!(read(&system_dir.join("bin/mustard-rt")), "old-system-mustard-rt");
        assert_eq!(read(&system_dir.join("bin/scan")), "old-system-scan");
        assert_eq!(read(&system_dir.join("templates/OLD.txt")), "old system templates");
        assert!(stdout.contains("sudo"), "sem root, o comando pronto tem de aparecer: {stdout}");
        assert!(stdout.contains(&system_dir.display().to_string()), "o comando pronto tem de citar a pasta do sistema: {stdout}");
    }

    // --- a volta ---------------------------------------------------------
    let restore = run_script(
        &["--restore", backup_dir.to_str().expect("utf8 path")],
        &shim,
        &home,
        &system_dir,
        &cargo_target,
        &backup_root,
    );
    assert!(restore.status.success(), "stdout={}\nstderr={}", String::from_utf8_lossy(&restore.stdout), String::from_utf8_lossy(&restore.stderr));

    assert_eq!(read(&plugin_copy.join("bin/mustard")), "old-mustard");
    assert_eq!(read(&plugin_copy.join("bin/mustard-rt")), "old-mustard-rt");
    assert_eq!(read(&plugin_copy.join("bin/scan")), "old-scan");
    assert_eq!(read(&plugin_copy.join("bin/.version")), version, "o selo de versão não deveria mudar nem na volta");
    assert_eq!(files_under(&plugin_copy.join("bin/templates")), vec![PathBuf::from("OLD.txt")]);
    assert_eq!(read(&plugin_copy.join("bin/templates/OLD.txt")), "old templates");
    assert_eq!(files_under(&plugin_copy.join("commands")), vec![PathBuf::from("OLD.md")]);
    assert_eq!(read(&plugin_copy.join("commands/OLD.md")), "old command");
    assert_eq!(files_under(&plugin_copy.join("hooks")), vec![PathBuf::from("hooks.json")]);
    assert_eq!(read(&plugin_copy.join("hooks/hooks.json")), "{\"old\":true}");
    assert_eq!(files_under(&plugin_copy.join("output-styles")), vec![PathBuf::from("OLD.md")]);
    assert_eq!(read(&plugin_copy.join("output-styles/OLD.md")), "old style");
}

/// Sem uma cópia do plugin instalada na versão do manifesto, o script recusa
/// antes de compilar ou tocar em qualquer arquivo — não há onde trocar nada.
#[test]
#[cfg_attr(not(unix), ignore = "o script é sh")]
fn refuses_without_a_matching_plugin_copy() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).expect("mkdir home");
    let system_dir = tmp.path().join("system");
    let cargo_target = tmp.path().join("cargo-target");
    let backup_root = tmp.path().join("backups");
    let shim = tmp.path().join("shim");
    fs::create_dir_all(&shim).expect("mkdir shim");
    shim_cargo(&shim);

    let out = run_script(&[], &shim, &home, &system_dir, &cargo_target, &backup_root);

    assert!(!out.status.success(), "sem cópia do plugin, o script não pode dar certo");
    assert!(!cargo_target.join("release").exists(), "recusou antes de compilar");
    assert!(!backup_root.exists(), "recusou antes de guardar qualquer backup");
}
