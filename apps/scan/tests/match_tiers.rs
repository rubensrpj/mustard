//! Integration contract over the digest's tier-ladder matching, driven through
//! the binary (`digest --query` over a synthetic `grain.model.json`). The ladder
//! is ENGLISH intra-language only — no cross-language lexicon bridge:
//!   * the prefix>=4 heuristic stays DEAD: a request token never matches an
//!     index token it merely truncates onto WITHOUT morphological backing
//!     ("pay" ~ "payables"); a truncation pair the English stemmer collapses to
//!     one key is genuine inflection and matches at tier `stem`
//!     ("payables" ~ "payable");
//!   * the whole identifier is indexed lowercased, so a same-case/concatenated
//!     request term lands at tier `exact` ("parentid");
//!   * same-language stems bridge real morphology only ("studies" ~ "study"),
//!     reported as tier `stem` with the language named;
//!   * a shared-root form a strict rung leaves weak/none is rescued by the T5
//!     trigram rung ("natureza" ~ "nature", "cancelado" ~ "cancel"), reported as
//!     tier `trigram` and flagged `bridged`;
//!   * the answer carries the per-term report (term, tier, lang, files) plus the
//!     aggregate matched k/n and reason; byte-stable across runs.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Write a synthetic `grain.model.json` into a temp dir owned by the test.
/// Mirrors `term_index.rs`; the `label` keeps parallel tests' dirs distinct.
fn write_model(label: &str, modules: serde_json::Value) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("scan-match-tiers-{}-{}", label, std::process::id()));
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

/// Run `digest --query` over the model and return raw bytes + parsed JSON.
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

/// The single per-term report entry of a one-term query.
fn sole_report_term(q: &serde_json::Value) -> &serde_json::Value {
    let terms = q["report"]["terms"].as_array().expect("report.terms");
    assert_eq!(terms.len(), 1, "one request term, one report entry: {q}");
    &terms[0]
}

#[test]
fn plural_singular_with_stem_backing_matches_and_bare_prefix_stays_dead() {
    // CONTRACT CHANGE (anchor-coverage spec): the old guard refused every
    // truncation pair, so the request "payables" could never reach the index
    // token "payable" — the target pages themselves stayed invisible. A pair
    // the English stemmer collapses to one key is genuine plural/singular
    // morphology and lands at tier `stem`; a bare prefix whose stems differ
    // ("pay" ~ "payables") stays an honest miss on every rung.
    let (dir, model) = write_model("payables", serde_json::json!([module("src/finance/payable.rs", &["PayableInvoice"])]));
    let (_, q) = run_query(&model, "payables", "q.json");
    let matched: Vec<&str> =
        q["matched_terms"].as_array().unwrap().iter().map(|t| t["term"].as_str().unwrap()).collect();
    assert!(matched.contains(&"payable"), "plural reaches the singular token: {q}");
    let t = sole_report_term(&q);
    assert_eq!(t["tier"], "stem", "morphological evidence, honestly tiered: {q}");
    let files: Vec<&str> = q["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert_eq!(files, vec!["src/finance/payable.rs"], "the defining file anchors: {q}");

    let (_, q) = run_query(&model, "pay", "q-pay.json");
    assert!(q["matched_terms"].as_array().unwrap().is_empty(), "bare prefix without stem backing: {q}");
    assert_eq!(sole_report_term(&q)["tier"], "none", "named miss, not silence: {q}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn shared_root_form_bridges_via_the_trigram_rescue() {
    // natureza ~ nature: a shared-root form the STRICT ladder leaves unmatched
    // (stemmers disagree on the truncation pair). Because the strict pass is
    // weak/none, the T5 trigram RESCUE turns on and bridges the pair by form
    // similarity — no glossary needed. The hit reports tier "trigram" and rides
    // as `bridged` (real evidence, form-not-literal), so the consumer keeps
    // planning. (The rescue's precision cost is confined here: it only fires
    // because the strict ladder already failed.)
    let (dir, model) = write_model("natureza", serde_json::json!([module("src/eco/nature.rs", &["NatureTrail"])]));
    let (_, q) = run_query(&model, "natureza", "q.json");
    let matched: Vec<&str> =
        q["matched_terms"].as_array().unwrap().iter().map(|t| t["term"].as_str().unwrap()).collect();
    assert!(matched.contains(&"nature"), "trigram rescue bridges the shared root: {q}");
    assert_eq!(q["miss"], false);
    let t = sole_report_term(&q);
    assert_eq!(t["term"], "natureza");
    assert_eq!(t["tier"], "trigram", "the fuzzy rescue rung is reported: {q}");
    assert_eq!(q["report"]["reason"], "weak", "fuzzy-only evidence is weak, never false confidence: {q}");
    assert_eq!(q["report"]["bridged"], true, "the trigram rescue carries a non-thin query: {q}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancelado_bridges_via_the_trigram_rescue() {
    // "cancelado" vs the identifier token "cancel": a truncation pair no STRICT
    // tier bridges by form. The strict ladder is weak/none, so the T5 trigram
    // RESCUE bridges it onto "cancel" by shared root (tier "trigram", `bridged`).
    let (dir, model) = write_model("cancelado", serde_json::json!([module("src/billing/cancel.rs", &["CancelCharge"])]));
    let (_, q) = run_query(&model, "cancelado", "q.json");
    let matched: Vec<&str> =
        q["matched_terms"].as_array().unwrap().iter().map(|t| t["term"].as_str().unwrap()).collect();
    assert!(matched.contains(&"cancel"), "trigram rescue bridges without a glossary: {q}");
    assert_eq!(sole_report_term(&q)["tier"], "trigram", "the fuzzy rescue rung is reported: {q}");
    assert_eq!(q["report"]["bridged"], true, "the trigram rescue is flagged bridged: {q}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn whole_identifier_matches_exactly() {
    // "ParentId" tokenizes to ["parent"] alone (the "id" half is under the
    // token floor) — the OLD index could never answer "parentid". The whole
    // lowercased identifier is now one extra entry per declaration, an exact
    // tier-1 key.
    let (dir, model) = write_model("ident", serde_json::json!([module("src/titles/parent.rs", &["ParentId", "SplitAsync"])]));
    let (_, q) = run_query(&model, "parentid", "q-ident.json");

    let matched: Vec<&str> = q["matched_terms"].as_array().unwrap().iter().map(|t| t["term"].as_str().unwrap()).collect();
    assert_eq!(matched, vec!["parentid"], "whole-ident exact entry: {q}");
    let t = sole_report_term(&q);
    assert_eq!(t["tier"], "exact");
    assert_eq!(t["lang"], "", "exact equality carries no language evidence");
    assert_eq!(q["report"]["reason"], "strong");
    assert_eq!(q["report"]["bridged"], false, "a literal exact hit is strong, never bridged: {q}");
    let files: Vec<&str> = q["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert_eq!(files, vec!["src/titles/parent.rs"], "ident match anchors its defining file: {q}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn same_language_stem_bridges_real_morphology_only() {
    // "studies" ~ "study": same-language stems agree AND the surfaces are not
    // a bare truncation pair — tier `stem`, language named. Recall the dead
    // prefix rule never had (neither form is a prefix of the other), gained
    // without resurrecting false cognates.
    let (dir, model) = write_model("stem", serde_json::json!([module("src/plan/study.rs", &["StudyPlan"])]));
    let (_, q) = run_query(&model, "studies", "q-stem.json");

    let matched: Vec<&str> = q["matched_terms"].as_array().unwrap().iter().map(|t| t["term"].as_str().unwrap()).collect();
    assert_eq!(matched, vec!["study"], "stem tier finds the morphological variant: {q}");
    let t = sole_report_term(&q);
    assert_eq!(t["tier"], "stem");
    assert_eq!(t["lang"], "en", "the stemmer language is the evidence: {q}");
    // A `stem`-only answer is weak AND NOT bridged: morphological guesses are
    // speculative, so the planning fields stay withheld (re-query in the code's
    // vocabulary is the right steer).
    assert_eq!(q["report"]["reason"], "weak", "stem-only is weak: {q}");
    assert_eq!(q["report"]["bridged"], false, "a stem guess is never a bridge: {q}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn report_aggregates_matched_k_of_n_and_is_byte_stable() {
    // Two terms, one hit: the aggregate is matched 1/2 and every term gets a
    // named outcome. Two binary invocations emit identical bytes — the whole
    // ladder (stems, trigram, report) is deterministic.
    let (dir, model) = write_model("aggregate", serde_json::json!([module("src/billing/cancel.rs", &["CancelCharge"])]));
    let (raw1, q) = run_query(&model, "cancelado,hierarquia", "q1.json");
    let (raw2, _) = run_query(&model, "cancelado,hierarquia", "q2.json");
    assert_eq!(raw1, raw2, "identical bytes across runs");

    assert_eq!(q["report"]["matched"], 1);
    assert_eq!(q["report"]["total"], 2);
    // matched 1/2 (k*2 >= n, not thin) carried by the trigram rescue with no
    // exact/fold hit: weak yet bridged — the matched half is real evidence.
    assert_eq!(q["report"]["reason"], "weak", "no literal hit -> weak: {q}");
    assert_eq!(q["report"]["bridged"], true, "half the vocabulary bridged via the trigram rescue: {q}");
    let terms = q["report"]["terms"].as_array().unwrap();
    assert_eq!(terms.len(), 2);
    assert_eq!(terms[0]["term"], "cancelado");
    assert_eq!(terms[0]["tier"], "trigram");
    assert_eq!(terms[1]["term"], "hierarquia");
    assert_eq!(terms[1]["tier"], "none", "the missed term is a NAMED miss: {q}");

    let _ = std::fs::remove_dir_all(&dir);
}
