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

/// Um executável (qualquer nome) que sempre escreve `stdout` para stdout,
/// ignorando os argumentos. Usado para congelar `date` (a mesma marca de
/// tempo nas duas rodadas de um teste) e para forjar `id -u` (fingir ser
/// root sem precisar de root de verdade).
fn shim_fixed_output(dir: &Path, name: &str, stdout: &str) {
    let script = format!("#!/bin/sh\necho '{stdout}'\n");
    let path = dir.join(name);
    write(&path, &script);
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

/// O conteúdo que o selo de versão (`bin/.version`) traz nos testes: um
/// valor que o script JAMAIS produziria sozinho (ele só escreveria a versão
/// do manifesto ali). Semear com a própria versão do manifesto escondia uma
/// gravação: se o script reescrevesse o selo com `$VERSAO`, o arquivo ficava
/// com o mesmo conteúdo de antes, e o teste não via diferença nenhuma.
const VERSION_SEAL_SEED: &str = "conteudo-que-o-script-nunca-escreveria";

/// Semeia uma cópia do plugin (versão do manifesto) com conteúdo antigo,
/// reconhecível, em tudo que o script troca — inclusive o selo de versão,
/// que tem de sobreviver sem ser tocado.
fn seed_plugin_copy(claude_dir: &Path, version: &str) -> PathBuf {
    let copy = seed_plugin_copy_without_version_seal(claude_dir, version);
    write(&copy.join("bin/.version"), VERSION_SEAL_SEED);
    copy
}

/// A mesma semeadura, sem gravar `bin/.version` — para a cópia que nunca
/// teve selo de versão (por exemplo, uma cópia dev que nasceu sem o
/// `mustard-boot` ter rodado) continuar sem ele depois do script.
fn seed_plugin_copy_without_version_seal(claude_dir: &Path, version: &str) -> PathBuf {
    let copy = claude_dir.join(format!("plugins/cache/mustard-local/mustard/{version}"));
    write(&copy.join("bin/mustard"), "old-mustard");
    write(&copy.join("bin/mustard-rt"), "old-mustard-rt");
    write(&copy.join("bin/scan"), "old-scan");
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

/// Um `chown` de mentira: em vez de trocar o dono de verdade (a máquina de
/// teste não é root), grava cada chamada — argumentos inclusive — numa linha
/// do arquivo de log. É contra esse log que os testes de dono conferem QUEM
/// o script tentou tornar dono de quê, sem precisar de privilégio real.
fn shim_logging_chown(dir: &Path, log: &Path) {
    let script = format!("#!/bin/sh\necho \"$*\" >> \"{}\"\n", log.display());
    let path = dir.join("chown");
    write(&path, &script);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod shim");
    }
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

    // O selo de versão nunca é tocado: continua com o valor semeado, que o
    // script não teria como produzir sozinho (ele só escreveria a versão).
    assert_eq!(read(&plugin_copy.join("bin/.version")), VERSION_SEAL_SEED, "o selo de versão mudou");

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
    assert_eq!(read(&plugin_copy.join("bin/.version")), VERSION_SEAL_SEED, "o selo de versão não deveria mudar nem na volta");
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

/// Duas rodadas no mesmo segundo caem no mesmo nome de pasta datada. A
/// segunda tem de recusar ANTES de trocar qualquer arquivo — senão ela
/// gravaria, como "original", o programa que a primeira rodada já trocou,
/// perdendo o original de verdade que a primeira guardou. Um `date`
/// congelado garante que as duas rodadas caiam no mesmo segundo de fato, sem
/// depender da sorte do relógio da máquina.
#[test]
#[cfg_attr(not(unix), ignore = "o script é sh")]
fn a_second_run_in_the_same_second_refuses_and_keeps_the_first_backup() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let claude_dir = home.join(".claude");
    let system_dir = tmp.path().join("system");
    let cargo_target = tmp.path().join("cargo-target");
    let backup_root = tmp.path().join("backups");
    let shim = tmp.path().join("shim");
    fs::create_dir_all(&shim).expect("mkdir shim");
    shim_cargo(&shim);
    shim_fixed_output(&shim, "date", "19991231-235959");

    let version = plugin_version();
    let plugin_copy = seed_plugin_copy(&claude_dir, &version);
    seed_system_copy(&system_dir);

    let first = run_script(&[], &shim, &home, &system_dir, &cargo_target, &backup_root);
    assert!(first.status.success(), "stdout={}\nstderr={}", String::from_utf8_lossy(&first.stdout), String::from_utf8_lossy(&first.stderr));
    let backup_dir = the_dated_backup(&backup_root);

    // A primeira rodada já trocou os três programas e guardou os originais.
    assert_eq!(read(&plugin_copy.join("bin/mustard")), "built-mustard");
    assert_eq!(read(&backup_dir.join("plugin/bin/mustard")), "old-mustard");

    let second = run_script(&[], &shim, &home, &system_dir, &cargo_target, &backup_root);
    assert!(
        !second.status.success(),
        "a segunda rodada, no mesmo segundo, tem de recusar: stdout={}\nstderr={}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );

    // Continua havendo uma pasta datada só — a da primeira rodada — e ela
    // continua com o ORIGINAL, sem o "old-mustard" virar "built-mustard".
    let dirs: Vec<_> = fs::read_dir(&backup_root).expect("backup root exists").map(|e| e.expect("dir entry").path()).collect();
    assert_eq!(dirs, vec![backup_dir.clone()], "uma pasta datada só, mesmo depois da recusa");
    assert_eq!(read(&backup_dir.join("plugin/bin/mustard")), "old-mustard", "a segunda rodada não pode sobrescrever o original guardado pela primeira");
    assert_eq!(read(&backup_dir.join("plugin/bin/mustard-rt")), "old-mustard-rt");
    assert_eq!(read(&backup_dir.join("plugin/bin/scan")), "old-scan");
}

/// O comando com `sudo` que o script imprime, sem root, tem de funcionar de
/// verdade: sem HOME real (o `sudo` do Ubuntu troca o HOME para `/root`),
/// sem `cargo` no PATH (não compila nada — só troca a cópia do sistema com o
/// que a rodada sem root já compilou), só quando quem roda é root (aqui,
/// forjado por um `id` de mentira que responde 0) e sem o privilégio de
/// verdade que só um root de verdade tem para o `chown` que deixa a cópia do
/// sistema de root:root (aqui, um `chown` de mentira que sempre dá certo).
#[test]
#[cfg_attr(not(unix), ignore = "o script é sh")]
fn the_printed_sudo_command_swaps_the_system_copy_without_cargo_or_the_real_home() {
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
    seed_plugin_copy(&claude_dir, &version);
    seed_system_copy(&system_dir);

    // A rodada sem root: compila (com o `cargo` de mentira), troca a cópia
    // do plugin e imprime o comando pronto, sem tocar na cópia do sistema.
    let out = run_script(&[], &shim, &home, &system_dir, &cargo_target, &backup_root);
    assert!(out.status.success(), "stdout={}\nstderr={}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if running_as_root() {
        // A própria rodada acima já tomou o ramo root; não há comando com
        // sudo para extrair e testar por fora.
        return;
    }
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let sudo_line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("sudo "))
        .unwrap_or_else(|| panic!("sem root, o comando pronto (com sudo) tem de aparecer: {stdout}"));
    let bare_command = sudo_line.trim_start().trim_start_matches("sudo ");

    // Roda o MESMO comando de verdade, sem `sudo` (o teste não é root), com
    // um `id` de mentira que responde 0, sem HOME real e sem `cargo` no
    // PATH — provando que o comando não depende de nenhum dos dois.
    let fake_root_home = tmp.path().join("fake-root-home");
    fs::create_dir_all(&fake_root_home).expect("mkdir fake root home");
    let id_shim = tmp.path().join("id-shim");
    fs::create_dir_all(&id_shim).expect("mkdir id shim");
    shim_fixed_output(&id_shim, "id", "0");
    shim_logging_chown(&id_shim, &tmp.path().join("chown.log"));

    let result = Command::new("sh")
        .arg("-c")
        .arg(bare_command)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", id_shim.display()))
        .env("HOME", &fake_root_home)
        .output()
        .expect("the printed command runs");
    assert!(result.status.success(), "stdout={}\nstderr={}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));

    // A cópia do sistema foi trocada de verdade, com o binário que a rodada
    // sem root já tinha compilado.
    assert_eq!(read(&system_dir.join("bin/mustard")), "built-mustard");
    assert_eq!(read(&system_dir.join("bin/mustard-rt")), "built-mustard-rt");
    assert_eq!(read(&system_dir.join("bin/scan")), "built-scan");
    assert_trees_equal(&system_dir.join("templates"), &repo_root().join("apps/cli/templates"));

    // E o original foi para a pasta datada que a rodada sem root já tinha
    // criado — a mesma que o comando impresso citou.
    let backup_dir = the_dated_backup(&backup_root);
    assert_eq!(read(&backup_dir.join("system/bin/mustard")), "old-system-mustard");
    assert_eq!(read(&backup_dir.join("system/bin/mustard-rt")), "old-system-mustard-rt");
    assert_eq!(read(&backup_dir.join("system/bin/scan")), "old-system-scan");
    assert_eq!(read(&backup_dir.join("system/templates/OLD.txt")), "old system templates");
}

/// A linha "Para desfazer" tem de rodar como ela sai — o arquivo do script
/// está no git sem permissão de execução (`100644`), então uma linha que
/// invocasse o caminho do script direto, sem `sh` na frente, daria
/// `Permission denied` na mão de quem só copia e cola.
#[test]
#[cfg_attr(not(unix), ignore = "o script é sh")]
fn the_printed_undo_line_runs_as_is() {
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

    let out = run_script(&[], &shim, &home, &system_dir, &cargo_target, &backup_root);
    assert!(out.status.success(), "stdout={}\nstderr={}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let undo_line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("Para desfazer:"))
        .unwrap_or_else(|| panic!("tem de imprimir a linha \"Para desfazer:\": {stdout}"));
    let bare_command = undo_line.trim_start().trim_start_matches("Para desfazer:").trim();

    // Roda a linha impressa exatamente como uma pessoa colaria no terminal:
    // mesma pasta pessoal e mesmas pastas trocáveis da instalação — só sem o
    // `cargo` no PATH (a volta não compila nada).
    let real_path = std::env::var("PATH").unwrap_or_default();
    let result = Command::new("sh")
        .arg("-c")
        .arg(bare_command)
        .env_clear()
        .env("PATH", real_path)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("MUSTARD_DEV_INSTALL_SYSTEM_DIR", &system_dir)
        .env("MUSTARD_DEV_INSTALL_BACKUP_DIR", &backup_root)
        .output()
        .expect("the undo line runs");
    assert!(result.status.success(), "a linha impressa tem de rodar como ela sai: stdout={}\nstderr={}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
    assert_eq!(read(&plugin_copy.join("bin/mustard")), "old-mustard", "a linha impressa tem de desfazer a troca do plugin");
}

/// Sem root, `--restore` só devolve a cópia do plugin. A cópia do sistema
/// pede administrador (como na instalação): o script não tenta mexer nela —
/// só imprime o comando pronto com sudo, que roda `--restore-system-only`
/// sem procurar a cópia do plugin nem compilar nada.
#[test]
#[cfg_attr(not(unix), ignore = "o script é sh")]
fn without_root_restore_only_touches_the_plugin_and_prints_a_ready_system_restore_command() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let claude_dir = home.join(".claude");
    let system_dir = tmp.path().join("system");
    let cargo_target = tmp.path().join("cargo-target");
    let backup_root = tmp.path().join("backups");

    // a instalação é forjada como root (id=0), para a cópia do sistema
    // também trocar e o backup nascer com a parte "system" — do jeito que
    // uma instalação real, feita como root, deixaria.
    let root_shim = tmp.path().join("root-shim");
    fs::create_dir_all(&root_shim).expect("mkdir root shim");
    shim_cargo(&root_shim);
    shim_fixed_output(&root_shim, "id", "0");

    let version = plugin_version();
    let plugin_copy = seed_plugin_copy(&claude_dir, &version);
    seed_system_copy(&system_dir);

    let install = run_script(&[], &root_shim, &home, &system_dir, &cargo_target, &backup_root);
    assert!(install.status.success(), "stdout={}\nstderr={}", String::from_utf8_lossy(&install.stdout), String::from_utf8_lossy(&install.stderr));
    let backup_dir = the_dated_backup(&backup_root);
    assert!(backup_dir.join("system").is_dir(), "a instalação como root tem de guardar backup da cópia do sistema também");

    // a restauração roda sem root (id != 0) — só o plugin pode voltar aqui.
    let nonroot_shim = tmp.path().join("nonroot-shim");
    fs::create_dir_all(&nonroot_shim).expect("mkdir nonroot shim");
    shim_cargo(&nonroot_shim);
    shim_fixed_output(&nonroot_shim, "id", "1000");

    let restore = run_script(&["--restore", backup_dir.to_str().expect("utf8 path")], &nonroot_shim, &home, &system_dir, &cargo_target, &backup_root);
    assert!(restore.status.success(), "stdout={}\nstderr={}", String::from_utf8_lossy(&restore.stdout), String::from_utf8_lossy(&restore.stderr));

    // o plugin voltou ao original
    assert_eq!(read(&plugin_copy.join("bin/mustard")), "old-mustard");

    // a cópia do sistema NÃO foi mexida por este processo sem root — continua
    // com o que a instalação (como root) tinha trocado, sem voltar ao
    // original sozinha.
    assert_eq!(read(&system_dir.join("bin/mustard")), "built-mustard", "sem root, --restore não pode mexer na cópia do sistema");

    let stdout = String::from_utf8_lossy(&restore.stdout);
    assert!(stdout.contains("sudo"), "sem root, o comando pronto para restaurar o sistema tem de aparecer: {stdout}");
    assert!(stdout.contains("--restore-system-only"), "o comando pronto usa --restore-system-only, sem procurar o plugin nem compilar: {stdout}");
    assert!(stdout.contains(backup_dir.join("system").to_str().expect("utf8 path")), "o comando pronto tem de citar a parte do sistema da pasta datada: {stdout}");
}

/// O comando `--restore-system-only`, impresso pronto com sudo, tem de
/// funcionar de verdade no mesmo ambiente do comando de instalação com sudo:
/// sem HOME real, sem `cargo` no PATH e só quando quem roda é root (forjado
/// aqui por um `id` de mentira que responde 0).
#[test]
#[cfg_attr(not(unix), ignore = "o script é sh")]
fn the_printed_system_restore_command_runs_and_restores_the_system_copy() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let claude_dir = home.join(".claude");
    let system_dir = tmp.path().join("system");
    let cargo_target = tmp.path().join("cargo-target");
    let backup_root = tmp.path().join("backups");

    // instalação forjada como root, para o backup nascer com a parte
    // "system" que este teste vai restaurar.
    let root_shim = tmp.path().join("root-shim");
    fs::create_dir_all(&root_shim).expect("mkdir root shim");
    shim_cargo(&root_shim);
    shim_fixed_output(&root_shim, "id", "0");

    let version = plugin_version();
    seed_plugin_copy(&claude_dir, &version);
    seed_system_copy(&system_dir);

    let install = run_script(&[], &root_shim, &home, &system_dir, &cargo_target, &backup_root);
    assert!(install.status.success(), "stdout={}\nstderr={}", String::from_utf8_lossy(&install.stdout), String::from_utf8_lossy(&install.stderr));
    let backup_dir = the_dated_backup(&backup_root);

    // a restauração roda sem root — só imprime o comando pronto.
    let nonroot_shim = tmp.path().join("nonroot-shim");
    fs::create_dir_all(&nonroot_shim).expect("mkdir nonroot shim");
    shim_cargo(&nonroot_shim);
    shim_fixed_output(&nonroot_shim, "id", "1000");

    let restore = run_script(&["--restore", backup_dir.to_str().expect("utf8 path")], &nonroot_shim, &home, &system_dir, &cargo_target, &backup_root);
    assert!(restore.status.success(), "stdout={}\nstderr={}", String::from_utf8_lossy(&restore.stdout), String::from_utf8_lossy(&restore.stderr));
    let stdout = String::from_utf8_lossy(&restore.stdout).into_owned();
    let sudo_line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("sudo "))
        .unwrap_or_else(|| panic!("sem root, o comando pronto para restaurar o sistema tem de aparecer: {stdout}"));
    let bare_command = sudo_line.trim_start().trim_start_matches("sudo ");

    // Roda o MESMO comando de verdade, sem sudo, com um `id` de mentira que
    // responde 0, sem HOME real e sem cargo no PATH.
    let fake_root_home = tmp.path().join("fake-root-home");
    fs::create_dir_all(&fake_root_home).expect("mkdir fake root home");
    let id_shim = tmp.path().join("id-shim");
    fs::create_dir_all(&id_shim).expect("mkdir id shim");
    shim_fixed_output(&id_shim, "id", "0");

    let result = Command::new("sh")
        .arg("-c")
        .arg(bare_command)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", id_shim.display()))
        .env("HOME", &fake_root_home)
        .output()
        .expect("the printed command runs");
    assert!(result.status.success(), "stdout={}\nstderr={}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));

    assert_eq!(read(&system_dir.join("bin/mustard")), "old-system-mustard", "a cópia do sistema tem de voltar ao original");
    assert_eq!(read(&system_dir.join("bin/mustard-rt")), "old-system-mustard-rt");
    assert_eq!(read(&system_dir.join("bin/scan")), "old-system-scan");
    assert_eq!(read(&system_dir.join("templates/OLD.txt")), "old system templates");
}

/// `--system-copy-only` exige root (linha ~175 do script). Sem essa
/// exigência, quem roda sem privilégio tentaria escrever direto na pasta do
/// sistema (real, fora do teste, dona de root) e ficaria com um erro
/// confuso de permissão, em vez da recusa clara de hoje.
#[test]
#[cfg_attr(not(unix), ignore = "o script é sh")]
fn system_copy_only_refuses_without_root() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).expect("mkdir home");
    let system_dir = tmp.path().join("system");
    let cargo_target = tmp.path().join("cargo-target");
    let backup_dir = tmp.path().join("backups/system-only");
    let shim = tmp.path().join("shim");
    fs::create_dir_all(&shim).expect("mkdir shim");
    shim_cargo(&shim);
    shim_fixed_output(&shim, "id", "1000");

    let release_dir = tmp.path().join("release");
    fs::create_dir_all(&release_dir).expect("mkdir release dir");
    for b in ["mustard", "mustard-rt", "scan"] {
        let path = release_dir.join(b);
        write(&path, "built");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
        }
    }
    seed_system_copy(&system_dir);

    let out = run_script(
        &["--system-copy-only", release_dir.to_str().expect("utf8 path"), backup_dir.to_str().expect("utf8 path")],
        &shim,
        &home,
        &system_dir,
        &cargo_target,
        &tmp.path().join("unused-backup-root"),
    );

    assert!(!out.status.success(), "sem root, --system-copy-only tem de recusar");
    assert_eq!(read(&system_dir.join("bin/mustard")), "old-system-mustard", "recusou antes de tocar na cópia do sistema");
    assert!(!backup_dir.exists(), "recusou antes de criar a pasta de backup");
}

/// A pasta de backup do `--system-copy-only` recusa colisão em vez de
/// sobrescrever (linhas ~180-183). Sem essa recusa, uma segunda rodada com o
/// mesmo destino gravaria por cima do backup da primeira o binário que a
/// primeira já trocou, perdendo o original de verdade.
#[test]
#[cfg_attr(not(unix), ignore = "o script é sh")]
fn system_copy_only_refuses_when_backup_dir_already_exists() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).expect("mkdir home");
    let system_dir = tmp.path().join("system");
    let cargo_target = tmp.path().join("cargo-target");
    let backup_dir = tmp.path().join("backups/system-only");
    let shim = tmp.path().join("shim");
    fs::create_dir_all(&shim).expect("mkdir shim");
    shim_cargo(&shim);
    shim_fixed_output(&shim, "id", "0");

    let release_dir = tmp.path().join("release");
    fs::create_dir_all(&release_dir).expect("mkdir release dir");
    for b in ["mustard", "mustard-rt", "scan"] {
        let path = release_dir.join(b);
        write(&path, "built");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
        }
    }
    seed_system_copy(&system_dir);
    write(&backup_dir.join("bin/mustard"), "backup-de-uma-rodada-anterior");

    let out = run_script(
        &["--system-copy-only", release_dir.to_str().expect("utf8 path"), backup_dir.to_str().expect("utf8 path")],
        &shim,
        &home,
        &system_dir,
        &cargo_target,
        &tmp.path().join("unused-backup-root"),
    );

    assert!(!out.status.success(), "a pasta de backup já existe: tem de recusar em vez de sobrescrever");
    assert_eq!(read(&system_dir.join("bin/mustard")), "old-system-mustard", "recusou antes de trocar a cópia do sistema");
    assert_eq!(read(&backup_dir.join("bin/mustard")), "backup-de-uma-rodada-anterior", "o backup anterior não pode ser sobrescrito");
}

/// Uma cópia do plugin sem selo de versão (`bin/.version`) — por exemplo,
/// uma cópia de desenvolvimento que nunca passou pelo `mustard-boot` —
/// continua sem selo depois do script: ele nunca lê, nunca cria e nunca
/// escreve esse arquivo.
#[test]
#[cfg_attr(not(unix), ignore = "o script é sh")]
fn a_plugin_copy_without_a_version_seal_stays_without_one() {
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
    let plugin_copy = seed_plugin_copy_without_version_seal(&claude_dir, &version);
    seed_system_copy(&system_dir);

    let out = run_script(&[], &shim, &home, &system_dir, &cargo_target, &backup_root);
    assert!(out.status.success(), "stdout={}\nstderr={}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));

    assert!(!plugin_copy.join("bin/.version").exists(), "sem selo antes, o script não pode criar um");
    let backup_dir = the_dated_backup(&backup_root);
    assert!(!backup_dir.join("plugin/bin/.version").exists(), "e não deveria ir para o backup, já que nunca existiu");
}

/// `--system-copy-only` deixa os binários da cópia do sistema de root:root,
/// modo 755, sem escrita para o grupo — mesmo quando o `cargo build` que os
/// gerou deixou o dono de quem compilou e escrita para o grupo (o defeito
/// real: os binários chegam de `cp -p`, que herda dono e modo da origem). O
/// modo é conferido de verdade (qualquer dono troca o próprio modo, sem
/// precisar de root); o dono root:root é conferido pela chamada que o script
/// faz a um `chown` de mentira, já que a máquina de teste não é root de
/// verdade.
#[test]
#[cfg_attr(not(unix), ignore = "o script é sh")]
fn the_system_copy_is_owned_by_root() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).expect("mkdir home");
    let system_dir = tmp.path().join("system");
    let cargo_target = tmp.path().join("cargo-target");
    let backup_dir = tmp.path().join("backups/system-only");
    let shim = tmp.path().join("shim");
    fs::create_dir_all(&shim).expect("mkdir shim");
    shim_fixed_output(&shim, "id", "0");
    let chown_log = tmp.path().join("chown.log");
    shim_logging_chown(&shim, &chown_log);

    let release_dir = tmp.path().join("release");
    fs::create_dir_all(&release_dir).expect("mkdir release dir");
    for b in ["mustard", "mustard-rt", "scan"] {
        let path = release_dir.join(b);
        write(&path, "built");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            // O dono de quem compilou, com escrita para o grupo — exatamente
            // o defeito que este teste prova que o script corrige.
            fs::set_permissions(&path, fs::Permissions::from_mode(0o775)).expect("chmod release bin");
        }
    }
    seed_system_copy(&system_dir);

    let real_path = std::env::var("PATH").unwrap_or_default();
    let out = Command::new("sh")
        .arg(repo_root().join("scripts/dev-install.sh"))
        .args(["--system-copy-only", release_dir.to_str().expect("utf8 path"), backup_dir.to_str().expect("utf8 path")])
        .env_clear()
        .env("PATH", format!("{}:{real_path}", shim.display()))
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("CARGO_TARGET_DIR", &cargo_target)
        .env("MUSTARD_DEV_INSTALL_SYSTEM_DIR", &system_dir)
        .output()
        .expect("the script runs");
    assert!(out.status.success(), "stdout={}\nstderr={}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));

    // O modo — prova real, sem simulação: qualquer dono troca o modo do
    // próprio arquivo, sem precisar de root.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        for b in ["mustard", "mustard-rt", "scan"] {
            let path = system_dir.join("bin").join(b);
            let mode = fs::metadata(&path).unwrap_or_else(|e| panic!("stat {}: {e}", path.display())).permissions().mode() & 0o777;
            assert_eq!(mode, 0o755, "{b} tem de ficar 755, sem escrita para o grupo (ficou {mode:o})");
        }
    }

    // O dono — a máquina de teste não é root de verdade, então a prova é a
    // chamada que o script fez ao `chown` de mentira: ele tem de pedir
    // root:root para os três binários e para os moldes.
    let log = read(&chown_log);
    for b in ["mustard", "mustard-rt", "scan"] {
        let expected = format!("root:root {}", system_dir.join("bin").join(b).display());
        assert!(log.contains(&expected), "o script tem de deixar {b} de root:root: log={log}");
    }
    let templates_line = format!("-R root:root {}", system_dir.join("templates").display());
    assert!(log.contains(&templates_line), "os moldes da cópia do sistema também têm de ficar de root:root: log={log}");
}

/// A pasta datada do `--system-copy-only` nasce de root (só roda com sudo)
/// dentro da pasta pessoal de quem chamou; sem devolver o dono das PASTAS a
/// essa pessoa, ela não apaga o próprio backup depois sem sudo de novo. A
/// prova é a chamada que o script faz ao `chown` de mentira, com o par
/// `SUDO_UID:SUDO_GID` que o `sudo` real preenche — a máquina de teste não é
/// root, então não há como conferir a posse de verdade.
#[test]
#[cfg_attr(not(unix), ignore = "o script é sh")]
fn the_system_backup_belongs_to_the_caller() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).expect("mkdir home");
    let system_dir = tmp.path().join("system");
    let cargo_target = tmp.path().join("cargo-target");
    let backup_dir = tmp.path().join("backups/system-only");
    let shim = tmp.path().join("shim");
    fs::create_dir_all(&shim).expect("mkdir shim");
    shim_fixed_output(&shim, "id", "0");
    let chown_log = tmp.path().join("chown.log");
    shim_logging_chown(&shim, &chown_log);

    let release_dir = tmp.path().join("release");
    fs::create_dir_all(&release_dir).expect("mkdir release dir");
    for b in ["mustard", "mustard-rt", "scan"] {
        let path = release_dir.join(b);
        write(&path, "built");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod release bin");
        }
    }
    seed_system_copy(&system_dir);

    let real_path = std::env::var("PATH").unwrap_or_default();
    let out = Command::new("sh")
        .arg(repo_root().join("scripts/dev-install.sh"))
        .args(["--system-copy-only", release_dir.to_str().expect("utf8 path"), backup_dir.to_str().expect("utf8 path")])
        .env_clear()
        .env("PATH", format!("{}:{real_path}", shim.display()))
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("CARGO_TARGET_DIR", &cargo_target)
        .env("MUSTARD_DEV_INSTALL_SYSTEM_DIR", &system_dir)
        // o `sudo` real preenche estas duas ao rodar como root em nome de
        // outra conta — é delas que o script tem de ler quem chamou.
        .env("SUDO_UID", "4242")
        .env("SUDO_GID", "4343")
        .output()
        .expect("the script runs");
    assert!(out.status.success(), "stdout={}\nstderr={}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));

    let log = read(&chown_log);
    assert!(
        log.contains("4242:4343") && log.contains(backup_dir.to_str().expect("utf8 path")),
        "o script tem de devolver as pastas do backup a quem chamou o sudo (SUDO_UID:SUDO_GID): log={log}"
    );
}
