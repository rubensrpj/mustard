//! Content contract: WHICH declaration gets WHICH kind, per query set.
//!
//! `kinds_parity.rs` proves the kind INVENTORY is alive — every declared kind
//! appears at least once, and nothing appears undeclared. That is a set-level
//! check, and it cannot see a declaration filed under the wrong kind: a member
//! recorded as a free function keeps `function` in the produced set, so parity
//! stays green while the model says something false about the code.
//!
//! It also cannot see a construct captured by NOTHING. That was not
//! hypothetical: `graph_rust` has always contained `trait Render { fn draw(); }`,
//! and `draw` — a `function_signature_item`, not a `function_item` — was
//! matched by no pattern at all. Parity passed the whole time, because
//! `function` still arrived from `helper` and `main`. An entire language
//! construct was invisible to the model and nothing could report it.
//!
//! So this test asserts the EXACT `(name, kind)` inventory each committed
//! fixture produces. It is deliberately brittle: any change to a query, a
//! grammar, or a fixture that moves one declaration shows up here as a diff of
//! the pair set, not as silence. No language name appears in the assertions —
//! each case names its fixture dir and the pairs that dir's own source spells
//! out.
//!
//! The UNIT/MEMBER line every entry below encodes: a named type is a unit; a
//! free (top-level) function is a unit; anything declared INSIDE a type — a
//! method, a field, a property, an enum member — is a member. `mine.rs` mines
//! roles from units only, so a dialect that files a member as `function`
//! silently promotes helpers into architecture.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

/// Scan a committed fixture into a temp model and return every
/// `(declaration name, kind)` pair it produced, deduplicated and sorted.
///
/// Mirrors `kinds_parity.rs::scan_fixture_labeled` — a per-call temp dir keyed
/// by fixture and pid, so tests running in parallel never yank each other's
/// output directory.
fn pairs_for(fixture_dir: &str) -> BTreeSet<(String, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(fixture_dir);
    let tmp = std::env::temp_dir().join(format!("scan-content-{}-{}", fixture_dir, std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let model = tmp.join("grain.model.json");

    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["scan", root.to_str().expect("fixture path"), "--out", model.to_str().expect("model path")])
        .output()
        .expect("run scan over fixture");
    assert!(out.status.success(), "scan failed: {}", String::from_utf8_lossy(&out.stderr));

    let text = std::fs::read_to_string(&model).expect("read model");
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid model JSON");
    let pairs = v["modules"]
        .as_array()
        .expect("model.modules")
        .iter()
        .flat_map(|m| m["declarations"].as_array().cloned().unwrap_or_default())
        .map(|d| {
            (
                d["name"].as_str().expect("declaration.name").to_string(),
                d["kind"].as_str().expect("declaration.kind").to_string(),
            )
        })
        .collect();
    let _ = std::fs::remove_dir_all(&tmp);
    pairs
}

/// Build the expected set from `(name, kind)` literals.
fn expect(pairs: &[(&str, &str)]) -> BTreeSet<(String, String)> {
    pairs.iter().map(|(n, k)| ((*n).to_string(), (*k).to_string())).collect()
}

/// Report the difference in both directions — a bare `assert_eq!` on two large
/// sets is unreadable, and the two directions mean different defects: MISSING
/// is a construct the queries stopped seeing, EXTRA is one they started
/// mis-filing.
fn assert_pairs(fixture_dir: &str, expected: &[(&str, &str)]) {
    let got = pairs_for(fixture_dir);
    let want = expect(expected);
    let missing: Vec<_> = want.difference(&got).collect();
    let extra: Vec<_> = got.difference(&want).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{fixture_dir}: declaration content drifted\n  MISSING (expected, not produced): {missing:?}\n  EXTRA   (produced, not expected): {extra:?}"
    );
}

#[test]
fn rust_files_a_free_fn_as_unit_and_an_impl_or_trait_fn_as_member() {
    // `main`/`helper` are module-level (units); `area` lives in an impl block
    // and `draw` is a trait signature — both members. `draw` is the regression
    // guard: it was captured by nothing until the function line was drawn.
    assert_pairs(
        "graph_rust",
        &[
            ("main", "function"),
            ("helper", "function"),
            ("Widget", "struct"),
            ("width", "field"),
            ("area", "method"),
            ("Mode", "enum"),
            ("Fast", "enum_member"),
            ("Slow", "enum_member"),
            ("Render", "trait"),
            ("draw", "method"),
            ("Count", "type"),
        ],
    );
}

#[test]
fn python_files_a_module_level_def_as_unit_and_a_class_def_as_member() {
    assert_pairs(
        "graph_python",
        &[
            ("User", "class"),
            ("name", "field"),
            ("rename", "method"),
            ("load", "function"),
        ],
    );
}

#[test]
fn dart_files_a_library_function_as_unit_and_a_body_member_as_member() {
    // `summarize` is declared at library level — a unit. `describe` (class),
    // `touch` (mixin, abstract) and `shout` (extension) are members, and the
    // three of them reach the engine through two different wrappers.
    assert_pairs(
        "graph_dart",
        &[
            ("Role", "enum"),
            ("Account", "class"),
            ("describe", "method"),
            ("Auditable", "mixin"),
            ("touch", "method"),
            ("AccountFormatting", "extension"),
            ("shout", "method"),
            ("summarize", "function"),
        ],
    );
}

#[test]
fn go_files_a_top_level_func_as_unit_and_a_receiver_method_as_member() {
    assert_pairs(
        "graph_go",
        &[
            ("User", "struct"),
            ("Name", "field"),
            ("Display", "method"),
            ("Storer", "interface"),
            ("ID", "type"),
            ("Load", "function"),
        ],
    );
}

#[test]
fn typescript_files_a_top_level_function_as_unit_and_a_class_member_as_member() {
    assert_pairs(
        "graph_typescript",
        &[
            ("Shape", "interface"),
            ("kind", "property"),
            ("area", "method"),
            ("ShapeId", "type"),
            ("Color", "enum"),
            ("Red", "enum_member"),
            ("Green", "enum_member"),
            ("DEFAULT_COLOR", "const"),
            ("describe", "function"),
            ("UserService", "class"),
            ("load", "method"),
            ("User", "class"),
            ("name", "field"),
        ],
    );
}

#[test]
fn php_files_a_top_level_function_as_unit_and_a_class_member_as_member() {
    assert_pairs(
        "graph_php",
        &[
            ("User", "class"),
            ("name", "property"),
            ("UserService", "class"),
            ("load", "method"),
            ("Identifiable", "interface"),
            ("id", "method"),
            ("HasLabel", "trait"),
            // The trait declares BOTH a `$label` property and a `label()`
            // accessor — one identifier, two kinds, and the pair set keeps them
            // apart where a name-only inventory would collapse them.
            ("label", "property"),
            ("label", "method"),
            ("Status", "enum"),
            ("Active", "enum_member"),
            ("Inactive", "enum_member"),
            ("helper", "function"),
        ],
    );
}

#[test]
fn csharp_files_every_member_as_member_because_it_has_no_free_function() {
    // The language has no top-level function, so its whole declaration surface
    // is types + members. This is the shape the other dialects are normalised
    // AGAINST: it needed no fix, and the test records that as a fact.
    assert_pairs(
        "graph_csharp",
        &[
            ("IShape", "interface"),
            ("Area", "method"),
            ("Point", "record"),
            ("Size", "struct"),
            ("Width", "field"),
            ("Status", "enum"),
            ("Active", "enum_member"),
            ("Inactive", "enum_member"),
            ("User", "class"),
            ("Name", "property"),
            ("UserService", "class"),
            ("Load", "method"),
        ],
    );
}
