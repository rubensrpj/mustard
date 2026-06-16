//! Integration contract over the query's anchor ranking, driven through the
//! binary (`digest --query` over a synthetic `grain.model.json`):
//!   * matched terms come back rarest first (count asc) — rarity is the
//!     discriminative signal, so the per-query cap trims frequent matches;
//!   * anchors are RANKED by inverse document frequency: each file scores the
//!     Σ IDF (`core::domain::ranking::idf_x1024`, fixed-point integer) of the
//!     matched terms that DECLARE it, so a rare domain term outranks a
//!     ubiquitous one that merely collides with framework vocabulary —
//!     regardless of the tier each matched on (tier is a confidence tiebreak
//!     only, never dominant). A file carrying >=2 queried concepts accumulates
//!     every term's IDF, but a frequent-term neighbour can never crowd a rare
//!     domain's file out;
//!   * a hub anchors only when a matched term lives in its DECLARATIONS — a
//!     path hit alone keeps it in `hubs`, never in `files`;
//!   * `files_detail` mirrors `files` with the IDF selection score + carrying
//!     terms, and `slices_omitted` mirrors `terms_omitted` (no silent loss);
//!   * the whole ranking is deterministic across runs (stable tie-breaks).
//!
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
fn anchor_files_are_the_deduped_union_of_per_term_samples() {
    // `files` is the deduped, IDF-RANKED set of the matched terms' per-term
    // declaration samples. A co-occurring file appears once, lists every term
    // that declares it, and carries the aggregate IDF score (Σ of its terms) —
    // so a file declared by BOTH queried concepts accumulates both and leads.
    let modules = serde_json::json!([
        module("m/aaa.rs", &["AlphaFirst"]),
        module("m/shared.rs", &["AlphaSecond", "OmegaThing"]),
        module("m/zzz.rs", &["OmegaOther"]),
    ]);
    let (dir, model) = write_model("union", modules);
    let (_, q) = run_query(&model, "alpha,omega", "query.json");

    let files: Vec<&str> = q["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert!(
        files.contains(&"m/aaa.rs") && files.contains(&"m/shared.rs") && files.contains(&"m/zzz.rs"),
        "the union covers every matched term's samples: {q}"
    );
    assert_eq!(files.iter().filter(|f| **f == "m/shared.rs").count(), 1, "co-occurring file deduped: {q}");
    let detail = q["files_detail"].as_array().unwrap();
    assert_eq!(detail.len(), files.len(), "one detail row per file: {q}");
    let shared = detail.iter().find(|d| d["file"] == "m/shared.rs").unwrap();
    let dterms: Vec<&str> = shared["terms"].as_array().unwrap().iter().map(|t| t.as_str().unwrap()).collect();
    assert!(dterms.contains(&"alpha") && dterms.contains(&"omega"), "the file lists both declaring terms: {q}");
    // The co-occurring file accumulates BOTH terms' IDF, so it carries a real
    // (non-zero) score and leads the ranking.
    assert!(shared["score_x1024"].as_u64().unwrap() > 0, "co-occurring file carries an aggregate IDF score: {q}");
    assert_eq!(files[0], "m/shared.rs", "the dual-concept file leads the IDF ranking: {q}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rare_domain_leads_the_union_ahead_of_a_frequent_neighbour() {
    // The rarest matched term leads the matched walk, so its samples lead the
    // union — a frequent neighbour's volume can never push the rare domain's
    // file out (the old crowding bug). quince: one file; garden/market: 14.
    let mut modules = vec![module("m/quince/page.rs", &["QuinceEntry", "QuinceFlow"])];
    for i in 0..14 {
        modules.push(module(
            &format!("m/gardenmarket/mod{i:02}.rs"),
            &[&format!("GardenMarketList{i:02}"), &format!("GardenMarketCard{i:02}"), &format!("GardenMarketTotal{i:02}")],
        ));
    }
    let (dir, model) = write_model("union-rare", serde_json::json!(modules));
    let (_, q) = run_query(&model, "quince,garden,market", "query.json");

    let matched: Vec<&str> =
        q["matched_terms"].as_array().unwrap().iter().map(|t| t["term"].as_str().unwrap()).collect();
    assert_eq!(matched[0], "quince", "rarest term leads the matched walk: {q}");
    let files: Vec<&str> = q["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert_eq!(files[0], "m/quince/page.rs", "rare domain's sample leads the union: {q}");
    // files_detail mirrors files order; the leader is carried by the rare term.
    let detail = q["files_detail"].as_array().unwrap();
    assert_eq!(detail.len(), files.len());
    for (f, d) in files.iter().zip(detail) {
        assert_eq!(d["file"].as_str().unwrap(), *f, "detail mirrors files order: {q}");
    }
    let dterms: Vec<&str> = detail[0]["terms"].as_array().unwrap().iter().map(|t| t.as_str().unwrap()).collect();
    assert_eq!(dterms, vec!["quince"], "the leader is declared by the rare term alone: {q}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rare_terms_samples_precede_frequent_terms_in_the_union() {
    // IDF ranks a rare term's files above a frequent term's: the rare term
    // carries a far larger IDF, so EVERY file it declares outscores the
    // frequent-term files. "ruby" lives in 2 modules; "stone"/"brick" in 20.
    let mut modules = vec![
        module("m/gem/ruby_top.rs", &["RubyAlpha", "RubyBeta"]),
        module("m/gem/ruby_more.rs", &["RubyGamma"]),
        module("m/yard/dd_a.rs", &["StoneBrickOne", "StoneBrickTwo", "StoneBrickThree"]),
        module("m/yard/dd_b.rs", &["StoneBrickFour", "StoneBrickFive", "StoneBrickSix"]),
    ];
    for i in 0..18 {
        modules.push(module(&format!("m/yard/f{i:02}.rs"), &[&format!("StoneUse{i:02}"), &format!("BrickUse{i:02}")]));
    }
    let (dir, model) = write_model("union-order", serde_json::json!(modules));
    let (_, q) = run_query(&model, "ruby,stone,brick", "query.json");

    let files: Vec<&str> = q["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    let pos = |f: &str| files.iter().position(|x| *x == f).unwrap_or_else(|| panic!("{f} missing from {files:?}"));
    // Both ruby files (the rare term) precede the frequent-term double-dipper.
    assert!(
        pos("m/gem/ruby_top.rs") < pos("m/yard/dd_b.rs") && pos("m/gem/ruby_more.rs") < pos("m/yard/dd_b.rs"),
        "rare term's samples lead the union ahead of frequent-term files: {files:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rare_stem_domain_outranks_a_ubiquitous_exact_collision() {
    // The field bug (sialia "Rota: contracts"): a generic word that collides
    // with framework vocabulary matched at the EXACT tier (`principal` ~ .NET's
    // `ClaimsPrincipal`, in dozens of auth files) and, under the old tier-first
    // walk, BURIED the rare DOMAIN term that only reached STEM (`contract`).
    // IDF fixes it: rarity decides, the match tier is a mere tiebreak. Here the
    // rare term lives in one module and matches at stem; the ubiquitous one in
    // twelve and matches at exact — the rare domain file must still lead.
    let mut modules = vec![module("m/contracts/studies_form.rs", &["StudiesForm"])];
    for i in 0..12 {
        modules.push(module(&format!("m/auth/principal{i:02}.rs"), &[&format!("PrincipalClaim{i:02}")]));
    }
    let (dir, model) = write_model("rare-stem-vs-exact", serde_json::json!(modules));
    // "study" reaches the rare index term "studies" at STEM (en plural); the
    // ubiquitous "principal" matches its index term at EXACT.
    let (_, q) = run_query(&model, "study,principal", "query.json");

    let study = q["report"]["terms"].as_array().unwrap().iter().find(|t| t["term"] == "study").unwrap();
    assert_eq!(study["tier"], "stem", "the rare domain term matched at stem, not exact: {q}");
    let files: Vec<&str> = q["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert_eq!(files[0], "m/contracts/studies_form.rs", "rare stem domain leads the ubiquitous exact collision: {q}");
    let lead = q["files_detail"].as_array().unwrap()[0]["score_x1024"].as_u64().unwrap();
    assert!(lead > 0, "anchors carry a real IDF score: {q}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn query_slices_omitted_mirrors_the_cap_and_terms_omitted_contract() {
    // 15 slice conventions match the query term; the per-query cap keeps 12
    // and `slices_omitted` names the 3 trimmed — the same no-silent-loss
    // contract `terms_omitted` already carries.
    let conventions: Vec<serde_json::Value> = (0..15)
        .map(|i| {
            serde_json::json!({
                "name": format!("conv{i:02}"), "roles": ["Quince", format!("Widget{i:02}")],
                "recurrence": 30 - i, "entities": [format!("Entity{i:02}")], "confidence": 0.9,
                "is_slice": true, "steps": [], "examples": [], "exemplar": "", "summary": ""
            })
        })
        .collect();
    let dir = std::env::temp_dir().join(format!("scan-anchor-ranking-slicecap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let model = dir.join("grain.model.json");
    let v = serde_json::json!({
        "root": dir.to_string_lossy(),
        "modules": [module("m/quince.rs", &["QuinceEntry"])],
        "conventions": conventions,
    });
    std::fs::write(&model, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let (_, q) = run_query(&model, "quince", "query.json");

    assert_eq!(q["slices"].as_array().unwrap().len(), 12, "per-query slice cap holds: {q}");
    assert_eq!(q["slices_omitted"], 3, "the trimmed tail is counted, never silent: {q}");
    assert_eq!(q["terms_omitted"], 0, "sibling field still present: {q}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn path_only_hub_never_anchors_only_declaration_matches_do() {
    // A module enters `files` ONLY through a term's declaration samples.
    // `billing_hub` path-hits "billing" (its NAME) but declares no billing
    // vocabulary, so it stays listed in `hubs` and never anchors; the two
    // modules whose DECLARATIONS carry "billing" are the union.
    let modules = vec![
        module("m/core/billing_hub.rs", &["RegistryWiring"]),
        module("m/core/billing_registry.rs", &["BillingRegistry"]),
        module("m/billing/invoice.rs", &["BillingInvoice"]),
    ];
    let dir = std::env::temp_dir().join(format!("scan-anchor-ranking-hubgate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let model = dir.join("grain.model.json");
    let v = serde_json::json!({
        "root": dir.to_string_lossy(),
        "modules": modules,
        "graph": {
            "nodes": 3, "edges": 0, "cyclic": false, "cycles": [],
            "top_fan_in": [ { "module": "m/core/billing_hub.rs", "degree": 2 } ],
            "top_fan_out": [],
            "layers": [], "cyclic_edges": 0, "total_edges": 0
        }
    });
    std::fs::write(&model, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let (_, q) = run_query(&model, "billing", "query.json");

    let hubs: Vec<&str> = q["hubs"].as_array().unwrap().iter().map(|h| h["module"].as_str().unwrap()).collect();
    assert!(hubs.contains(&"m/core/billing_hub.rs"), "path hit keeps the hub listed in `hubs`: {q}");
    let files: Vec<&str> = q["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert!(!files.contains(&"m/core/billing_hub.rs"), "path-only hub never anchors: {q}");
    assert!(
        files.contains(&"m/core/billing_registry.rs") && files.contains(&"m/billing/invoice.rs"),
        "both declaration-matched modules are in the union: {q}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn high_fan_in_file_that_declares_the_term_now_anchors() {
    // The structural stop-file heuristic was REMOVED with the ranking: a file
    // is selected purely because a matched term is DECLARED in it (per-term
    // BM25 samples). A high-fan-in module that genuinely declares the queried
    // vocabulary is legitimate evidence and now anchors — per-term BM25 already
    // disfavours glue that only re-exports (few real declarations of a term),
    // and the cross-term aggregation that the stop-file once guarded is gone.
    let modules = vec![
        module_fi("m/glue/common.rs", &["LedgerCore", "LedgerStore", "LedgerSync"], 4),
        module("m/feat/ledger.rs", &["LedgerReport"]),
    ];
    let (dir, model) = write_model("nostopfile", serde_json::json!(modules));
    let (_, q) = run_query(&model, "ledger", "query.json");

    let files: Vec<&str> = q["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert!(files.contains(&"m/glue/common.rs"), "a file declaring the term anchors (no stop-file): {q}");
    assert!(files.contains(&"m/feat/ledger.rs"), "the other declaration-matched file too: {q}");

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

/// Field regression (sialia, 2026-06-12 — spec `ranking-digest-deixa-alvo-central`),
/// rewritten for the insumo contract: on a WIDE query (more matched terms
/// than the flat `files` cap) the deterministic FACT the reader navigates is
/// the GROUPED per-term evidence `report.terms[].files` — it carries every
/// term's declaring files regardless of the flat cap, so the co-occurring
/// capability file is always reachable UNDER ITS TERM even when the flat union
/// (rarest-first, capped) crowds it out. The redesign deletes the cross-term
/// ranking that used to fake "these N are the targets"; the reader picks from
/// the grouped index instead of trusting a top-N.
#[test]
fn wide_query_target_survives_in_the_grouped_per_term_evidence() {
    let mut mods = vec![
        module("backend/financialtitles/resolver.cs", &["FinancialTitlesQueryResolver"]),
        module("app/financial/all-titles/titles-table.tsx", &["TitlesTable"]),
    ];
    for i in 0..11 {
        mods.push(module(&format!("other/dom{i:02}/file{i:02}.ts"), &[&format!("Term{i:02}Thing")]));
    }
    let modules = serde_json::Value::Array(mods);
    let (dir, model) = write_model("widequery", modules);
    let terms: Vec<String> = ["financial".to_string(), "titles".to_string()]
        .into_iter()
        .chain((0..11).map(|i| format!("term{i:02}")))
        .collect();
    let (_, q) = run_query(&model, &terms.join(","), "query.json");

    // The grouped evidence: the request term "titles" lists the target file,
    // regardless of the flat-`files` cap that a wide query overflows.
    let report_terms = q["report"]["terms"].as_array().unwrap();
    let titles = report_terms
        .iter()
        .find(|t| t["term"] == "titles")
        .expect("`titles` present in the grouped report");
    let tfiles: Vec<&str> = titles["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert!(
        tfiles.contains(&"app/financial/all-titles/titles-table.tsx"),
        "the grouped per-term evidence carries the target under its term: {q}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
