//! The scanner must skip the harness's own `.claude/` directory — scanning it
//! is self-referential noise. Measured defect this guards against: a real
//! sialia scan pulled `.claude/skills/skill-creator/scripts/*.py` (the bundled
//! skill's Python helpers) into the enrich worklist, surfacing Python in a
//! C#/TS project. `.claude` lives in `manifests.toml`'s skip_dirs, so the walker
//! prunes it by name at any depth (same mechanism as `.git`/`node_modules`).

use std::path::{Path, PathBuf};
use std::process::Command;

/// A committed fixture root, resolved from the crate manifest dir so the test
/// is location-independent.
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
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

/// Scan a root into a temp `grain.model.json` and return the parsed value.
fn scan_root(root: &Path, out_dir: &Path) -> serde_json::Value {
    let model = out_dir.join("grain.model.json");
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["scan", root.to_str().unwrap(), "--out", model.to_str().unwrap()])
        .output()
        .expect("run scan over temp repo");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_str(&std::fs::read_to_string(&model).expect("read model")).expect("valid model JSON")
}

#[test]
fn scan_skips_harness_claude_dir() {
    let dir = std::env::temp_dir().join(format!("scan-skip-claude-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

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

    // AC-1 — Python (present ONLY inside .claude) is absent from the model: the
    // walker pruned the whole `.claude` subtree, so no Python file was read.
    let langs = v["languages"].as_array().expect("model carries languages");
    assert!(
        !langs.iter().any(|l| l["language"] == "python"),
        "no python leaks from .claude: {langs:?}"
    );

    // AC-2 — `.claude` is reported as a deliberate skip (proof the walker
    // recognised and pruned it), not silently dropped.
    let skipped = v["coverage"]["skipped_build_dirs"].as_array().expect("coverage carries skipped_build_dirs");
    assert!(
        skipped.iter().any(|d| d == ".claude"),
        "`.claude` is recorded among the skipped dirs: {skipped:?}"
    );

    // AC-3 — nothing under `.claude` leaks into the source-side model (no unit
    // dir, no manifest path references the pruned subtree).
    let none_under_claude = |arr: &serde_json::Value, key: &str| {
        arr.as_array()
            .map(|a| !a.iter().any(|e| e[key].as_str().map_or(false, |s| s.contains(".claude"))))
            .unwrap_or(true)
    };
    assert!(none_under_claude(&v["projects"], "dir"), "no project unit under .claude: {:?}", v["projects"]);
    assert!(none_under_claude(&v["manifests"], "path"), "no manifest under .claude: {:?}", v["manifests"]);

    let _ = std::fs::remove_dir_all(&dir);
}
