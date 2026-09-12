// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! `scratch-gc` apaga sem volta, então o contrato de saída é travado no
//! binário, não só na função: `--path` recusado sai com 1 e não toca em nada;
//! `--dry-run --path` é recusado pelo parser antes de qualquer exclusão (era o
//! defeito: a pasta sumia com `"dry_run": false`); `--path` válido apaga e sai
//! com 0.
//!
//! O temp do binário é um temp falso, para nenhuma pasta real da máquina entrar
//! no alcance do teste. `std::env::temp_dir()` lê `TMPDIR` no Unix e `TMP`/
//! `TEMP` no Windows, então as três apontam para ele.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

/// Uma cópia deste projeto em `dir`: `Cargo.toml` + `apps/rt`.
fn project_copy(dir: &Path) {
    fs::create_dir_all(dir.join("apps").join("rt")).unwrap();
    fs::write(dir.join("Cargo.toml"), "[workspace]\n").unwrap();
}

/// Roda `mustard-rt run scratch-gc <args>` com o temp apontado para `temp`,
/// a partir de `cwd`.
fn scratch_gc(cwd: &Path, temp: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "scratch-gc"])
        .args(args)
        .current_dir(cwd)
        .env("TMPDIR", temp)
        .env("TMP", temp)
        .env("TEMP", temp)
        .env("MUSTARD_SESSION_ID", "scratch-gc-exit-test")
        .output()
        .unwrap()
}

#[test]
fn scratch_gc_path_exit_codes() {
    let base = tempfile::tempdir().unwrap();
    let temp = base.path().join("tmp");
    fs::create_dir_all(&temp).unwrap();
    let cwd = base.path().join("cwd");
    fs::create_dir_all(&cwd).unwrap();

    // Fora do temp: recusado, exit 1, nada tocado.
    let repo = base.path().join("repo");
    project_copy(&repo);
    let out = scratch_gc(&cwd, &temp, &["--path", repo.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stderr).contains("outside the temp directory"));
    assert!(repo.join("Cargo.toml").exists(), "the repository is untouched");

    // `--dry-run --path`: o parser recusa, nada é apagado.
    let copy = temp.join("tmp.copy");
    project_copy(&copy);
    let out = scratch_gc(&cwd, &temp, &["--dry-run", "--path", copy.to_str().unwrap()]);
    assert!(!out.status.success(), "--dry-run --path must be refused");
    assert_ne!(out.status.code(), Some(1), "refused by the parser, not by the door");
    assert!(copy.join("Cargo.toml").exists(), "--dry-run never deletes");

    // Temp que é a própria home (`TMPDIR=$HOME`): recusado, exit 1, nada tocado.
    let out = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "scratch-gc", "--path", copy.to_str().unwrap()])
        .current_dir(&cwd)
        .env("TMPDIR", &temp)
        .env("HOME", &temp)
        .env("USERPROFILE", &temp)
        .env("TMP", &temp)
        .env("TEMP", &temp)
        .env("MUSTARD_SESSION_ID", "scratch-gc-exit-test")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stderr).contains("home directory"));
    assert!(copy.join("Cargo.toml").exists(), "a temp at the home protects nothing, so nothing goes");

    // `--path` válido dentro do temp: apaga, exit 0.
    let out = scratch_gc(&cwd, &temp, &["--path", copy.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(!copy.exists(), "a checked scratch copy is removed");
}
