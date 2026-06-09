//! Stack-inference evidence must DISCOUNT test/fixture trees (data:
//! test-dirs.toml). Measured defect this guards against: scanning a repo that
//! ships committed fixtures of another stack (e.g. a composer.json under
//! tests/fixtures/) reported that stack at repo level — `dep:` from the
//! fixture's own manifest, `path:` and `code:` from its files.
//!
//! The parent fixture is assembled in a temp dir from the two committed
//! fixtures: `python_django` is the REAL project (copied at the root) and
//! `php_laravel` is the alien stack nested under `tests/fixtures/` — exactly
//! the shape of the measured defect, exercising all three evidence classes
//! (the nested composer manifest carries the dep signal).
//!
//! Scope guard: only STACK EVIDENCE is filtered. The nested tree stays fully
//! ingested (manifests, modules, units) — convention mining must keep seeing
//! test code.

use std::path::{Path, PathBuf};
use std::process::Command;

use mustard_core::domain::vocabulary::stacks::CONFIDENCE_TWO_CLASSES;

/// A committed fixture root, resolved from the crate manifest dir so the test
/// is location-independent.
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

/// Recursively copy a committed fixture into the assembled parent fixture.
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
/// Mirrors `stack_detection_e2e.rs`: a temp dir owned by the test, removed at
/// the end.
fn scan_root(root: &Path, out_dir: &Path) -> serde_json::Value {
    let model = out_dir.join("grain.model.json");
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["scan", root.to_str().unwrap(), "--out", model.to_str().unwrap()])
        .output()
        .expect("run scan over parent fixture");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_str(&std::fs::read_to_string(&model).expect("read model")).expect("valid model JSON")
}

#[test]
fn stack_evidence_excludes_nested_fixture_stack_from_repo_level() {
    let dir = std::env::temp_dir().join(format!("scan-stack-excludes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    // Parent fixture: a real django project at the root + a laravel fixture
    // nested under a conventional test tree.
    let root = dir.join("repo");
    copy_tree(&fixture("python_django"), &root);
    copy_tree(&fixture("php_laravel"), &root.join("tests").join("fixtures").join("php_laravel"));

    let v = scan_root(&root, &dir);

    // AC-1 — repo level detects ONLY the real project's stack: the nested
    // fixture's laravel evidence (dep:laravel/framework from its composer
    // manifest, path:routes/web.php, code:Illuminate\...) is discounted
    // because every one of its paths sits under a `tests` segment.
    let stacks = v["detected_stacks"].as_array().expect("model carries detected_stacks");
    assert_eq!(stacks.len(), 1, "only the real project's stack detected: {stacks:?}");
    assert_eq!(stacks[0]["name"], "django");
    let got = stacks[0]["confidence"].as_f64().unwrap();
    assert!(
        (got - f64::from(CONFIDENCE_TWO_CLASSES)).abs() < 1e-6,
        "django keeps its two-class confidence {got}: {stacks:?}"
    );

    // The discount applies ONLY to stack evidence — the nested manifest is
    // still ingested (the miner keeps seeing test trees).
    let manifests = v["manifests"].as_array().expect("model carries manifests");
    let nested = manifests
        .iter()
        .find(|m| m["path"] == "tests/fixtures/php_laravel/composer.json")
        .expect("nested fixture manifest still ingested");
    assert_eq!(nested["kind"], "composer");

    // Per-unit inference applies the same discount relative to the SCANNED
    // root: the unit born from the nested fixture manifest sits entirely
    // inside the test tree, so it reports no stack of its own.
    let projects = v["projects"].as_array().expect("model carries projects");
    let nested_unit = projects
        .iter()
        .find(|p| p["dir"] == "tests/fixtures/php_laravel")
        .expect("nested fixture forms a project unit");
    let unit_stacks = nested_unit["detected_stacks"].as_array().expect("unit carries detected_stacks");
    assert!(unit_stacks.is_empty(), "no stack from inside a test tree: {unit_stacks:?}");

    let _ = std::fs::remove_dir_all(&dir);
}
