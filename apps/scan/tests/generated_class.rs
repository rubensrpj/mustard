//! End-to-end contract over machine-written-file classification (classify.rs
//! + generated-markers.toml + the digest demotion), driven through the binary.
//!
//! Two surfaces:
//!   * the committed `tests/fixtures/generated_mix` project — a generator
//!     banner, a `.gitattributes` override in BOTH directions, and a
//!     hand-written control — proves scan stamps `file_class`/`marker`
//!     additively on the model and that overrides beat the catalog;
//!   * synthetic models (the digest is a projection — never a re-scan) prove
//!     the index policy: lockfile|minified leave the term index,
//!     generated|vendored stay demoted by the catalog multiplier and never
//!     surface as samples/anchors/hubs, and a query landing only on generated
//!     code answers `reason = "generated_only"` instead of a bare empty list.
//!
//! Every tool/ecosystem marker name lives in the fixture and in the catalog
//! (generated-markers.toml); `src/` stays agnostic.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The committed fixture root, resolved from the crate manifest dir.
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("generated_mix")
}

/// Scan the fixture into a temp `grain.model.json` and return (temp dir,
/// model path, parsed model). The `label` keeps each test's temp dir distinct
/// — tests run in parallel in one binary, so a pid-only path would collide.
fn scan_fixture(label: &str) -> (PathBuf, PathBuf, serde_json::Value) {
    let dir = std::env::temp_dir().join(format!("scan-generated-mix-{}-{}", label, std::process::id()));
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
    (dir, model, v)
}

/// Write a synthetic `grain.model.json` (every model field is additive /
/// defaulted) into a temp dir owned by the test.
fn write_model(label: &str, body: serde_json::Value) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("scan-generated-class-{}-{}", label, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let model = dir.join("grain.model.json");
    std::fs::write(&model, serde_json::to_string_pretty(&body).unwrap()).unwrap();
    (dir, model)
}

/// One synthetic module carrying declaration names and an optional file class.
fn module(path: &str, decls: &[&str], file_class: &str) -> serde_json::Value {
    let declarations: Vec<serde_json::Value> =
        decls.iter().map(|n| serde_json::json!({ "kind": "class", "name": n, "line": 1 })).collect();
    if file_class.is_empty() {
        serde_json::json!({ "path": path, "declarations": declarations })
    } else {
        serde_json::json!({ "path": path, "declarations": declarations, "file_class": file_class, "marker": "test" })
    }
}

/// Run `digest` over a model (`query` empty = full digest) and parse the output.
fn run_digest(model: &Path, query: &str, out_name: &str) -> serde_json::Value {
    let out_file = model.parent().unwrap().join(out_name);
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["digest", model.to_str().unwrap(), "--query", query, "--out", out_file.to_str().unwrap()])
        .output()
        .expect("run digest over model");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_str(&std::fs::read_to_string(&out_file).expect("read digest")).expect("valid digest JSON")
}

fn find_module<'a>(model: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    model["modules"].as_array().unwrap().iter().find(|m| m["path"] == path).unwrap_or_else(|| panic!("module {path} present"))
}

fn find_term<'a>(digest: &'a serde_json::Value, term: &str) -> &'a serde_json::Value {
    digest["terms"].as_array().unwrap().iter().find(|t| t["term"] == term).unwrap_or_else(|| panic!("term `{term}` indexed"))
}

fn samples(term: &serde_json::Value) -> Vec<&str> {
    term["samples"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect()
}

#[test]
fn fixture_modules_carry_class_and_provenance() {
    let (dir, _model_path, v) = scan_fixture("model");

    // Banner-marked file: classed generated, marker = the catalog literal.
    let banner = find_module(&v, "src/api_client.ts");
    assert_eq!(banner["file_class"], "generated", "banner file classed: {banner}");
    assert!(
        banner["marker"].as_str().unwrap().contains("generated"),
        "marker carries the catalog literal as provenance: {banner}"
    );

    // Same banner, but .gitattributes says `-linguist-generated`: the override
    // WINS over the catalog — no class at all (field absent, additive).
    let pinned = find_module(&v, "src/override_banner.ts");
    assert!(pinned.get("file_class").is_none(), "negative override pins hand-written: {pinned}");

    // No banner, but .gitattributes says `linguist-generated`: override classes
    // it, with the attribute file as marker.
    let marked = find_module(&v, "src/marked_by_attr.ts");
    assert_eq!(marked["file_class"], "generated", "positive override classes: {marked}");
    assert_eq!(marked["marker"], ".gitattributes:linguist-generated", "override provenance: {marked}");

    // Control: the plain file stays unclassed.
    let plain = find_module(&v, "src/handwritten.ts");
    assert!(plain.get("file_class").is_none(), "hand-written module carries no class: {plain}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn digest_keeps_generated_terms_without_samples_and_honors_override() {
    let (dir, model_path, _v) = scan_fixture("digest");
    let digest = run_digest(&model_path, "", "digest.json");

    // The generated module's vocabulary STAYS in the index (a query must still
    // land) but never offers the generated file as a sample to read.
    let payment = find_term(&digest, "payment");
    assert!(payment["count"].as_u64().unwrap() >= 1, "demoted but present: {payment}");
    assert!(samples(payment).is_empty(), "generated file never samples: {payment}");

    // Hand-written control anchors normally.
    let ledger = find_term(&digest, "ledger");
    assert_eq!(samples(ledger), vec!["src/handwritten.ts"], "hand-written sample survives");

    // The negative .gitattributes override propagates downstream: the file
    // with a banner but pinned hand-written samples like any other module.
    let billing = find_term(&digest, "billing");
    assert_eq!(samples(billing), vec!["src/override_banner.ts"], "override honored by the digest");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn query_landing_only_on_generated_answers_generated_only() {
    let (dir, model_path, _v) = scan_fixture("query");

    // "payment" lives ONLY in the generated client: matched (not a miss), but
    // with no anchorable surface — the reason says WHY instead of handing the
    // caller an empty files list to misread as "no precedent".
    let q = run_digest(&model_path, "payment", "query-gen.json");
    assert_eq!(q["miss"], false, "the term matched: {q}");
    assert!(!q["matched_terms"].as_array().unwrap().is_empty(), "matched terms surface: {q}");
    assert!(q["files"].as_array().unwrap().is_empty(), "no generated anchor offered: {q}");
    assert_eq!(q["reason"], "generated_only", "the caller learns why: {q}");

    // A hand-written hit carries anchors and no reason at all (field skipped).
    let q = run_digest(&model_path, "ledger", "query-hand.json");
    let files: Vec<&str> = q["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert!(files.contains(&"src/handwritten.ts"), "hand-written anchor present: {files:?}");
    assert!(q.get("reason").is_none(), "no reason on an anchorable answer: {q}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lockfile_and_minified_leave_the_index_entirely() {
    let (dir, model) = write_model(
        "out-of-index",
        serde_json::json!({
            "root": "x",
            "modules": [
                module("src/real.ts", &["InvoicePolicy"], ""),
                module("deps.lock.ts", &["ZebraPinned"], "lockfile"),
                module("bundle.min.ts", &["YakBundled"], "minified"),
            ]
        }),
    );
    let digest = run_digest(&model, "", "digest.json");

    let terms: Vec<&str> = digest["terms"].as_array().unwrap().iter().map(|t| t["term"].as_str().unwrap()).collect();
    assert!(terms.contains(&"invoice"), "hand-written vocabulary indexed: {terms:?}");
    assert!(!terms.contains(&"zebra"), "lockfile vocabulary out of the index: {terms:?}");
    assert!(!terms.contains(&"yak"), "minified vocabulary out of the index: {terms:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn multiplier_demotes_machine_counts_but_keeps_presence() {
    // 8 occurrences in a generated module vs 1 hand-written: with the catalog
    // multiplier 0.25 the demoted side contributes max(1, floor(8*0.25)) = 2,
    // so the total is 3 — present, never dominant (raw would be 9).
    let gen_decls = ["OmegaAlpha", "OmegaBravo", "OmegaCharlie", "OmegaDelta", "OmegaEcho", "OmegaFox", "OmegaGolf", "OmegaHotel"];
    let (dir, model) = write_model(
        "multiplier",
        serde_json::json!({
            "root": "x",
            "modules": [
                module("src/real.ts", &["OmegaReal"], ""),
                module("src/generated_client.ts", &gen_decls, "generated"),
            ]
        }),
    );
    let digest = run_digest(&model, "", "digest.json");

    let omega = find_term(&digest, "omega");
    assert_eq!(omega["count"], 3, "1 hand-written + scaled machine share: {omega}");
    assert_eq!(samples(omega), vec!["src/real.ts"], "only the hand-written file samples: {omega}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hubs_and_touchpoints_exclude_machine_written_modules() {
    // The generated registry has the highest degree, but a machine-written
    // file is never the file to read or edit — vendored counts the same.
    let (dir, model) = write_model(
        "hubs",
        serde_json::json!({
            "root": "x",
            "modules": [
                module("src/gen_registry.ts", &["WiredAll"], "generated"),
                module("src/vendored_lib.ts", &["BorrowedCode"], "vendored"),
                module("src/real_hub.ts", &["RealWiring"], ""),
            ],
            "graph": {
                "nodes": 3, "edges": 4, "cyclic": false,
                "top_fan_in": [
                    { "module": "src/gen_registry.ts", "degree": 9 },
                    { "module": "src/vendored_lib.ts", "degree": 5 },
                    { "module": "src/real_hub.ts", "degree": 3 }
                ],
                "top_fan_out": [],
                "layers": [],
                "touchpoints": [
                    { "module": "src/gen_registry.ts", "fan_out": 9, "breadth": 4 },
                    { "module": "src/real_hub.ts", "fan_out": 2, "breadth": 2 }
                ]
            }
        }),
    );
    let digest = run_digest(&model, "", "digest.json");

    let hubs: Vec<&str> =
        digest["graph"]["top_fan_in"].as_array().unwrap().iter().map(|h| h["module"].as_str().unwrap()).collect();
    assert_eq!(hubs, vec!["src/real_hub.ts"], "machine-written hubs dropped: {hubs:?}");
    let touch: Vec<&str> =
        digest["graph"]["touchpoints"].as_array().unwrap().iter().map(|t| t["module"].as_str().unwrap()).collect();
    assert_eq!(touch, vec!["src/real_hub.ts"], "machine-written touchpoints dropped: {touch:?}");

    let _ = std::fs::remove_dir_all(&dir);
}
