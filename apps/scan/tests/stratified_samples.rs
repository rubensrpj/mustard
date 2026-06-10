//! Stratified + MMR-diversified per-term samples (data: ranking.toml
//! `[samples]`), driven through the binary like term_index.rs:
//!   * monorepo fixture (two subprojects, two languages): when >=2 strata
//!     (the model's `projects[].dir`) carry a term, each keeps >=1 sample
//!     slot — the published catalog shows the convention in EVERY subproject
//!     instead of three shades of the dominant one;
//!   * a repo where only one stratum matches degenerates to the global
//!     ranking — stratification has no effect there;
//!   * MMR: with relevance tied, the next slot prefers the diverse module
//!     (other directory, other subtokens) over the near-duplicate of the one
//!     already picked. Every tie breaks on path asc — byte-stable output.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A committed fixture root, resolved from the crate manifest dir.
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

/// Scan a fixture into `grain.model.json` inside a per-test temp dir and
/// return (temp dir, model path, parsed model). The caller keeps the dir
/// alive so `digest` can run over the model file afterwards.
fn scan_fixture(label: &str, name: &str) -> (PathBuf, PathBuf, serde_json::Value) {
    let dir = std::env::temp_dir().join(format!("scan-stratified-{}-{}", label, std::process::id()));
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
    (dir, model, v)
}

/// Write a synthetic `grain.model.json` (modules + optional projects — every
/// other model field is `#[serde(default)]`) into a temp dir owned by the
/// test, mirroring term_index.rs.
fn write_model(label: &str, body: serde_json::Value) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("scan-stratified-{}-{}", label, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let model = dir.join("grain.model.json");
    let mut v = body;
    v["root"] = serde_json::json!(dir.to_string_lossy());
    std::fs::write(&model, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    (dir, model)
}

/// One synthetic module carrying the given declaration names.
fn module(path: &str, decls: &[&str]) -> serde_json::Value {
    let declarations: Vec<serde_json::Value> =
        decls.iter().map(|n| serde_json::json!({ "kind": "class", "name": n, "line": 1 })).collect();
    serde_json::json!({ "path": path, "declarations": declarations })
}

/// Run `digest` over the model (`query` empty = full digest) and return the
/// parsed JSON.
fn run_digest(model: &Path, query: &str, out_name: &str) -> serde_json::Value {
    let out_file = model.parent().unwrap().join(out_name);
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["digest", model.to_str().unwrap(), "--query", query, "--out", out_file.to_str().unwrap()])
        .output()
        .expect("run digest over model");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_str(&std::fs::read_to_string(&out_file).expect("read digest")).expect("valid digest JSON")
}

/// The samples of one published term.
fn samples_of(digest: &serde_json::Value, term: &str) -> Vec<String> {
    digest["terms"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["term"] == term)
        .unwrap_or_else(|| panic!("`{term}` must be indexed: {digest}"))["samples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn monorepo_strata_each_keep_a_sample_slot() {
    // Two subprojects in two languages share the "invoice" vocabulary. The
    // web stratum carries three focused modules (BM25 winners); the api
    // stratum carries one longer module that loses every global slot on
    // relevance alone. Stratification must still hand it one slot.
    let (dir, model, v) = scan_fixture("monorepo", "monorepo_mix");

    let project_dirs: Vec<&str> =
        v["projects"].as_array().unwrap().iter().map(|p| p["dir"].as_str().unwrap()).collect();
    assert!(
        project_dirs.contains(&"web") && project_dirs.contains(&"api"),
        "fixture premise: two project strata: {project_dirs:?}"
    );

    let digest = run_digest(&model, "", "digest.json");
    assert_eq!(
        samples_of(&digest, "invoice"),
        vec![
            "web/src/billing/invoice_card.ts",
            "api/Billing/InvoiceService.cs",
            "web/src/billing/invoice_list.ts",
        ],
        "stratum winners by relevance (web best, then api's guaranteed slot), then the MMR fill"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn single_matched_stratum_degenerates_to_global_ranking() {
    // Same module shapes as the monorepo fixture, but only ONE project is
    // declared — the api module belongs to no stratum, so the guarantee is
    // inert and pure relevance + MMR keeps the three web modules.
    let (dir, model) = write_model(
        "onestratum",
        serde_json::json!({
            "projects": [ { "name": "web", "dir": "web", "kind": "npm", "code_files": 3 } ],
            "modules": [
                module("web/src/billing/invoice_card.ts", &["InvoiceCard"]),
                module("web/src/billing/invoice_list.ts", &["InvoiceList"]),
                module("web/src/billing/invoice_total.ts", &["InvoiceTotal"]),
                module("api/Billing/InvoiceService.cs", &["InvoiceService", "LoadTotals", "ComputeBalance", "SyncBook"]),
            ]
        }),
    );
    let digest = run_digest(&model, "", "digest.json");
    assert_eq!(
        samples_of(&digest, "invoice"),
        vec![
            "web/src/billing/invoice_card.ts",
            "web/src/billing/invoice_list.ts",
            "web/src/billing/invoice_total.ts",
        ],
        "one matched stratum: no guaranteed slot for the unclaimed module"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mmr_prefers_the_diverse_module_over_the_near_duplicate() {
    // Three equal-relevance modules (tf 1, one declaration each). The legacy
    // ranking would emit them in path order — both `billing` files first.
    // MMR must spend slot 1 on the diverse `ship` module (other directory,
    // other path subtokens) and only then return to the second `billing` one.
    let (dir, model) = write_model(
        "mmr",
        serde_json::json!({
            "modules": [
                module("apps/billing/invoice_one.rs", &["InvoiceOne"]),
                module("apps/billing/invoice_two.rs", &["InvoiceTwo"]),
                module("apps/ship/invoice_remote.rs", &["InvoiceRemote"]),
            ]
        }),
    );
    let digest = run_digest(&model, "", "digest.json");
    assert_eq!(
        samples_of(&digest, "invoice"),
        vec!["apps/billing/invoice_one.rs", "apps/ship/invoice_remote.rs", "apps/billing/invoice_two.rs"],
        "diversity beats the near-duplicate once relevance ties"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
