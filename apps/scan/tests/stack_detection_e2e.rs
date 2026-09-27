//! End-to-end contract over the registry-driven stack-inference engine
//! (`mustard-core`'s `infer_stacks`, wired into `scan` via `ingest`):
//! scanning a committed fixture yields a model whose `detected_stacks`
//! names the stack, scores it by signal-class convergence, and carries the
//! concrete signals that fired.
//!
//! Two fixtures, two convergence levels:
//!   * `php_laravel` — all three signal classes fire (a parsed composer
//!     manifest dep + a path marker + a code signature) → high confidence.
//!   * `python_django` — only two classes can fire: path markers + code
//!     signatures. There is no python build-manifest format registered in
//!     `manifests.toml`, so no dependency evidence exists to match
//!     `manifest_deps` against (a data gap, not an engine fault) → medium
//!     confidence.
//!     Everything Laravel/Django-specific lives in the fixtures and in the data
//!     files (core's stacks.toml); `src/` stays blind to stack names.

#[path = "support/manifest_dir.rs"]
mod manifest_dir;
#[path = "support/model.rs"]
mod model;

use std::path::PathBuf;

use mustard_core::domain::vocabulary::stacks::{CONFIDENCE_THREE_CLASSES, CONFIDENCE_TWO_CLASSES};

/// A committed fixture root, resolved from the crate manifest dir so the test
/// is location-independent.
fn fixture(name: &str) -> PathBuf {
    manifest_dir::manifest_dir().join("tests").join("fixtures").join(name)
}

/// Scan a fixture into a temp map and return the parsed value.
/// Mirrors `php_laravel_fixture.rs`: a temp dir owned by the test, removed at
/// the end. The `label` keeps each test's temp dir distinct — both tests in
/// this file run in the same binary in parallel, so a process-id-only path
/// would collide and one test's cleanup would yank the dir out from under the
/// other.
fn scan_fixture(name: &str, label: &str) -> (tempfile::TempDir, serde_json::Value) {
    let temp = tempfile::Builder::new().prefix(&format!("scan-stacks-{}-", label)).tempdir().unwrap();
    let (v, _) = model::scan(&fixture(name), temp.path(), &[]);
    (temp, v)
}

/// The detection's signals as plain strings.
fn signals(detection: &serde_json::Value) -> Vec<&str> {
    detection["signals"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect()
}

/// Confidence survives an f32 → JSON → f64 round trip, so compare against the
/// engine's scoring constant with a tolerance instead of bit-exact equality.
fn assert_confidence(detection: &serde_json::Value, expected: f32) {
    let got = detection["confidence"].as_f64().unwrap();
    assert!(
        (got - f64::from(expected)).abs() < 1e-6,
        "confidence {got} != expected {expected}: {detection}"
    );
}

#[test]
fn stack_detection_e2e_laravel_converges_three_signal_classes_at_high_confidence() {
    let (_dir, v) = scan_fixture("php_laravel", "laravel");

    // Exactly one stack detected — no invented detections from the rest of
    // the built-in registry.
    let stacks = v["detected_stacks"].as_array().expect("model carries detected_stacks");
    assert_eq!(stacks.len(), 1, "only laravel detected: {stacks:?}");
    let laravel = &stacks[0];
    assert_eq!(laravel["name"], "laravel");

    // All three signal classes converged → the high-confidence tier.
    assert_confidence(laravel, CONFIDENCE_THREE_CLASSES);

    // Explainable output, deterministically ordered (class order dep → path
    // → code, declaration order within each class): the parsed composer dep,
    // the routes-file layout marker, and the facade-import code signature.
    assert_eq!(
        signals(laravel),
        vec!["dep:laravel/framework", "path:routes/web.php", "code:Illuminate\\Support\\Facades"],
        "laravel signals: {laravel}"
    );

    // PER-UNIT detection: the fixture's composer manifest sits at its root, so
    // the root unit (dir == "") must carry the same detection — the unit-level
    // field is populated from the unit's own evidence slice, never left at the
    // serde default. Single-unit fixture → identical to the model-level field
    // by construction, which is exactly what proves the mechanism runs.
    let projects = v["projects"].as_array().expect("model carries projects");
    let root_unit = projects
        .iter()
        .find(|p| p["dir"] == "")
        .expect("fixture has a root unit (composer manifest at fixture root)");
    let unit_stacks = root_unit["detected_stacks"].as_array().expect("unit carries detected_stacks");
    assert_eq!(unit_stacks.len(), 1, "root unit detects exactly laravel: {unit_stacks:?}");
    assert_eq!(unit_stacks[0]["name"], "laravel");
    assert_confidence(&unit_stacks[0], CONFIDENCE_THREE_CLASSES);
}

#[test]
fn stack_detection_e2e_django_converges_path_and_code_classes_at_medium_confidence() {
    let (_dir, v) = scan_fixture("python_django", "django");

    let stacks = v["detected_stacks"].as_array().expect("model carries detected_stacks");
    assert_eq!(stacks.len(), 1, "only django detected: {stacks:?}");
    let django = &stacks[0];
    assert_eq!(django["name"], "django");

    // Two classes (path markers + code signatures): the fixture has no
    // manifest the scanner can parse (manifests.toml registers no python
    // build system), so the dependency class cannot fire → the medium tier.
    assert_confidence(django, CONFIDENCE_TWO_CLASSES);

    let sigs = signals(django);
    assert_eq!(
        sigs,
        vec![
            "path:manage.py",
            "path:settings.py",
            "path:urls.py",
            "code:from django.db import models",
            "code:models.Model",
            "code:INSTALLED_APPS",
        ],
        "django signals: {django}"
    );
    // And by construction: no dependency evidence at all.
    assert!(!sigs.iter().any(|s| s.starts_with("dep:")), "no dep signals expected: {sigs:?}");

}
