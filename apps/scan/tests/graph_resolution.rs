//! Characterization + contract tests for import resolution in `graph::build`.
//!
//! One minimal fixture per language under `tests/fixtures/graph_<lang>/`, each
//! with a REAL internal import in that language's idiomatic shape:
//!   * csharp      `using Demo.Models;`            (namespace import)
//!   * typescript  `import { User } from "./user"` (relative path import)
//!   * go          `import "module/internal/model"` (module-prefixed path)
//!   * python      `from mypkg.models import X`     (dotted module path)
//!   * rust        `use crate::util::helper;`       (root-alias + `::` path)
//!   * php         `use App\Models\User;`           (FQCN naming a type)
//! plus `graph_rust_external_std/`: an EXTERNAL `use std::collections::HashMap;`
//! beside an internal `src/collections.rs` — must yield ZERO edges (the
//! root-alias branch only runs for declared aliases like `crate`).
//!
//! Characterization baseline (recorded on the code BEFORE the resolution fix):
//! csharp, typescript and go already produced edges; python, rust and php
//! produced 0 edges — PHP FQCNs never matched the namespace index, dotted /
//! `::` paths never reached the path branch (it required a literal '/').
//! After the fix every language must yield edges > 0, and the three languages
//! that already resolved must keep exactly the same edges (non-regression).
//!
//! The fixtures live in dedicated `graph_*` dirs so they never collide with the
//! php_laravel / python_django fixtures used by other test files.

use std::path::PathBuf;
use std::process::Command;

/// A committed fixture root, resolved from the crate manifest dir.
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

/// Scan a fixture into a temp `grain.model.json` and return the parsed value.
/// Mirrors `php_laravel_fixture.rs`: a per-CALL temp dir (label + fixture name
/// + pid) so parallel tests scanning the same fixture never yank each other's
/// dir (the per-language test and the non-regression test share fixtures).
fn scan_fixture_labeled(label: &str, name: &str) -> serde_json::Value {
    let dir = std::env::temp_dir().join(format!("scan-graph-{}-{}-{}", label, name, std::process::id()));
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

/// Per-language entry point: the test fn name doubles as the temp-dir label.
fn scan_fixture(name: &str) -> serde_json::Value {
    scan_fixture_labeled("lang", name)
}

fn edges(v: &serde_json::Value) -> u64 {
    v["graph"]["edges"].as_u64().expect("graph.edges")
}

/// The modules ranked by fan-in (the import TARGETS) — enough to pin which
/// node the single fixture edge points at.
fn fan_in_modules(v: &serde_json::Value) -> Vec<String> {
    v["graph"]["top_fan_in"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["module"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn graph_resolution_csharp() {
    let v = scan_fixture("graph_csharp");
    assert!(edges(&v) > 0, "C# `using Demo.Models;` must resolve to an internal edge: {}", v["graph"]);
}

#[test]
fn graph_resolution_typescript() {
    let v = scan_fixture("graph_typescript");
    assert!(edges(&v) > 0, "TS relative import `./user` must resolve to an internal edge: {}", v["graph"]);
}

#[test]
fn graph_resolution_go() {
    let v = scan_fixture("graph_go");
    assert!(edges(&v) > 0, "Go module-prefixed import must resolve to an internal edge: {}", v["graph"]);
}

#[test]
fn graph_resolution_python() {
    let v = scan_fixture("graph_python");
    assert!(edges(&v) > 0, "Python `from mypkg.models import X` must resolve to an internal edge: {}", v["graph"]);
}

#[test]
fn graph_resolution_rust() {
    let v = scan_fixture("graph_rust");
    assert!(edges(&v) > 0, "Rust `use crate::util::helper;` must resolve to an internal edge: {}", v["graph"]);
}

/// Regression — root-alias false positive: a fully EXTERNAL import must never
/// edge to an internal module that happens to share a segment name. Before the
/// `root_aliases` gate, the root-alias branch dropped the FIRST segment of any
/// `::`/`\` import and probed the tail against the importer's ancestor dirs,
/// so `use std::collections::HashMap;` plus an internal `src/collections.rs`
/// produced a false edge (reproduced: edges=1). `std` is not a declared root
/// alias for Rust, so the branch must not run at all: edges == 0.
#[test]
fn graph_resolution_rust_external_std_no_false_edge() {
    let v = scan_fixture("graph_rust_external_std");
    assert_eq!(
        edges(&v),
        0,
        "external `std::collections::HashMap` must not edge to src/collections.rs: {}",
        v["graph"]
    );
}

#[test]
fn graph_resolution_php() {
    let v = scan_fixture("graph_php");
    assert!(edges(&v) > 0, "PHP `use App\\Models\\User;` must resolve to an internal edge: {}", v["graph"]);
}

/// Non-regression: the languages that already resolved BEFORE the fix (see the
/// header) must keep exactly the same edges — same count, same target module.
#[test]
fn graph_resolution_no_regression_preexisting() {
    for (fixture_name, target) in [
        ("graph_csharp", "src/Models/User.cs"),
        ("graph_typescript", "src/user.ts"),
        ("graph_go", "internal/model/user.go"),
    ] {
        let v = scan_fixture_labeled("noregress", fixture_name);
        assert_eq!(edges(&v), 1, "{fixture_name}: exactly the one pre-fix edge: {}", v["graph"]);
        assert_eq!(
            fan_in_modules(&v),
            vec![target.to_string()],
            "{fixture_name}: the edge still points at the same module"
        );
    }
}

/// Cascade smoke: with PHP FQCNs resolving, the graph stops collapsing into a
/// single L0 layer and hubs/touchpoints/fan-in stop being empty.
/// Fixture shape: 3 Models <- 2 Services <- 2 Controllers, every import an
/// internal FQCN (`App\Models\User`, `App\Services\UserService`, ...):
///   UserService -> User; PostService -> Post, Comment;
///   UserController -> UserService, User; PostController -> PostService, Post.
#[test]
fn graph_resolution_php_cascade_layers_hubs_touchpoints() {
    let v = scan_fixture("graph_php_cascade");
    let g = &v["graph"];

    assert_eq!(g["edges"].as_u64(), Some(7), "all 7 internal FQCN imports resolve: {g}");

    // Layers: Models at L0, Services at L1, Controllers at L2 — not one flat L0.
    let layers = g["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 3, "emergent layering must have 3 depths: {g}");
    let l0 = layers.iter().find(|l| l["name"] == "L0").expect("L0 present");
    assert_eq!(l0["modules"].as_u64(), Some(3), "the 3 Models are the innermost layer: {g}");

    // Fan-in: the models are depended upon (User and Post twice each).
    let fan_in = fan_in_modules(&v);
    assert!(!fan_in.is_empty(), "fan-in must not be empty: {g}");
    assert!(
        fan_in.contains(&"app/Models/User.php".to_string()),
        "User model is a fan-in target: {fan_in:?}"
    );

    // Touchpoints/hubs: controllers import across Services + Models (breadth 2).
    let touchpoints = g["touchpoints"].as_array().unwrap();
    assert!(!touchpoints.is_empty(), "touchpoints must not be empty: {g}");
    assert_eq!(touchpoints[0]["breadth"].as_u64(), Some(2), "top hub spans two dirs: {touchpoints:?}");
}
