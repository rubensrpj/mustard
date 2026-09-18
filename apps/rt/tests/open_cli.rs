//! `mustard-rt run open` pelo binário: um passo que pergunta de volta sai com
//! exit 0, a recusa com exit 1, e a abertura cria a branch e a spec com o
//! nome exato.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(ok, "git {args:?} failed");
}

/// Um repositório com `main` e `dev`, as bases declaradas e um arquivo de
/// código, parado em `dev`. O Mustard fica fora do git.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["checkout", "-q", "-b", "main"]);
    std::fs::write(root.join(".git").join("info").join("exclude"), ".claude/\nmustard.json\n").expect("exclude");
    std::fs::write(root.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#).expect("config");
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("src").join("main.rs"), "fn main() {}\n").expect("code");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "init"]);
    git(root, &["checkout", "-q", "-b", "dev"]);
    dir
}

fn open(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .arg("run")
        .arg("open")
        .args(args)
        .arg("--root")
        .arg(root)
        .current_dir(root)
        .output()
        .expect("run open")
}

fn report(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).expect("one JSON report")
}

#[test]
fn open_answers_a_step_with_exit_0_a_refusal_with_exit_1_and_opens_the_spec() {
    let dir = repo();
    let root = dir.path();

    let step = open(root, &["--kind", "feature", "--name", "trava de pendências", "--base", "dev"]);
    assert_eq!(step.status.code(), Some(0));
    assert_eq!(report(&step)["step"], "confirm_name");

    let refused = open(root, &["--kind", "feature", "--name", "x", "--base", "nao-existe"]);
    assert_eq!(refused.status.code(), Some(1));
    assert_eq!(report(&refused)["reason"], "base-not-found");

    let opened = open(root, &["--name", "feature/trava-de-pendencias", "--base", "dev"]);
    assert_eq!(opened.status.code(), Some(0), "{}", String::from_utf8_lossy(&opened.stdout));
    let opened = report(&opened);
    assert_eq!(opened["branch"], "feature/trava-de-pendencias");
    assert_eq!(opened["step"], "ask_goal");
    assert!(root.join(".claude").join("spec").join("trava-de-pendencias").join("spec.ndjson").is_file());
}
