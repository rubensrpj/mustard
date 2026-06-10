//! Integration contract over the query's anchor ranking, driven through the
//! binary (`digest --query` over a synthetic `grain.model.json`):
//!   * matched terms come back rarest first (count asc) — rarity is the
//!     discriminative signal, so the per-query cap trims frequent matches;
//!   * anchors are MATCH-FIRST, scored by BM25 summed over the matched terms
//!     (fixed-point integer, data in ranking.toml): a file carrying >=2
//!     queried concepts accumulates every term's contribution, so it rises;
//!   * a hub anchors only when a matched term lives in its DECLARATIONS — a
//!     path hit alone keeps it in `hubs`, never in `files`; fan-in is a small
//!     additive tiebreak between matched candidates, never dominant;
//!   * structural stop-file: fan-in above the ranking.toml percent of the
//!     repo's module count removes a module from anchor eligibility — a
//!     repo-relative statistic, no name knowledge;
//!   * the whole ranking is deterministic across runs (stable tie-breaks).
//! Plus the `QueryResult` stack contract over the committed php_laravel
//! fixture: the per-query response carries the model's `detected_stacks`
//! verbatim, hit or miss — and scan persists per-module `fan_in` such that
//! the per-module degrees sum to the graph's edge count.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Write a synthetic `grain.model.json` into a temp dir owned by the test.
/// Mirrors `term_index.rs`; the `label` keeps parallel tests' dirs distinct.
fn write_model(label: &str, modules: serde_json::Value) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("scan-anchor-ranking-{}-{}", label, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let model = dir.join("grain.model.json");
    let v = serde_json::json!({ "root": dir.to_string_lossy(), "modules": modules });
    std::fs::write(&model, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    (dir, model)
}

/// One synthetic module carrying the given declaration names.
fn module(path: &str, decls: &[&str]) -> serde_json::Value {
    let declarations: Vec<serde_json::Value> =
        decls.iter().map(|n| serde_json::json!({ "kind": "class", "name": n, "line": 1 })).collect();
    serde_json::json!({ "path": path, "declarations": declarations })
}

/// A synthetic module with a persisted import-graph fan-in.
fn module_fi(path: &str, decls: &[&str], fan_in: usize) -> serde_json::Value {
    let mut m = module(path, decls);
    m["fan_in"] = serde_json::json!(fan_in);
    m
}

/// Run `digest --query` over the model into `out_name` and return the raw
/// bytes + parsed JSON (raw bytes feed the determinism assertion).
fn run_query(model: &Path, query: &str, out_name: &str) -> (String, serde_json::Value) {
    let out_file = model.parent().unwrap().join(out_name);
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["digest", model.to_str().unwrap(), "--query", query, "--out", out_file.to_str().unwrap()])
        .output()
        .expect("run digest --query over model");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let raw = std::fs::read_to_string(&out_file).expect("read query result");
    let v = serde_json::from_str(&raw).expect("valid query JSON");
    (raw, v)
}

#[test]
fn anchor_ranking_orders_matched_terms_by_rarity_then_term() {
    // "alpha" recurs 3x, "omega" once — the rare (discriminative) term must
    // come first regardless of raw frequency.
    let modules = serde_json::json!([
        module("m/a.rs", &["AlphaOne", "AlphaTwo", "AlphaThree"]),
        module("m/b.rs", &["OmegaSolo"]),
    ]);
    let (dir, model) = write_model("rarity", modules);
    let (_, q) = run_query(&model, "alpha,omega", "query.json");

    let matched: Vec<&str> =
        q["matched_terms"].as_array().unwrap().iter().map(|t| t["term"].as_str().unwrap()).collect();
    assert_eq!(matched, vec!["omega", "alpha"], "rarest first: {q}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn anchor_ranking_cooccurring_sample_file_rises() {
    // `m/shared.rs` carries BOTH queried terms; `m/aaa.rs` sorts before it
    // alphabetically and belongs to the first matched term, but co-occurrence
    // (samples of >=2 matched terms) must outrank both biases.
    let modules = serde_json::json!([
        module("m/aaa.rs", &["AlphaFirst"]),
        module("m/shared.rs", &["AlphaSecond", "OmegaThing"]),
        module("m/zzz.rs", &["OmegaOther"]),
    ]);
    let (dir, model) = write_model("cooccur", modules);
    let (_, q) = run_query(&model, "alpha,omega", "query.json");

    let files: Vec<&str> = q["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert_eq!(
        files,
        vec!["m/shared.rs", "m/aaa.rs", "m/zzz.rs"],
        "co-occurring file first, then rarest-term order, then path: {q}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn anchor_hub_needs_declaration_match_not_just_path() {
    // Both hubs path-hit "billing" (and stay listed in `hubs`), but only the
    // one whose DECLARATIONS carry the queried vocabulary may anchor. Among
    // the term-matched candidates with equal BM25, the hub's fan-in acts as
    // the tiebreak — so the matching hub leads, the plain module follows and
    // the path-only hub never appears in `files`. Seven fillers keep both
    // hubs' fan-in (2) under the structural stop-file percent of 10 modules.
    let mut modules = vec![
        module_fi("m/core/billing_hub.rs", &["RegistryWiring"], 2),
        module_fi("m/core/billing_registry.rs", &["BillingRegistry"], 2),
        module("m/billing/invoice.rs", &["BillingInvoice"]),
    ];
    for i in 0..7 {
        modules.push(module(&format!("m/f/f{i}.rs"), &[&format!("Filler{i}")]));
    }
    let dir = std::env::temp_dir().join(format!("scan-anchor-ranking-hubgate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let model = dir.join("grain.model.json");
    let v = serde_json::json!({
        "root": dir.to_string_lossy(),
        "modules": modules,
        "graph": {
            "nodes": 10, "edges": 4, "cyclic": false, "cycles": [],
            "top_fan_in": [
                { "module": "m/core/billing_hub.rs", "degree": 2 },
                { "module": "m/core/billing_registry.rs", "degree": 2 }
            ],
            "top_fan_out": [],
            "layers": [], "cyclic_edges": 0, "total_edges": 4
        }
    });
    std::fs::write(&model, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let (_, q) = run_query(&model, "billing", "query.json");

    let hubs: Vec<&str> = q["hubs"].as_array().unwrap().iter().map(|h| h["module"].as_str().unwrap()).collect();
    assert!(hubs.contains(&"m/core/billing_hub.rs"), "path hit keeps the hub listed in `hubs`: {q}");
    let files: Vec<&str> = q["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert_eq!(
        files,
        vec!["m/core/billing_registry.rs", "m/billing/invoice.rs"],
        "declaration-matched hub leads via fan-in tiebreak; path-only hub never anchors: {q}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn anchor_structural_stopfile_excludes_repo_glue() {
    // `m/glue/common.rs` carries the queried term 3x — by match score alone
    // it would lead the anchors — but its fan-in (4) exceeds the stop-file
    // percent of the 10-module repo: glue the whole repo leans on is never
    // the file to read for one capability. It must stay visible as an index
    // SAMPLE (the published view is untouched) while leaving `files`.
    let mut modules = vec![
        module_fi("m/glue/common.rs", &["LedgerCore", "LedgerStore", "LedgerSync"], 4),
        module("m/feat/ledger.rs", &["LedgerReport"]),
    ];
    for i in 0..8 {
        modules.push(module(&format!("m/f/f{i}.rs"), &[&format!("Filler{i}")]));
    }
    let (dir, model) = write_model("stopfile", serde_json::json!(modules));
    let (_, q) = run_query(&model, "ledger", "query.json");

    let samples: Vec<&str> =
        q["matched_terms"][0]["samples"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
    assert_eq!(samples[0], "m/glue/common.rs", "the index sample view is untouched: {q}");
    let files: Vec<&str> = q["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert_eq!(files, vec!["m/feat/ledger.rs"], "the stop-file leaves anchor eligibility: {q}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn anchor_ranking_is_deterministic_across_runs() {
    // Two independent binary invocations over the same model must emit
    // byte-identical answers — every ranking tie-break is stable.
    let modules = serde_json::json!([
        module("m/aaa.rs", &["AlphaFirst"]),
        module("m/shared.rs", &["AlphaSecond", "OmegaThing"]),
        module("m/zzz.rs", &["OmegaOther", "AlphaThird"]),
    ]);
    let (dir, model) = write_model("determinism", modules);
    let (raw1, _) = run_query(&model, "alpha,omega,thing", "query1.json");
    let (raw2, _) = run_query(&model, "alpha,omega,thing", "query2.json");
    assert_eq!(raw1, raw2, "identical bytes across runs");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn digest_query_stacks_copies_model_detected_stacks() {
    // Scan the committed php_laravel fixture (same shape as
    // stack_detection_e2e.rs), then prove the per-query response carries the
    // model's detections verbatim — on a hit AND on a miss (the stacks are
    // repo facts, not match results).
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("php_laravel");
    let dir = std::env::temp_dir().join(format!("scan-anchor-ranking-stacks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let model = dir.join("grain.model.json");
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["scan", fixture.to_str().unwrap(), "--out", model.to_str().unwrap()])
        .output()
        .expect("run scan over fixture");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let m: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&model).expect("read model")).expect("valid model JSON");

    // Fan-in persistence invariant: scan stores each module's incoming degree
    // on the model (leaf modules omit the field), and every resolved edge
    // contributes exactly one incoming degree somewhere — so the per-module
    // fan-ins sum to the graph's edge count.
    let edges = m["graph"]["edges"].as_u64().expect("graph edge count");
    let fanin_sum: u64 = m["modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|md| md.get("fan_in").and_then(|f| f.as_u64()).unwrap_or(0))
        .sum();
    assert_eq!(fanin_sum, edges, "per-module fan-in sums to the edge count: {}", m["graph"]);

    let (_, hit) = run_query(&model, "user", "hit.json");
    assert_eq!(hit["miss"], false, "fixture vocabulary matched: {hit}");
    assert_eq!(hit["detected_stacks"], m["detected_stacks"], "stacks copied verbatim on a hit");
    assert_eq!(hit["detected_stacks"][0]["name"], "laravel", "fixture stack named: {hit}");

    let (_, miss) = run_query(&model, "zzzznothing", "miss.json");
    assert_eq!(miss["miss"], true, "nonsense term misses: {miss}");
    assert_eq!(miss["detected_stacks"], m["detected_stacks"], "stacks carried even on a miss");

    let _ = std::fs::remove_dir_all(&dir);
}
