//! Integration contract over the digest's domain-term index, driven through
//! the binary (`digest` over a synthetic `grain.model.json` — the digest is a
//! projection of the model, never a re-scan):
//!   * stopwords (data: stopwords.toml) are glue, not vocabulary — they never
//!     enter the index and never act as query tokens;
//!   * the published full digest stays capped, but `query` searches the
//!     UNCAPPED index, so a rare discriminative term beyond the cap is found;
//!   * per-term samples rank modules by BM25 (fixed-point integer, data in
//!     ranking.toml), not by walk order: tf saturation still lets the
//!     term-dense module win, and document-length normalization keeps a
//!     sprawling module from outranking a focused one by sheer declaration
//!     volume. Ties break by path asc — byte-stable output.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Write a synthetic `grain.model.json` (modules + declarations only — every
/// other model field is `#[serde(default)]`) into a temp dir owned by the
/// test. The `label` keeps each test's temp dir distinct — the tests in this
/// file run in the same binary in parallel, so a process-id-only path would
/// collide and one test's cleanup would yank the dir out from under the other.
fn write_model(label: &str, modules: serde_json::Value) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("scan-term-index-{}-{}", label, std::process::id()));
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

/// One synthetic module with explicit (kind, name) declarations.
fn kinded_module(path: &str, decls: &[(&str, &str)]) -> serde_json::Value {
    let declarations: Vec<serde_json::Value> =
        decls.iter().map(|(k, n)| serde_json::json!({ "kind": k, "name": n, "line": 1 })).collect();
    serde_json::json!({ "path": path, "declarations": declarations })
}

/// Run `digest` over the model (`query` empty = full digest) into `out_name`
/// inside the model's dir, and return the parsed JSON.
fn run_digest(model: &Path, query: &str, out_name: &str) -> serde_json::Value {
    let out_file = model.parent().unwrap().join(out_name);
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["digest", model.to_str().unwrap(), "--query", query, "--out", out_file.to_str().unwrap()])
        .output()
        .expect("run digest over model");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_str(&std::fs::read_to_string(&out_file).expect("read digest")).expect("valid digest JSON")
}

/// Term names of a digest/query `terms`-shaped array.
fn term_names(v: &serde_json::Value, key: &str) -> Vec<String> {
    v[key].as_array().unwrap().iter().map(|t| t["term"].as_str().unwrap().to_string()).collect()
}

#[test]
fn term_index_keeps_domain_terms_and_drops_stopwords() {
    // Glue words recur MORE than the domain words here — under the old
    // top-by-frequency index they would crowd the catalog; now they must be
    // absent while every domain term stays.
    let (dir, model) = write_model(
        "stopwords",
        serde_json::json!([module(
            "src/engine.rs",
            &["GrammarAndStack", "GrammarFromStack", "TheStackForGrammar", "StackWithGrammar"],
        )]),
    );
    let digest = run_digest(&model, "", "digest.json");

    let terms = term_names(&digest, "terms");
    assert!(terms.contains(&"grammar".to_string()), "domain term indexed: {terms:?}");
    assert!(terms.contains(&"stack".to_string()), "domain term indexed: {terms:?}");
    for glue in ["and", "from", "the", "for", "with"] {
        assert!(!terms.contains(&glue.to_string()), "stopword `{glue}` must not be indexed: {terms:?}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn term_index_stopword_query_token_does_not_match() {
    // "and" exists in the source vocabulary (decl names carry it), but as a
    // stopword it must be inert as a query token: no matched terms, no path
    // hits, a clean miss.
    let (dir, model) = write_model(
        "stopquery",
        serde_json::json!([module("src/and/engine.rs", &["GrammarAndStack", "LoadAndParse"])]),
    );
    let q = run_digest(&model, "and", "query.json");

    assert!(q["matched_terms"].as_array().unwrap().is_empty(), "`and` must match nothing: {q}");
    assert_eq!(q["miss"], true, "stopword-only query is a miss: {q}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn term_index_rare_discriminative_term_enters_query_beyond_digest_cap() {
    // 125 distinct filler terms (count 2 each) push past the 120-term cap of
    // the published digest; "ledger" appears ONCE, so by frequency it ranks
    // dead last and falls off the published view. The query must still find it
    // — the lookup runs over the uncapped index, and the rare term is exactly
    // the discriminative one.
    let mut decls: Vec<serde_json::Value> = Vec::new();
    for i in 0..125 {
        for _ in 0..2 {
            decls.push(serde_json::json!({ "kind": "class", "name": format!("Filler{i:03}"), "line": 1 }));
        }
    }
    let modules = serde_json::json!([
        { "path": "src/filler.rs", "declarations": decls },
        module("src/ledger.rs", &["Ledger"]),
    ]);
    let (dir, model) = write_model("rare", modules);

    // Published full digest: capped, and the rare term is the one trimmed.
    let digest = run_digest(&model, "", "digest.json");
    let terms = term_names(&digest, "terms");
    assert_eq!(terms.len(), 120, "published digest stays capped");
    assert!(!terms.contains(&"ledger".to_string()), "rare term beyond the published cap: {}", terms.len());

    // Query: found anyway, with its real anchor file.
    let q = run_digest(&model, "ledger", "query.json");
    let matched = term_names(&q, "matched_terms");
    assert_eq!(matched, vec!["ledger"], "rare discriminative term found by lookup: {q}");
    assert_eq!(q["miss"], false);
    let files: Vec<&str> = q["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert!(files.contains(&"src/ledger.rs"), "anchor carries the defining file: {files:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn term_index_samples_rank_by_bm25_not_walk_order() {
    // Walk order lists the alphabetically-early `apps/alpha` modules first,
    // but `apps/zeta/dense.rs` uses the term 3x in 3 declarations — its BM25
    // (saturated tf, length-normalized) must win sample slot 0, with the
    // tf-1 ties resolved by path asc.
    let modules = serde_json::json!([
        module("apps/alpha/a.rs", &["InvoiceAaa"]),
        module("apps/alpha/b.rs", &["InvoiceBbb"]),
        module("apps/alpha/c.rs", &["InvoiceCcc"]),
        module("apps/zeta/dense.rs", &["InvoiceOne", "InvoiceTwo", "InvoiceThree"]),
    ]);
    let (dir, model) = write_model("density", modules);
    let digest = run_digest(&model, "", "digest.json");

    let invoice = digest["terms"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["term"] == "invoice")
        .expect("`invoice` indexed");
    assert_eq!(invoice["count"], 6, "total across modules: {invoice}");
    let samples: Vec<&str> = invoice["samples"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
    assert_eq!(
        samples,
        vec!["apps/zeta/dense.rs", "apps/alpha/a.rs", "apps/alpha/b.rs"],
        "highest-BM25 module first, then path-asc ties"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn term_index_kind_class_weight_keeps_types_above_the_member_flood() {
    // "widget" occurs twice, but only under MEMBER declarations (methods);
    // "ledger" occurs once, under a TYPE declaration (class). The published
    // catalog ranks by kind-class weight (ranking.toml [catalog]: type 2.5 vs
    // member 1), so the type term must lead — that is what protects the
    // MAX_TERMS cap from member-name flooding — while `count` keeps
    // publishing the raw demoted occurrence count.
    let modules = serde_json::json!([
        kinded_module("src/widgets.rs", &[("method", "WidgetSpin"), ("method", "WidgetFlip")]),
        kinded_module("src/ledger.rs", &[("class", "LedgerBook")]),
    ]);
    let (dir, model) = write_model("kindweight", modules);
    let digest = run_digest(&model, "", "digest.json");

    let terms = term_names(&digest, "terms");
    let pos = |name: &str| terms.iter().position(|t| t == name).unwrap_or_else(|| panic!("`{name}` indexed: {terms:?}"));
    assert!(pos("ledger") < pos("widget"), "type vocabulary outranks the member flood: {terms:?}");

    let count_of = |name: &str| {
        digest["terms"].as_array().unwrap().iter().find(|t| t["term"] == name).unwrap()["count"].as_u64().unwrap()
    };
    assert_eq!(count_of("ledger"), 1, "count stays the occurrence count, never the weighted rank");
    assert_eq!(count_of("widget"), 2, "count stays the occurrence count, never the weighted rank");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn term_index_samples_bm25_length_normalizes_against_sprawl() {
    // Raw density would rank the sprawling module first: it mentions the term
    // 2x vs 1x. But it buries those mentions among 100 declarations while the
    // focused module is 50x shorter — BM25's length normalization must put
    // the focused module in sample slot 0.
    let mut sprawl: Vec<&str> = vec!["LedgerCore", "LedgerSync"];
    let fillers: Vec<String> = (0..98).map(|i| format!("Widget{i:03}")).collect();
    sprawl.extend(fillers.iter().map(|s| s.as_str()));
    let modules = serde_json::json!([
        module("apps/big/everything.rs", &sprawl),
        module("apps/small/ledger.rs", &["LedgerReport", "ReportTotal"]),
    ]);
    let (dir, model) = write_model("lennorm", modules);
    let digest = run_digest(&model, "", "digest.json");

    let ledger = digest["terms"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["term"] == "ledger")
        .expect("`ledger` indexed");
    let samples: Vec<&str> = ledger["samples"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
    assert_eq!(
        samples,
        vec!["apps/small/ledger.rs", "apps/big/everything.rs"],
        "the focused module outranks the sprawling one despite a lower raw count"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
