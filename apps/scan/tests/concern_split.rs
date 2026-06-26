//! Integration contract over the digest's CONCERN SPLIT, driven through the
//! binary (`digest --query` over a synthetic `grain.model.json`):
//!   * a single-concern query (every matched concept co-occurs into one group)
//!     returns NO split — `concerns` is absent/empty and the flat `files`
//!     ranking is unchanged (zero regression on a strong query);
//!   * a query whose concepts split into ≥2 co-occurrence groups (no shared
//!     module, no import bridge) returns one concern per group, each with its
//!     OWN ranked `files` restricted to that concern's concepts — so each
//!     concern recovers its target in its own sub-digest instead of one blended
//!     list where the larger concern buries the smaller.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Write a synthetic `grain.model.json` into a temp dir owned by the test.
/// Mirrors `match_tiers.rs`; the `label` keeps parallel tests' dirs distinct.
fn write_model(label: &str, modules: serde_json::Value) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("scan-concern-split-{}-{}", label, std::process::id()));
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

/// Run `digest --query` over the model and return the parsed JSON.
fn run_query(model: &Path, query: &str, out_name: &str) -> serde_json::Value {
    let out_file = model.parent().unwrap().join(out_name);
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["digest", model.to_str().unwrap(), "--query", query, "--out", out_file.to_str().unwrap()])
        .output()
        .expect("run digest --query over model");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let raw = std::fs::read_to_string(&out_file).expect("read query result");
    serde_json::from_str(&raw).expect("valid query JSON")
}

fn files_of(v: &serde_json::Value) -> Vec<String> {
    v["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().to_string()).collect()
}

#[test]
fn concern_split_single_concept_no_split() {
    // One matched concept → exactly one concern → no split: `concerns` is
    // absent (is_empty-skipped), and the flat `files` is the answer unchanged.
    let (dir, model) =
        write_model("single", serde_json::json!([module("src/finance/invoice.rs", &["Invoice", "InvoiceLine"])]));
    let q = run_query(&model, "invoice", "q.json");
    assert_eq!(files_of(&q), vec!["src/finance/invoice.rs"], "the defining file anchors: {q}");
    assert!(q.get("concerns").is_none() || q["concerns"].as_array().unwrap().is_empty(), "single concern → no split: {q}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cooccurrence_single_concern_one_cluster() {
    // Two concepts that CO-OCCUR (both declared in the same module) form ONE
    // connected component → one concern → no split, flat `files` unchanged.
    let (dir, model) =
        write_model("cooccur", serde_json::json!([module("src/billing/charge.rs", &["TenantCharge", "ChargeTenant"])]));
    let q = run_query(&model, "tenant,charge", "q.json");
    assert_eq!(files_of(&q), vec!["src/billing/charge.rs"], "both concepts share the file: {q}");
    assert!(q.get("concerns").is_none() || q["concerns"].as_array().unwrap().is_empty(), "co-occurring concepts → one cluster, no split: {q}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cooccurrence_disjoint_concepts_split() {
    // Two concepts that NEVER co-occur (disjoint modules, no import bridge)
    // form TWO components → a split with one concern each, labeled by its
    // concept.
    let (dir, model) = write_model(
        "disjoint",
        serde_json::json!([
            module("src/auth/tenant.rs", &["Tenant", "TenantContext"]),
            module("src/report/export.rs", &["Export", "ExportJob"]),
        ]),
    );
    let q = run_query(&model, "tenant,export", "q.json");
    let concerns = q["concerns"].as_array().expect("disjoint concepts → concerns array");
    assert_eq!(concerns.len(), 2, "two disconnected concepts → two concerns: {q}");
    let labels: Vec<&str> = concerns.iter().map(|c| c["label"].as_str().unwrap()).collect();
    assert!(labels.contains(&"tenant") && labels.contains(&"export"), "each concept labels its own concern: {labels:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn concern_split_target_surfaces_per_concern() {
    // A multi-concern fixture: the "tenant" concern lives across MANY modules
    // (so a blended ranking would let it dominate the top-N), the "ledger"
    // concern lives in a single focused module. Without the split the focused
    // ledger file could be buried; with it, each concern recovers its OWN
    // target file in its OWN sub-digest.
    let (dir, model) = write_model(
        "targets",
        serde_json::json!([
            module("src/tenant/a.rs", &["TenantA"]),
            module("src/tenant/b.rs", &["TenantB"]),
            module("src/tenant/c.rs", &["TenantC"]),
            module("src/tenant/d.rs", &["TenantD"]),
            module("src/ledger/posting.rs", &["LedgerPosting", "LedgerEntry"]),
        ]),
    );
    let q = run_query(&model, "tenant,ledger", "q.json");
    let concerns = q["concerns"].as_array().expect("multi-concern → concerns array");
    assert_eq!(concerns.len(), 2, "tenant and ledger never co-occur → two concerns: {q}");

    // Find each concern by label and assert its target file surfaces in ITS
    // OWN ranked list — the ledger file is not buried under the tenant flood.
    let by_label = |label: &str| -> Vec<String> {
        let c = concerns.iter().find(|c| c["label"].as_str() == Some(label)).unwrap_or_else(|| panic!("concern {label} present: {q}"));
        c["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().to_string()).collect()
    };
    let ledger = by_label("ledger");
    assert!(ledger.contains(&"src/ledger/posting.rs".to_string()), "the ledger concern surfaces its target: {ledger:?}");
    assert!(ledger.iter().all(|f| f.starts_with("src/ledger/")), "the ledger concern carries ONLY ledger files: {ledger:?}");

    let tenant = by_label("tenant");
    assert!(tenant.iter().all(|f| f.starts_with("src/tenant/")), "the tenant concern carries ONLY tenant files: {tenant:?}");
    assert!(!tenant.contains(&"src/ledger/posting.rs".to_string()), "the ledger file does not leak into the tenant concern: {tenant:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
