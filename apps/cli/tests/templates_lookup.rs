//! Onde o instalador acha a pasta `templates/` quando não há moldes ao lado
//! do programa: na pasta do pacote que o cargo entrega ao rodar
//! (`CARGO_MANIFEST_DIR`, lida na hora), nunca na pasta gravada na
//! compilação.
//!
//! A pasta de compilação passa de uma cópia do projeto para outra, e o cargo
//! reaproveita o programa já compilado quando o código não mudou. Um endereço
//! gravado na compilação seria o da cópia que compilou — outra cópia, ou uma
//! já apagada. Por isso os testes rodam o programa de verdade, compilado
//! nesta cópia, com a variável apontando para outra pasta: a cópia que
//! compilou continua aqui, com os moldes dela, e mesmo assim não é lida.
//!
//! Os testes rodam só no unix: o `rtk` e o `rg` do caminho de busca são
//! scripts de shell.

#[path = "support/manifest_dir.rs"]
mod manifest_dir;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// O molde que o instalador copia de `templates/`.
const INSTALLER_MOLD: &str = ".github/pull_request_template.md";

/// Uma pasta de caminho de busca onde `rtk` e `rg` respondem, para a trava
/// do instalador deixar passar, e o `git` é o de verdade.
fn tools(root: &Path) -> PathBuf {
    let dir = root.join("bin");
    fs::create_dir_all(&dir).expect("mkdir bin");
    for tool in ["rtk", "rg"] {
        let path = dir.join(tool);
        fs::write(&path, "#!/bin/sh\nexit 0\n").expect("write shim");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod shim");
        }
    }
    #[cfg(unix)]
    {
        let real_git = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .expect("git on PATH");
        std::os::unix::fs::symlink(real_git, dir.join("git")).expect("link git");
    }
    dir
}

/// Um projeto novo, com git, onde o instalador aceita rodar.
fn project(root: &Path) -> PathBuf {
    let project = root.join("project");
    fs::create_dir_all(&project).expect("mkdir project");
    let ok = Command::new("git").args(["init", "-q", "."]).current_dir(&project).status().expect("git init");
    assert!(ok.success(), "git init failed");
    project
}

/// Uma pasta de pacote de outra cópia: com `templates/` e, quando pedido, com
/// o molde do instalador dentro.
fn other_package(root: &Path, name: &str, with_mold: bool) -> PathBuf {
    let package = root.join(name);
    let templates = package.join("templates");
    fs::create_dir_all(&templates).expect("mkdir templates");
    if with_mold {
        let mold = templates.join(INSTALLER_MOLD);
        fs::create_dir_all(mold.parent().expect("the mold has a folder")).expect("mkdir .github");
        fs::write(mold, "## O que mudou\n").expect("write mold");
    }
    package
}

/// Roda `mustard init --yes --dry-run` com o ambiente limpo: sem
/// `MUSTARD_TEMPLATES_DIR`, e com `CARGO_MANIFEST_DIR` só quando `package`
/// é dado.
fn dry_run(root: &Path, package: Option<&Path>) -> Output {
    let home = root.join("home");
    fs::create_dir_all(&home).expect("mkdir home");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mustard"));
    cmd.args(["init", "--yes", "--dry-run"])
        .current_dir(project(root))
        .env_clear()
        .env("PATH", tools(root))
        .env("HOME", &home)
        .env("USERPROFILE", &home);
    if let Some(package) = package {
        cmd.env("CARGO_MANIFEST_DIR", package);
    }
    cmd.output().expect("the mustard binary runs")
}

/// A condição de todo teste daqui: nada de `templates/` ao lado do programa,
/// e a cópia que compilou com os moldes do instalador no lugar.
fn assert_fixture() {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_mustard"));
    let exe_dir = exe.parent().expect("the binary has a folder");
    assert!(
        !exe_dir.join("templates").exists() && !exe_dir.join("../templates").exists(),
        "fixture broken: there is a templates/ beside {}",
        exe.display(),
    );
    assert!(
        manifest_dir::manifest_dir().join("templates").join(INSTALLER_MOLD).is_file(),
        "fixture broken: this copy must hold the installer's mold, or refusing proves nothing",
    );
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Compilado nesta cópia e rodado com a pasta do pacote de outra: com o
/// molde lá, instala; com `templates/` lá sem o molde, recusa e nomeia a
/// pasta da outra. Os moldes desta cópia não salvam a segunda rodada.
#[test]
#[cfg_attr(not(unix), ignore = "the shims are shell scripts")]
fn the_package_folder_given_at_run_time_decides_where_the_molds_come_from() {
    assert_fixture();
    let tmp = tempfile::tempdir().expect("tempdir");

    let with_mold = other_package(tmp.path(), "other-copy", true);
    let out = dry_run(&tmp.path().join("first"), Some(&with_mold));
    assert!(out.status.success(), "the other copy holds the mold, the install must run:\n{}", stderr(&out));

    let without_mold = other_package(tmp.path(), "unrelated-package", false);
    let out = dry_run(&tmp.path().join("second"), Some(&without_mold));
    let err = stderr(&out);
    assert!(!out.status.success(), "a templates/ without the installer's mold must be refused");
    assert!(err.contains(&without_mold.join("templates").display().to_string()), "the refusal names what it probed:\n{err}");
    assert!(err.contains("MUSTARD_TEMPLATES_DIR"), "the refusal points at the override:\n{err}");
}

/// Sem a variável e sem moldes ao lado do programa, o instalador recusa —
/// mesmo com a cópia que o compilou ainda aqui, com os moldes dela.
#[test]
#[cfg_attr(not(unix), ignore = "the shims are shell scripts")]
fn without_the_variable_and_without_molds_beside_it_the_installer_refuses() {
    assert_fixture();
    let tmp = tempfile::tempdir().expect("tempdir");

    let out = dry_run(tmp.path(), None);
    let err = stderr(&out);
    assert!(!out.status.success(), "no templates/ anywhere it may look: the install must refuse");
    assert!(err.contains("MUSTARD_TEMPLATES_DIR"), "the refusal points at the override:\n{err}");
    let compiled_in = manifest_dir::manifest_dir().display().to_string();
    assert!(!err.contains(&compiled_in), "the refusal cites the copy that compiled the binary:\n{err}");
}
