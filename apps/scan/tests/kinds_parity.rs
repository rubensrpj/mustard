//! Parity contract between the kind manifest (`queries/kinds-manifest.toml`)
//! and the committed per-query-set fixtures (`tests/fixtures/graph_<dir>/`).
//!
//! The manifest is DATA: each entry is a queries/ subdirectory and the
//! `@definition.<kind>` inventory its tags.scm can emit. This test iterates
//! the manifest — no language name appears in this file — scans each entry's
//! fixture, and checks BOTH directions:
//!   * every declared kind yields >= 1 declaration in the fixture. This is
//!     the net that catches a pattern that silently stopped compiling against
//!     a grammar bump (the engine drops bad patterns individually by design,
//!     so nothing else fails loudly);
//!   * every kind the fixture produces is declared — the manifest never lies
//!     by omission.
//! Adding a language = a languages.toml row + tags.scm + a graph_<dir>
//! fixture + a manifest entry; a gap in any of the four surfaces here.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

/// A committed fixture root, resolved from the crate manifest dir.
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

/// Scan a fixture into a temp `grain.model.json` and return the parsed value.
/// Mirrors `graph_resolution.rs::scan_fixture_labeled`: a per-call temp dir
/// (label + fixture name + pid) so parallel tests scanning the same fixture
/// never yank each other's dir.
fn scan_fixture_labeled(label: &str, name: &str) -> serde_json::Value {
    let dir = std::env::temp_dir().join(format!("scan-kinds-{}-{}-{}", label, name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let model = dir.join("grain.model.json");
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["scan", fixture(name).to_str().unwrap(), "--out", model.to_str().unwrap()])
        .output()
        .expect("run scan over fixture");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&model).expect("read model")).expect("valid model JSON");
    let _ = std::fs::remove_dir_all(&dir);
    v
}

/// Every `declarations[].kind` the scanned model carries, deduplicated.
fn produced_kinds(v: &serde_json::Value) -> BTreeSet<String> {
    v["modules"]
        .as_array()
        .expect("model.modules")
        .iter()
        .flat_map(|m| m["declarations"].as_array().cloned().unwrap_or_default())
        .map(|d| d["kind"].as_str().expect("declaration.kind").to_string())
        .collect()
}

#[test]
fn kinds_manifest_matches_fixture_declarations_both_ways() {
    let manifest_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("queries").join("kinds-manifest.toml");
    let raw = std::fs::read_to_string(&manifest_path).expect("read kinds-manifest.toml");
    let manifest: toml::Value = toml::from_str(&raw).expect("kinds-manifest.toml is valid TOML");
    let entries = manifest.as_table().expect("kinds-manifest.toml is a table of query sets");
    assert!(!entries.is_empty(), "manifest declares at least one query set");

    for (dir, entry) in entries {
        let declared: BTreeSet<String> = entry
            .get("kinds")
            .and_then(|v| v.as_array())
            .unwrap_or_else(|| panic!("`{dir}` entry must carry a `kinds` array"))
            .iter()
            .map(|k| k.as_str().expect("each kind is a string").to_string())
            .collect();
        assert!(!declared.is_empty(), "`{dir}` declares at least one kind");

        let fixture_name = format!("graph_{dir}");
        let v = scan_fixture_labeled("parity", &fixture_name);
        let produced = produced_kinds(&v);

        for kind in &declared {
            assert!(
                produced.contains(kind),
                "`{dir}`: declared kind `{kind}` produced no declaration in fixture \
                 {fixture_name} — pattern dropped (grammar drift?) or fixture lacks the \
                 construct; produced: {produced:?}"
            );
        }
        for kind in &produced {
            assert!(
                declared.contains(kind),
                "`{dir}`: fixture {fixture_name} produced undeclared kind `{kind}` — \
                 declare it in queries/kinds-manifest.toml; declared: {declared:?}"
            );
        }
    }
}
