//! End-to-end contract over the committed `tests/fixtures/php_laravel` project:
//! a minimal Laravel app (composer.json + an Eloquent model + a routes file).
//! Two guards:
//!   * `composer_manifest` — the composer build manifest surfaces with its
//!     require/require-dev deps and scripts, in manifest document order
//!     (serde_json `preserve_order`).
//!   * `php_laravel_fixture` — scanning the whole fixture yields a model whose
//!     languages/modules carry php, whose project is `kind = composer`, and whose
//!     framework ranking names the Laravel dependency.
//! Everything PHP/Laravel/composer-specific lives in the fixture and in the
//! data files (languages.toml / manifests.toml / queries); `src/` stays agnostic.

use std::path::PathBuf;
use std::process::Command;

/// The committed fixture root, resolved from the crate manifest dir so the test
/// is location-independent.
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("php_laravel")
}

/// Scan the fixture into a temp `grain.model.json` and return the parsed value.
/// Mirrors `facts_cli.rs`: a temp dir owned by the test, removed at the end. The
/// `label` keeps each test's temp dir distinct — both tests in this file run in
/// the same binary in parallel, so a process-id-only path would collide and one
/// test's cleanup would yank the dir out from under the other.
fn scan_fixture(label: &str) -> (PathBuf, serde_json::Value) {
    let dir = std::env::temp_dir().join(format!("scan-php-laravel-{}-{}", label, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let model = dir.join("grain.model.json");
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["scan", fixture().to_str().unwrap(), "--out", model.to_str().unwrap()])
        .output()
        .expect("run scan over fixture");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&model).expect("read model")).expect("valid model JSON");
    (dir, v)
}

#[test]
fn composer_manifest_carries_deps_scripts_in_document_order() {
    let (dir, v) = scan_fixture("composer");

    // The composer manifest is discovered (data-driven via manifests.toml).
    let manifest = v["manifests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["kind"] == "composer")
        .expect("a composer manifest");

    // require + require-dev deps, flattened in the order the manifest declared
    // them — guards the serde_json `preserve_order` feature end-to-end: `require`
    // (php, laravel/framework, guzzlehttp/guzzle) precedes `require-dev`
    // (phpunit/phpunit, mockery/mockery), and within each block document order
    // survives instead of being alphabetized.
    let deps: Vec<&str> = manifest["dependencies"].as_array().unwrap().iter().map(|d| d.as_str().unwrap()).collect();
    assert_eq!(
        deps,
        vec!["php", "laravel/framework", "guzzlehttp/guzzle", "phpunit/phpunit", "mockery/mockery"],
        "deps must preserve require → require-dev document order: {deps:?}"
    );

    // Scripts surfaced verbatim as "name: cmd".
    let scripts: Vec<&str> = manifest["scripts"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
    assert!(
        scripts.iter().any(|s| s.starts_with("test:")),
        "the composer `scripts` block must surface: {scripts:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn php_laravel_fixture_yields_php_composer_and_laravel_signal() {
    let (dir, v) = scan_fixture("model");

    // (a) php present in languages and on the modules.
    assert!(
        v["languages"].as_array().unwrap().iter().any(|l| l["language"] == "php"),
        "php language stat present: {}",
        v["languages"]
    );
    assert!(
        v["modules"].as_array().unwrap().iter().any(|m| m["language"] == "php"),
        "at least one php module present"
    );

    // (b) the project / manifest is labelled kind = composer.
    assert!(
        v["projects"].as_array().unwrap().iter().any(|p| p["kind"] == "composer"),
        "a composer project unit present: {}",
        v["projects"]
    );

    // (c) the Laravel framework dependency is named in the frequency-ranked
    // frameworks projection (verbatim from composer.json — no curated catalog).
    let frameworks: Vec<&str> = v["frameworks"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert!(frameworks.contains(&"laravel/framework"), "Laravel dep ranked in frameworks: {frameworks:?}");

    let _ = std::fs::remove_dir_all(&dir);
}
