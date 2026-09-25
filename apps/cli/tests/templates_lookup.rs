//! O instalador não procura pasta de moldes: tudo o que ele grava no projeto
//! vem compilado no programa.
//!
//! O teste roda o programa de verdade, copiado para uma pasta onde não há
//! `templates/` ao lado dele nem um nível acima, com o ambiente limpo: sem a
//! variável que apontava a pasta de moldes e sem `CARGO_MANIFEST_DIR`. Assim
//! nenhum dos caminhos por onde a busca antiga achava moldes está disponível,
//! e o `init --yes` num projeto git vazio precisa terminar com sucesso.
//!
//! Roda só no unix: o `rtk` e o `rg` do caminho de busca são scripts de shell.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

/// Um projeto novo, com git e nada mais.
fn project(root: &Path) -> PathBuf {
    let project = root.join("project");
    fs::create_dir_all(&project).expect("mkdir project");
    let ok = Command::new("git").args(["init", "-q", "."]).current_dir(&project).status().expect("git init");
    assert!(ok.success(), "git init failed");
    project
}

/// O programa compilado, copiado para uma pasta própria: nem ao lado dele nem
/// um nível acima existe `templates/`.
fn program_alone(root: &Path) -> PathBuf {
    let dir = root.join("programa").join("bin");
    fs::create_dir_all(&dir).expect("mkdir program dir");
    let program = dir.join(format!("mustard{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_mustard"), &program).expect("copy the program");
    for beside in [dir.join("templates"), dir.join("..").join("templates")] {
        assert!(!beside.exists(), "fixture broken: {} exists", beside.display());
    }
    program
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// `mustard init --yes` num projeto git vazio, sem nenhuma pasta de moldes
/// ao alcance, instala e não mexe na configuração do git.
#[test]
#[cfg_attr(not(unix), ignore = "the rtk and rg shims are shell scripts")]
fn init_runs_in_an_empty_project_without_any_folder_of_molds() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let bin = tools(root);
    let home = root.join("home");
    fs::create_dir_all(&home).expect("mkdir home");
    let project = project(root);
    let program = program_alone(root);
    let config_before = fs::read_to_string(project.join(".git/config")).expect("read git config");

    let out = Command::new(&program)
        .args(["init", "--yes"])
        .current_dir(&project)
        .env_clear()
        .env("PATH", &bin)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .output()
        .expect("the mustard binary runs");

    assert!(
        out.status.success(),
        "init without molds must install:\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&out.stdout),
        stderr(&out),
    );
    assert!(
        project.join(".claude").join("settings.local.json").is_file(),
        "the install seeds .claude/settings.local.json",
    );
    assert!(project.join("mustard.json").is_file(), "the install writes mustard.json");
    assert!(!project.join(".github").exists(), "the install seeds nothing under .github/");
    assert_eq!(
        fs::read_to_string(project.join(".git/config")).expect("read git config"),
        config_before,
        "the installer only reads the git configuration",
    );
}
