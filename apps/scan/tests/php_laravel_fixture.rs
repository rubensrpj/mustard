//! End-to-end contract over the committed `tests/fixtures/php_laravel` project:
//! a minimal Laravel app (composer.json + an Eloquent model + a routes file).
//! One scan of the fixture, two guards:
//!   * the composer manifest — the composer build manifest surfaces with its
//!     require/require-dev deps and scripts, in manifest document order
//!     (serde_json `preserve_order`).
//!   * the model — scanning the whole fixture yields a model whose
//!     languages/modules carry php, whose project is `kind = composer`, and whose
//!     framework ranking names the Laravel dependency.
//!     Everything PHP/Laravel/composer-specific lives in the fixture and in the
//!     data files (languages.toml / manifests.toml / queries); `src/` stays agnostic.

#[path = "support/manifest_dir.rs"]
mod manifest_dir;
#[path = "support/model.rs"]
mod model;

use std::path::PathBuf;

/// The committed fixture root, resolved from the crate manifest dir so the test
/// is location-independent.
fn fixture() -> PathBuf {
    manifest_dir::manifest_dir().join("tests").join("fixtures").join("php_laravel")
}

/// Scan the fixture into a temp map and return the parsed value. The temp dir
/// is owned by the test and removed at the end, as in `facts_cli.rs`.
fn scan_fixture() -> (tempfile::TempDir, serde_json::Value) {
    let temp = tempfile::Builder::new().prefix("scan-php-laravel-").tempdir().unwrap();
    let (v, _) = model::scan(&fixture(), temp.path(), &[]);
    (temp, v)
}

/// One scan of the fixture, checked in two parts: the composer manifest, then
/// the model the scan builds around it.
#[test]
fn php_laravel_fixture_yields_composer_manifest_php_and_laravel_signal() {
    let (_dir, v) = scan_fixture();

    // Part 1: the composer manifest.

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

    // Part 2: the model. (a) php present in languages and on the modules.
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
}
