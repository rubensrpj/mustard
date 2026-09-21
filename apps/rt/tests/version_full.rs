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

use std::path::{Path, PathBuf};
use std::process::Command;

/// A raiz do repositório, a partir deste crate (`apps/rt`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
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
