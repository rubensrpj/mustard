//! The scanner must skip the harness's own `.claude/` directory — scanning it
//! is self-referential noise. Measured defect this guards against: a real
//! sialia scan pulled `.claude/skills/skill-creator/scripts/*.py` (the bundled
//! skill's Python helpers) into the enrich worklist, surfacing Python in a
//! C#/TS project. `.claude` lives in `manifests.toml`'s skip_dirs, so the walker
//! prunes it by name at any depth (same mechanism as `.git`/`node_modules`).

#[path = "support/manifest_dir.rs"]
mod manifest_dir;
#[path = "support/model.rs"]
mod model;

use std::path::{Path, PathBuf};

/// A committed fixture root, resolved from the crate manifest dir so the test
/// is location-independent.
fn fixture(name: &str) -> PathBuf {
    manifest_dir::manifest_dir().join("tests").join("fixtures").join(name)
}

/// Recursively copy a committed fixture into the assembled temp repo.
fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

/// Scan a root into a temp map and return the parsed value.
fn scan_root(root: &Path, out_dir: &Path) -> serde_json::Value {
    model::scan(root, out_dir, &[]).0
}

#[test]
fn scan_skips_harness_claude_dir() {
    let temp = tempfile::Builder::new().prefix("scan-skip-claude-").tempdir().unwrap();
    let dir = temp.path().to_path_buf();

    // Real project: a TypeScript-only fixture (no Python of its own).
    let root = dir.join("repo");
    copy_tree(&fixture("graph_typescript"), &root);

    // Harness tooling nested under .claude — exactly the sialia defect: a
    // bundled skill's Python helper that must NOT be ingested as source.
    let py_dir = root.join(".claude").join("skills").join("skill-creator").join("scripts");
    std::fs::create_dir_all(&py_dir).unwrap();
    std::fs::write(
        py_dir.join("process_batch.py"),
        "def infer_purpose(method_id, body):\n    return 'noise'\n",
    )
    .unwrap();

    let v = scan_root(&root, &dir);

    // Python (present ONLY inside .claude) is absent from the model: the
    // walker pruned the whole `.claude` subtree, so no Python file was read.
    let langs = v["languages"].as_array().expect("model carries languages");
    assert!(
        !langs.iter().any(|l| l["language"] == "python"),
        "no python leaks from .claude: {langs:?}"
    );

    // `.claude` is reported as a deliberate skip (proof the walker
    // recognised and pruned it), not silently dropped.
    let skipped = v["coverage"]["skipped_build_dirs"].as_array().expect("coverage carries skipped_build_dirs");
    assert!(
        skipped.iter().any(|d| d == ".claude"),
        "`.claude` is recorded among the skipped dirs: {skipped:?}"
    );

    // Nothing under `.claude` leaks into the source-side model (no unit
    // dir, no manifest path references the pruned subtree).
    let none_under_claude = |arr: &serde_json::Value, key: &str| {
        arr.as_array()
            .map(|a| !a.iter().any(|e| e[key].as_str().is_some_and(|s| s.contains(".claude"))))
            .unwrap_or(true)
    };
    assert!(none_under_claude(&v["projects"], "dir"), "no project unit under .claude: {:?}", v["projects"]);
    assert!(none_under_claude(&v["manifests"], "path"), "no manifest under .claude: {:?}", v["manifests"]);

}

fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(["-c", "user.email=scan@example.com", "-c", "user.name=scan", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// Um repositório do git com `committed` comitado e `loose` só na pasta,
/// fora do índice.
fn repo(prefix: &str, committed: &[(&str, &str)], loose: &[(&str, &str)]) -> tempfile::TempDir {
    let temp = tempfile::Builder::new().prefix(prefix).tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    for (rel, body) in committed {
        write(dir, rel, body);
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    for (rel, body) in loose {
        write(dir, rel, body);
    }
    temp
}

/// Os caminhos dos módulos e as pastas puladas do mapa da pasta `root`.
fn modules_and_skipped(root: &Path) -> (Vec<String>, Vec<String>) {
    let out = tempfile::Builder::new().prefix("scan-skip-out-").tempdir().unwrap();
    let map = scan_root(root, out.path());
    let texts = |value: &serde_json::Value, key: Option<&str>| -> Vec<String> {
        value
            .as_array()
            .map(|all| all.iter().filter_map(|e| key.map_or(e, |k| &e[k]).as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    };
    (texts(&map["modules"], Some("path")), texts(&map["coverage"]["skipped_build_dirs"], None))
}

#[test]
fn a_committed_code_folder_named_like_build_output_is_read() {
    let temp = repo(
        "scan-skip-build-lida-",
        &[("ferramentas/build/gerar.rs", "pub fn gerar() {}\n"), ("src/lib.rs", "pub fn raiz() {}\n")],
        &[],
    );
    let (modules, skipped) = modules_and_skipped(temp.path());
    assert!(modules.contains(&"ferramentas/build/gerar.rs".to_string()), "{modules:?}");
    assert!(!skipped.contains(&"ferramentas/build".to_string()), "{skipped:?}");
}

#[test]
fn a_dependency_folder_outside_the_index_is_skipped_and_reported_by_its_path() {
    let temp = repo(
        "scan-skip-dependencia-",
        &[("web/src/app.ts", "export const app = 1;\n")],
        &[("web/node_modules/pacote/index.ts", "export const pacote = 1;\n")],
    );
    let (modules, skipped) = modules_and_skipped(temp.path());
    assert!(!modules.iter().any(|m| m.contains("node_modules")), "{modules:?}");
    assert!(modules.contains(&"web/src/app.ts".to_string()), "{modules:?}");
    assert!(skipped.contains(&"web/node_modules".to_string()), "{skipped:?}");
}

#[test]
fn a_folder_the_gitignore_takes_stays_out_and_off_the_list() {
    let temp = repo(
        "scan-skip-ignorada-",
        &[(".gitignore", "bin/\n"), ("app/main.ts", "export const main = 1;\n")],
        &[("app/bin/saida.ts", "export const saida = 1;\n")],
    );
    let (modules, skipped) = modules_and_skipped(temp.path());
    assert!(!modules.contains(&"app/bin/saida.ts".to_string()), "{modules:?}");
    assert!(!skipped.contains(&"app/bin".to_string()), "the folder the gitignore takes is not the list's: {skipped:?}");
}

#[test]
fn outside_git_a_build_folder_stays_out_and_is_reported() {
    let temp = tempfile::Builder::new().prefix("scan-skip-sem-git-").tempdir().unwrap();
    write(temp.path(), "build/x.rs", "pub fn x() {}\n");
    write(temp.path(), "src/lib.rs", "pub fn raiz() {}\n");
    let (modules, skipped) = modules_and_skipped(temp.path());
    assert!(!modules.contains(&"build/x.rs".to_string()), "{modules:?}");
    assert!(skipped.contains(&"build".to_string()), "{skipped:?}");
}

#[test]
fn a_committed_claude_folder_stays_out_and_is_reported() {
    let temp = repo(
        "scan-skip-claude-comitada-",
        &[(".claude/hooks/x.py", "def x():\n    return 1\n"), ("src/lib.rs", "pub fn raiz() {}\n")],
        &[],
    );
    let (modules, skipped) = modules_and_skipped(temp.path());
    assert!(!modules.iter().any(|m| m.starts_with(".claude/")), "{modules:?}");
    assert!(skipped.contains(&".claude".to_string()), "{skipped:?}");
}
