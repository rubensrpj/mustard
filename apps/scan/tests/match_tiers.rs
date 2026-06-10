//! Integration contract over the digest's tier-ladder matching, driven
//! through the binary (`digest --query [--lang]` over a synthetic
//! `grain.model.json`):
//!   * the prefix>=4 heuristic stays DEAD: a request token never matches an
//!     index token it merely truncates onto WITHOUT morphological backing
//!     ("pay" ~ "payables"); a truncation pair every active stemmer
//!     collapses to one key is genuine inflection and matches at tier `stem`
//!     ("payables" ~ "payable"), while a pair the stemmers disagree on stays
//!     blocked ("natureza" ~ "nature", "cancelado" ~ "cancel" — the
//!     cross-language case, the lexicon's job alone);
//!   * the whole identifier is indexed lowercased, so a same-case/concatenated
//!     request term lands at tier `exact` ("parentid");
//!   * same-language stems bridge real morphology only ("studies" ~ "study"),
//!     reported as tier `stem` with the language named;
//!   * the bilingual seed lexicon bridges across languages with the tier and
//!     pair reported ("cancelado" -> "cancel" via pt-en), and ONLY when the
//!     request language activates the pair — no glossary, no bridge;
//!   * the scanned project's own lexicon overlay
//!     (`<root>/.claude/lexicons/<pair>.toml`, root from the MODEL) extends
//!     the seed, wins per key, and degrades silently when absent/malformed;
//!   * the answer carries the per-term report (term, tier, lang, files) plus
//!     the aggregate matched k/n and reason; byte-stable across runs.

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

/// Run `digest --query` (with an explicit `--lang`; empty = the binary reads
/// the root config, absent in these temp dirs) and return raw bytes + JSON.
fn run_query(model: &Path, query: &str, lang: &str, out_name: &str) -> (String, serde_json::Value) {
    let out_file = model.parent().unwrap().join(out_name);
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["digest", model.to_str().unwrap(), "--query", query, "--lang", lang, "--out", out_file.to_str().unwrap()])
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
    // every active stemmer collapses to one key is genuine plural/singular
    // morphology and lands at tier `stem`; a bare prefix whose stems differ
    // ("pay" ~ "payables") stays an honest miss on every rung.
    let (dir, model) = write_model("payables", serde_json::json!([module("src/finance/payable.rs", &["PayableInvoice"])]));
    for (lang, out) in [("", "q-en.json"), ("pt-BR", "q-pt.json")] {
        let (_, q) = run_query(&model, "payables", lang, out);
        let matched: Vec<&str> =
            q["matched_terms"].as_array().unwrap().iter().map(|t| t["term"].as_str().unwrap()).collect();
        assert!(matched.contains(&"payable"), "plural reaches the singular token (lang={lang:?}): {q}");
        let t = sole_report_term(&q);
        assert_eq!(t["tier"], "stem", "morphological evidence, honestly tiered (lang={lang:?}): {q}");
        let files: Vec<&str> = q["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
        assert_eq!(files, vec!["src/finance/payable.rs"], "the defining file anchors (lang={lang:?}): {q}");
    }

    let (_, q) = run_query(&model, "pay", "", "q-pay.json");
    assert!(q["matched_terms"].as_array().unwrap().is_empty(), "bare prefix without stem backing: {q}");
    assert_eq!(sole_report_term(&q)["tier"], "none", "named miss, not silence: {q}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cross_language_truncation_pair_stays_blocked_without_the_lexicon() {
    // natureza ~ nature: pt collapses both to one stem, en does not — the
    // stemmers DISAGREE, so the pair never climbs T3 in any configuration
    // (one language's lone opinion on a truncation pair is the dead prefix
    // heuristic). Cross-language equivalence stays the lexicon's job alone;
    // no vendored pair carries this entry, so it is an honest, named miss.
    let (dir, model) = write_model("natureza", serde_json::json!([module("src/eco/nature.rs", &["NatureTrail"])]));
    for (lang, out) in [("", "q-en.json"), ("pt-BR", "q-pt.json")] {
        let (_, q) = run_query(&model, "natureza", lang, out);
        assert!(q["matched_terms"].as_array().unwrap().is_empty(), "stemmers disagree -> miss (lang={lang:?}): {q}");
        assert_eq!(q["miss"], true);
        let t = sole_report_term(&q);
        assert_eq!(t["term"], "natureza");
        assert_eq!(t["tier"], "none", "named miss, not silence: {q}");
        assert_eq!(q["report"]["reason"], "none");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancelado_needs_the_glossary_and_reports_its_tier() {
    // "cancelado" vs the identifier token "cancel": a truncation pair, so no
    // tier bridges it by form alone. Without the pt-en pair active (request
    // language en-only) it is an honest miss; with the request declared pt,
    // the seed lexicon bridges it (cancelar -> cancel, the inflected request
    // reaches the entry via the same-language stem) and the report names the
    // tier and the pair.
    let (dir, model) = write_model("cancelado", serde_json::json!([module("src/billing/cancel.rs", &["CancelCharge"])]));

    let (_, without) = run_query(&model, "cancelado", "", "q-nolex.json");
    assert!(without["matched_terms"].as_array().unwrap().is_empty(), "no glossary, no bridge: {without}");
    assert_eq!(sole_report_term(&without)["tier"], "none");

    let (_, with) = run_query(&model, "cancelado", "pt-BR", "q-lex.json");
    let matched: Vec<&str> =
        with["matched_terms"].as_array().unwrap().iter().map(|t| t["term"].as_str().unwrap()).collect();
    assert!(matched.contains(&"cancel"), "glossary bridges onto the repo's own vocabulary: {with}");
    assert_eq!(with["miss"], false);
    let t = sole_report_term(&with);
    assert_eq!(t["tier"], "lexicon", "tier reported: {with}");
    assert_eq!(t["lang"], "pt-en", "pair reported: {with}");
    assert_eq!(t["files"][0], "src/billing/cancel.rs", "report carries the files where the match lives: {with}");
    assert_eq!(with["report"]["matched"], 1);
    assert_eq!(with["report"]["total"], 1);
    // A lexicon-only answer is honest about its strength: no exact/fold hit.
    assert_eq!(with["report"]["reason"], "weak", "derived-only evidence is weak, not false confidence: {with}");
    let files: Vec<&str> = with["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert!(files.contains(&"src/billing/cancel.rs"), "anchor still lands: {with}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn whole_identifier_matches_exactly() {
    // "ParentId" tokenizes to ["parent"] alone (the "id" half is under the
    // token floor) — the OLD index could never answer "parentid". The whole
    // lowercased identifier is now one extra entry per declaration, an exact
    // tier-1 key.
    let (dir, model) = write_model("ident", serde_json::json!([module("src/titles/parent.rs", &["ParentId", "SplitAsync"])]));
    let (_, q) = run_query(&model, "parentid", "", "q-ident.json");

    let matched: Vec<&str> = q["matched_terms"].as_array().unwrap().iter().map(|t| t["term"].as_str().unwrap()).collect();
    assert_eq!(matched, vec!["parentid"], "whole-ident exact entry: {q}");
    let t = sole_report_term(&q);
    assert_eq!(t["tier"], "exact");
    assert_eq!(t["lang"], "", "exact equality carries no language evidence");
    assert_eq!(q["report"]["reason"], "strong");
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
    let (_, q) = run_query(&model, "studies", "", "q-stem.json");

    let matched: Vec<&str> = q["matched_terms"].as_array().unwrap().iter().map(|t| t["term"].as_str().unwrap()).collect();
    assert_eq!(matched, vec!["study"], "stem tier finds the morphological variant: {q}");
    let t = sole_report_term(&q);
    assert_eq!(t["tier"], "stem");
    assert_eq!(t["lang"], "en", "the stemmer language is the evidence: {q}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Write a project lexicon overlay into the fixture root, where the digest
/// resolves it from the model's `root` field (never the cwd).
fn write_overlay(root: &Path, body: &str) {
    let dir = root.join(".claude").join("lexicons");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pt-en.toml"), body).unwrap();
}

#[test]
fn project_lexicon_overlay_bridges_domain_vocabulary() {
    // "titulo" left the embedded seed (fintech domain jargon — in a CMS,
    // titulo = title): without a project lexicon it is an honest miss, the
    // seed-only behavior. The project's own overlay is what bridges it onto
    // the repo's vocabulary, reported as tier `lexicon` with the pair named.
    let (dir, model) = write_model("overlay", serde_json::json!([module("src/finance/payable.rs", &["PayableInvoice"])]));

    let (_, before) = run_query(&model, "titulo", "pt-BR", "q-before.json");
    assert!(before["matched_terms"].as_array().unwrap().is_empty(), "no overlay, seed has no domain entry: {before}");
    assert_eq!(sole_report_term(&before)["tier"], "none");

    write_overlay(&dir, "[terms]\ntitulo = [\"payable\"]\n");
    let (_, with) = run_query(&model, "titulo", "pt-BR", "q-with.json");
    let matched: Vec<&str> =
        with["matched_terms"].as_array().unwrap().iter().map(|t| t["term"].as_str().unwrap()).collect();
    assert!(matched.contains(&"payable"), "project entry bridges onto the index: {with}");
    let t = sole_report_term(&with);
    assert_eq!(t["tier"], "lexicon", "tier reported: {with}");
    assert_eq!(t["lang"], "pt-en", "pair reported: {with}");
    assert_eq!(t["files"][0], "src/finance/payable.rs", "files where the vocabulary lives: {with}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn project_lexicon_entries_win_over_the_seed() {
    // The same key on both sides: the project's synonyms REPLACE the seed's
    // (merge by key, project last) — "pedido" stops bridging onto the seed's
    // "order" and bridges onto the project's "quote" instead.
    let (dir, model) = write_model(
        "precedence",
        serde_json::json!([module("src/sales/order.rs", &["OrderService"]), module("src/sales/quote.rs", &["QuoteService"])]),
    );
    write_overlay(&dir, "[terms]\npedido = [\"quote\"]\n");

    let (_, q) = run_query(&model, "pedido", "pt-BR", "q-precedence.json");
    let matched: Vec<&str> = q["matched_terms"].as_array().unwrap().iter().map(|t| t["term"].as_str().unwrap()).collect();
    assert!(matched.contains(&"quote"), "project synonym bridges: {q}");
    assert!(!matched.contains(&"order"), "overridden seed synonym no longer bridges: {q}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn malformed_project_lexicon_degrades_to_the_seed() {
    // Invalid TOML in the overlay must never panic or fail the query — the
    // ladder silently keeps the embedded seed (run_query asserts exit 0).
    let (dir, model) = write_model("badlex", serde_json::json!([module("src/billing/cancel.rs", &["CancelCharge"])]));
    write_overlay(&dir, "not [valid toml");

    let (_, q) = run_query(&model, "cancelado", "pt-BR", "q-badlex.json");
    let t = sole_report_term(&q);
    assert_eq!(t["tier"], "lexicon", "seed entry still bridges: {q}");
    assert_eq!(t["lang"], "pt-en");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn seed_entry_vencido_bridges_overdue() {
    // The new generic seed entry works with no project lexicon present: the
    // seed remains the always-on floor of generic business equivalences.
    let (dir, model) = write_model("vencido", serde_json::json!([module("src/billing/overdue.rs", &["OverdueInvoice"])]));
    let (_, q) = run_query(&model, "vencido", "pt-BR", "q-vencido.json");
    let matched: Vec<&str> = q["matched_terms"].as_array().unwrap().iter().map(|t| t["term"].as_str().unwrap()).collect();
    assert!(matched.contains(&"overdue"), "vencido -> overdue via the seed: {q}");
    let t = sole_report_term(&q);
    assert_eq!(t["tier"], "lexicon");
    assert_eq!(t["lang"], "pt-en");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn report_aggregates_matched_k_of_n_and_is_byte_stable() {
    // Two terms, one hit: the aggregate is matched 1/2 and every term gets a
    // named outcome. Two binary invocations emit identical bytes — the whole
    // ladder (stems, lexicon, report) is deterministic.
    let (dir, model) = write_model("aggregate", serde_json::json!([module("src/billing/cancel.rs", &["CancelCharge"])]));
    let (raw1, q) = run_query(&model, "cancelado,hierarquia", "pt-BR", "q1.json");
    let (raw2, _) = run_query(&model, "cancelado,hierarquia", "pt-BR", "q2.json");
    assert_eq!(raw1, raw2, "identical bytes across runs");

    assert_eq!(q["report"]["matched"], 1);
    assert_eq!(q["report"]["total"], 2);
    let terms = q["report"]["terms"].as_array().unwrap();
    assert_eq!(terms.len(), 2);
    assert_eq!(terms[0]["term"], "cancelado");
    assert_eq!(terms[0]["tier"], "lexicon");
    assert_eq!(terms[1]["term"], "hierarquia");
    assert_eq!(terms[1]["tier"], "none", "the missed term is a NAMED miss: {q}");

    let _ = std::fs::remove_dir_all(&dir);
}
