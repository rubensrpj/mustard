//! Two passes of the scan over a git project, with one file changed between
//! them: the second pass reads only that file, the map it writes is the same
//! one a pass reading every file gives, and neither pass writes to git or to
//! any `CLAUDE.md`. The project carries the exclude rules a Mustard install
//! writes, so the map stays out of git.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.email=scan@example.com", "-c", "user.name=scan", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn model_of(dir: &Path) -> PathBuf {
    dir.join(".claude").join("grain.model.json")
}

fn scan(dir: &Path, extra: &[&str]) -> Value {
    let model = model_of(dir);
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["scan", dir.to_str().unwrap(), "--out", model.to_str().unwrap(), "--json"])
        .args(extra)
        .output()
        .expect("run scan");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.lines().last().unwrap_or("{}")).expect("the report is one JSON line")
}

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

#[test]
fn a_second_pass_reads_only_the_changed_file_and_leaves_git_clean() {
    let temp = tempfile::Builder::new().prefix("scan-incremental-").tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    git(&dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();

    write(&dir, "Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
    write(&dir, "src/lib.rs", "pub mod a;\npub mod b;\n");
    write(&dir, "src/a.rs", "pub fn alpha() -> u32 {\n    1\n}\n");
    write(&dir, "src/b.rs", "use crate::a::alpha;\npub fn beta() -> u32 {\n    alpha() + 1\n}\n");
    write(&dir, "CLAUDE.md", "# Demo\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "first"]);

    let first = scan(&dir, &[]);
    assert_eq!(first["full"], json!(true), "{first}");
    for file in ["src/a.rs", "src/b.rs", "src/lib.rs"] {
        assert!(first["read"].as_array().unwrap().contains(&json!(file)), "{file} in {first}");
    }

    // One file changes and is committed: the next pass reads only it.
    write(&dir, "src/b.rs", "use crate::a::alpha;\npub fn beta() -> u32 {\n    alpha() + 2\n}\npub fn gamma() {}\n");
    git(&dir, &["commit", "-q", "-am", "second"]);
    let second = scan(&dir, &[]);
    assert_eq!(second["full"], json!(false), "{second}");
    assert_eq!(second["read"], json!(["src/b.rs"]), "{second}");

    // Neither pass wrote to git or to the CLAUDE.md.
    assert_eq!(git(&dir, &["status", "--porcelain"]), "", "the map stays out of git");
    assert_eq!(git(&dir, &["rev-list", "--count", "HEAD"]).trim(), "2", "the scan never commits");
    assert_eq!(std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap(), "# Demo\n");

    // The map read in steps is the map read at once.
    let stepped = std::fs::read(model_of(&dir)).unwrap();
    assert_eq!(scan(&dir, &["--all"])["full"], json!(true));
    assert_eq!(std::fs::read(model_of(&dir)).unwrap(), stepped, "reading only what changed gives the same map");

    // A change not committed is read, and read again once it is undone.
    let original = std::fs::read_to_string(dir.join("src/a.rs")).unwrap();
    write(&dir, "src/a.rs", "pub fn alpha() -> u32 {\n    7\n}\n");
    assert_eq!(scan(&dir, &[])["read"], json!(["src/a.rs"]));
    write(&dir, "src/a.rs", &original);
    assert_eq!(scan(&dir, &[])["read"], json!(["src/a.rs"]), "a file put back is read again");
    let undone = std::fs::read(model_of(&dir)).unwrap();
    scan(&dir, &["--all"]);
    assert_eq!(std::fs::read(model_of(&dir)).unwrap(), undone);

    // The map keeps the history and what each file imports.
    let model: Value = serde_json::from_slice(&undone).unwrap();
    assert_eq!(model["history"]["commits"].as_array().unwrap().len(), 2, "{}", model["history"]);
    // And it keeps the named edges between declarations: this last pass read
    // only `src/a.rs`, so the use of `alpha` inside `src/b.rs` came from the
    // call sites the unread file carries, not from reading it again.
    let alpha = model["modules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["path"] == json!("src/a.rs"))
        .map(|m| m["declarations"][0].clone())
        .unwrap();
    assert_eq!(alpha["used_by"], json!(["src/b.rs:3:beta"]), "{alpha}");
    assert_eq!(git(&dir, &["status", "--porcelain"]), "");

}
