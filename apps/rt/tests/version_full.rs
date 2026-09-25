//! `apps/rt/build.rs` monta `MUSTARD_VERSION_FULL` — a linha que
//! `mustard-rt --version` mostra — a partir do `git` do repositório onde ele
//! compila. O pacote Linux (`packaging/linux/build-deb.sh`) copia o código
//! para uma pasta de build SEM a pasta `.git` antes de compilar; ali o `git`
//! não acha o commit, e a versão sairia só com o número. Por isso o script
//! lê o commit, a marca de mudança (dirty) e a data no repositório ORIGINAL,
//! antes da cópia, e os entrega ao build por variável de ambiente
//! (`MUSTARD_GIT_HASH`, `MUSTARD_GIT_DIRTY`, `MUSTARD_GIT_DATE`) — que
//! `git_describe`, em `apps/rt/build.rs`, lê antes de tentar o `git`.
//!
//! Para provar isso sem compilar o `mustard-rt` inteiro (todas as suas
//! dependências), o `build.rs` — um programa Rust comum, sem dependências
//! além da std — é compilado sozinho com `rustc`, do jeito que o cargo já faz
//! de verdade com todo script de build, e rodado com o diretório de trabalho
//! fora de qualquer repositório git, para as chamadas internas a `git`
//! falharem de propósito, como aconteceria dentro da pasta de build do
//! pacote Linux. A prova é a linha `cargo:rustc-env=MUSTARD_VERSION_FULL=…`
//! que ele imprime — a mesma linha que o cargo lê para gravar a variável de
//! compilação que os binários embutem com `env!(...)`.

#[path = "support/manifest_dir.rs"]
mod manifest_dir;

use std::path::{Path, PathBuf};
use std::process::Command;

/// A raiz do repositório, a partir deste crate (`apps/rt`).
fn repo_root() -> PathBuf {
    manifest_dir::manifest_dir()
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

/// Compila `apps/rt/build.rs` sozinho — sem depender do resto do workspace,
/// do mesmo jeito que o cargo compila todo script de build antes de rodá-lo.
fn compile_build_rs(out_bin: &Path) {
    let build_rs = repo_root().join("apps/rt/build.rs");
    let out = Command::new("rustc")
        .arg(&build_rs)
        .arg("-o")
        .arg(out_bin)
        .output()
        .expect("rustc runs");
    assert!(out.status.success(), "rustc não compilou {}: {}", build_rs.display(), String::from_utf8_lossy(&out.stderr));
}

/// Roda o `build.rs` já compilado, com o diretório de trabalho fora de
/// qualquer repositório git (`/tmp`, sem `.git` acima) — para as chamadas a
/// `git` de dentro dele falharem de propósito, como falhariam na pasta de
/// build do pacote Linux, que não tem `.git`. Devolve a linha
/// `cargo:rustc-env=MUSTARD_VERSION_FULL=…` que ele imprime.
fn run_build_rs_without_git(bin: &Path, cwd: &Path, extra_env: &[(&str, &str)]) -> String {
    let mut cmd = Command::new(bin);
    cmd.current_dir(cwd).env_clear().env("CARGO_PKG_VERSION", "9.9.9");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("the compiled build.rs runs");
    assert!(out.status.success(), "build.rs saiu com erro: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    stdout
        .lines()
        .find(|l| l.starts_with("cargo:rustc-env=MUSTARD_VERSION_FULL="))
        .unwrap_or_else(|| panic!("sem a linha MUSTARD_VERSION_FULL: {stdout}"))
        .trim_start_matches("cargo:rustc-env=MUSTARD_VERSION_FULL=")
        .to_string()
}

/// Sem `.git` e sem as variáveis: a versão sai só com o número, como hoje —
/// o `git` de dentro do build.rs não acha nada nesse diretório, e não há
/// variável para substituir.
#[test]
fn without_the_environment_and_without_git_the_version_is_just_the_number() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bin = tmp.path().join("build_rs_bin");
    compile_build_rs(&bin);
    let empty_cwd = tmp.path().join("no-git-here");
    std::fs::create_dir_all(&empty_cwd).expect("mkdir");

    let full = run_build_rs_without_git(&bin, &empty_cwd, &[]);
    assert_eq!(full, "9.9.9", "sem git e sem variável, a versão tem de ficar só com o número: {full}");
}

/// Com `.git` ausente (o cenário do pacote Linux) mas com o commit, a marca
/// de mudança e a data entregues por variável de ambiente, a versão mostra o
/// commit — exatamente o que `packaging/linux/build-deb.sh` passa a fazer.
#[test]
fn the_version_shows_the_commit_from_the_environment() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bin = tmp.path().join("build_rs_bin");
    compile_build_rs(&bin);
    let empty_cwd = tmp.path().join("no-git-here");
    std::fs::create_dir_all(&empty_cwd).expect("mkdir");

    let full = run_build_rs_without_git(
        &bin,
        &empty_cwd,
        &[("MUSTARD_GIT_HASH", "deadbeef1234"), ("MUSTARD_GIT_DIRTY", "1"), ("MUSTARD_GIT_DATE", "2026-01-02")],
    );
    assert_eq!(full, "9.9.9 (build dev, gdeadbeef1234-dirty 2026-01-02)", "a versão tem de trazer o commit da variável de ambiente: {full}");
}

/// O `git` em `dir`, com quem comita já dito, e a saída sem as bordas.
fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Um projeto git em `root` com dois pacotes, cada um com um dos dois
/// scripts de build de verdade — o do `mustard-rt` e o do `mustard` — e um
/// binário que só imprime a versão que o script carimbou.
fn stamped_project(root: &Path) {
    let write = |path: &str, body: &str| {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("a parent folder")).expect("mkdir");
        std::fs::write(path, body).expect("write");
    };
    write("Cargo.toml", "[workspace]\nmembers = [\"apps/rt\", \"apps/cli\"]\nresolver = \"2\"\n");
    for (crate_dir, name) in [("apps/rt", "carimbo-rt"), ("apps/cli", "carimbo-cli")] {
        let manifest = format!("[package]\nname = \"{name}\"\nversion = \"9.9.9\"\nedition = \"2021\"\n");
        write(&format!("{crate_dir}/Cargo.toml"), &manifest);
        let script = std::fs::read_to_string(repo_root().join(crate_dir).join("build.rs")).expect("the build script");
        write(&format!("{crate_dir}/build.rs"), &script);
        write(&format!("{crate_dir}/src/main.rs"), "fn main() {\n    println!(\"{}\", env!(\"MUSTARD_VERSION_FULL\"));\n}\n");
    }
    write("plugin/hooks/hooks.json", "{}\n");
    write(".gitignore", "target/\nCargo.lock\n");
    write("LEIAME.md", "um\n");
    git_in(root, &["init", "-q"]);
    git_in(root, &["add", "-A"]);
    git_in(root, &["commit", "-q", "-m", "primeiro"]);
}

/// Compila o projeto de teste na cópia `copy`, com a pasta de compilação
/// dentro dela, pelo mesmo cargo que roda os testes, e devolve a versão que
/// cada binário imprime.
fn build_and_read_versions(copy: &Path) -> Vec<String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut build = Command::new(cargo);
    build.args(["build", "--offline", "--quiet"]).current_dir(copy).env("CARGO_TARGET_DIR", copy.join("target"));
    for var in ["MUSTARD_BUILD_NUMBER", "MUSTARD_GIT_HASH", "MUSTARD_GIT_DIRTY", "MUSTARD_GIT_DATE", "GIT_DIR", "GIT_WORK_TREE"] {
        build.env_remove(var);
    }
    let out = build.output().expect("cargo runs");
    assert!(out.status.success(), "cargo build: {}", String::from_utf8_lossy(&out.stderr));
    ["carimbo-rt", "carimbo-cli"]
        .iter()
        .map(|name| {
            let bin = copy.join("target").join("debug").join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
            let out = Command::new(&bin).output().expect("the stamped binary runs");
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        })
        .collect()
}

/// Na cópia ligada ao projeto (`git worktree add`), com a compilação dentro
/// dela, a versão acompanha o commit da cópia: compilada num commit, levada a
/// outro com `checkout --detach` — que só muda o arquivo que o commit mudou,
/// e nenhum dos pacotes — e compilada de novo, a versão dos dois binários
/// mostra o commit novo.
#[test]
fn a_linked_copy_moved_to_another_commit_stamps_the_new_commit() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("projeto");
    std::fs::create_dir_all(&root).expect("mkdir");
    stamped_project(&root);
    let copy = tmp.path().join("vaga");
    git_in(&root, &["worktree", "add", "-q", "--detach", &copy.to_string_lossy(), "HEAD"]);

    let first = git_in(&copy, &["rev-parse", "--short=12", "HEAD"]);
    for version in build_and_read_versions(&copy) {
        assert!(version.contains(&format!("g{first} ")), "a primeira compilação carimba o commit da cópia: {version}");
    }

    std::fs::write(root.join("LEIAME.md"), "dois\n").expect("write");
    git_in(&root, &["commit", "-q", "-am", "segundo"]);
    let second = git_in(&root, &["rev-parse", "--short=12", "HEAD"]);
    git_in(&copy, &["checkout", "-q", "--detach", "--force", &second]);

    for version in build_and_read_versions(&copy) {
        assert!(version.contains(&format!("g{second} ")), "a cópia levada a outro commit carimba o novo: {version}");
    }
}
