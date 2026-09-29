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

/// Roda `mustard-rt run clean <args>` com o temp apontado para `temp`,
/// a partir de `cwd`.
fn scratch_gc(cwd: &Path, temp: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "clean"])
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
fn clean_path_exit_codes() {
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
        .args(["run", "clean", "--path", copy.to_str().unwrap()])
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

/// `git` em `dir`, com identidade de teste; o comando tem de passar.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// A obra `spec` do projeto `root`, levada passo a passo do fluxo até a fase
/// `phase`.
fn work(root: &Path, spec: &str, phase: &str) {
    const STEPS: &[&str] = &["survey", "plan", "approved", "running", "closed"];
    let path = mustard_core::io::spec_events::spec_file(root, spec).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    for step in STEPS {
        let mut fields = serde_json::json!({"phase": step, "author": "binary"});
        if *step == "survey" {
            fields["branch"] = serde_json::json!(format!("feature/{spec}"));
            fields["base"] = serde_json::json!("dev");
        }
        if *step == "approved" {
            fields["witness"] = serde_json::json!({"question": "Aprovar?", "answer": "Aprovar"});
        }
        mustard_core::io::spec_events::write(&path, "state", fields.as_object().cloned().unwrap(), &[]).unwrap();
        if *step == phase {
            return;
        }
    }
}

/// Na pasta de um projeto, a limpeza lista a cópia da obra fechada e deixa a
/// da obra aberta, com o motivo; sem a opção de apagar, nada sai. Com
/// `--apply`, a da obra fechada sai — pasta e registro no git — e a da obra
/// aberta, a pasta principal e o que ela compilou ficam.
#[test]
fn clean_in_a_project_removes_the_copy_of_a_closed_work_and_keeps_the_open_one() {
    let base = tempfile::tempdir().unwrap();
    let temp = base.path().join("tmp");
    let home = base.path().join("home");
    let root = base.path().join("obra");
    let copies_base = base.path().join("copias");
    for dir in [&temp, &home, &root] {
        fs::create_dir_all(dir).unwrap();
    }
    git(&root, &["init", "-q", "."]);
    fs::write(root.join("mustard.json"), b"{}").unwrap();
    fs::write(root.join(".gitignore"), "target/\n.claude/\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "semente"]);
    let built = root.join("target").join("debug").join("mustard");
    fs::create_dir_all(built.parent().unwrap()).unwrap();
    fs::write(&built, "compilado").unwrap();
    work(&root, "fechada", "closed");
    work(&root, "aberta", "running");
    // As cópias moram sob a pasta-base que o teste dá ao binário, dentro da
    // pasta temporária do teste; a variável do ambiente de quem roda o teste
    // não entra na conta. A pasta do projeto sob a base é a que o binário
    // monta, e o nome dela não depende da base.
    let copies = copies_base.join(mustard_core::io::wave_prompt::copies_dir(&root).file_name().unwrap());
    let closed = copies.join("fechada").join(mustard_core::io::wave_prompt::slot_name(0));
    let open = copies.join("aberta").join(mustard_core::io::wave_prompt::slot_name(0));
    for slot in [&closed, &open] {
        git(&root, &["worktree", "add", "-q", "--detach", &slot.to_string_lossy()]);
        fs::create_dir_all(slot.join("target")).unwrap();
        fs::write(slot.join("target").join("compilado"), "x").unwrap();
    }
    let clean = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(["run", "clean"])
            .args(args)
            .current_dir(&root)
            .env("TMPDIR", &temp)
            .env("TMP", &temp)
            .env("TEMP", &temp)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("MUSTARD_COPIES_DIR", &copies_base)
            .env("MUSTARD_SESSION_ID", "scratch-gc-copies-test")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let shown = |path: &Path| mustard_core::io::wave_prompt::shown(path);
    let spec_folder = |spec: &str| shown(&copies.join(spec));

    let listed = clean(&[]);
    let section = &listed["copies"];
    assert_eq!(section["candidates"][0]["path"], serde_json::json!(spec_folder("fechada")), "{listed}");
    assert_eq!(section["candidates"][0]["reason"], serde_json::json!("spec closed"), "{listed}");
    assert_eq!(section["kept"][0]["path"], serde_json::json!(spec_folder("aberta")), "{listed}");
    assert_eq!(section["kept"][0]["reason"], serde_json::json!("spec still open: running"), "{listed}");
    assert!(closed.join("target").join("compilado").is_file(), "sem a opção de apagar, nada sai");

    let applied = clean(&["--apply"]);
    assert_eq!(applied["copies"]["removed"], serde_json::json!([spec_folder("fechada")]), "{applied}");
    assert!(!copies.join("fechada").exists(), "a cópia da obra fechada saiu: {applied}");
    assert!(!git(&root, &["worktree", "list", "--porcelain"]).contains(&shown(&closed)), "{applied}");
    assert!(open.join("target").join("compilado").is_file(), "a cópia da obra aberta fica");
    assert_eq!(fs::read_to_string(&built).unwrap(), "compilado", "a compilação principal fica");
    assert!(root.join("mustard.json").is_file() && root.join(".git").is_dir(), "a pasta principal fica");
}
