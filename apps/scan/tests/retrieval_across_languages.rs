//! Retrieval across the prompt-language × code-language grid.
//!
//! Nothing in this repository's instruments could see this axis. The ten
//! hand-written probe queries were all English, and `retrieval_self_recall`
//! builds each query from the words a module DECLARES — which in an
//! English-identifier codebase are English. Both are blind by construction to
//! the case where the asker and the code do not share a language, so a real
//! gap sat measured at 100% while a Portuguese request returned an EMPTY list.
//!
//! There is no rule about which language either side uses. A team may code in
//! English and ask in Portuguese, code in Portuguese and ask in English, or mix
//! both inside one repository — this very workspace does, with a module whose
//! first line declares its domain heading is canonical pt-BR. So the grid has
//! four cells and all four are legitimate:
//!
//! ```text
//!                     code EN            code PT
//!   prompt EN     same language      CROSS language
//!   prompt PT     CROSS language     same language
//! ```
//!
//! ## Why twin fixtures rather than a real repository
//!
//! Measuring the cross cells needs a corpus in each language. Depending on
//! finding one in the wild makes the instrument unrunnable; writing twins makes
//! it deterministic and reviewable. `i18n_code_en` and `i18n_code_pt` are the
//! SAME domain — a payment processor and an invoice ledger — with the same
//! shapes and the same number of declarations, differing only in the language
//! of the identifiers. Any asymmetry the test reports is therefore about
//! retrieval, never about one fixture being richer than the other.
//!
//! ## Identifiers only
//!
//! Every discriminating word lives in a DECLARATION NAME. The fixtures' prose
//! is deliberately unhelpful to the queries: a comment states intent, and
//! intent goes stale, gets copied, or is simply wrong. It is a hint for a human
//! reading the file, never the evidence a measurement leans on.
//!
//! ## What this asserts, and what it only records
//!
//! The SAME-language cells are hard assertions: a request phrased in the
//! language the code is written in must find the code. That is the floor every
//! downstream flow depends on.
//!
//! The CROSS-language cells are recorded against the value observed today. They
//! are not asserted as successes because they are not successes yet — the point
//! of the instrument is to make the gap visible and make closing it a
//! deliberate, reviewed act rather than a silent drift. When the bridge starts
//! working these numbers go UP and the test fails, asking to be re-baselined
//! with the improvement.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

/// How deep in the ranked file list a target still counts as found. A caller
/// reads the head of this list; beyond a handful it did not answer the request.
const FOUND_WITHIN: usize = 5;

/// The cross-language cells as they stand TODAY. Raise these — never lower
/// them — when the bridge improves; the assertion below explains why a change
/// is a conversation rather than an edit.
const CROSS_EN_PROMPT_PT_CODE_BASELINE: usize = 0;
const CROSS_PT_PROMPT_EN_CODE_BASELINE: usize = 0;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

/// Scan a fixture into a temp model and return its path. Per-call temp dir so
/// parallel tests never share output.
fn model_of(fixture_dir: &str) -> PathBuf {
    let tmp = std::env::temp_dir()
        .join(format!("scan-i18n-{}-{}", fixture_dir, std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let model = tmp.join("grain.model.json");
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args([
            "scan",
            fixture(fixture_dir).to_str().expect("fixture path"),
            "--out",
            model.to_str().expect("model path"),
        ])
        .output()
        .expect("run scan over fixture");
    assert!(out.status.success(), "scan failed: {}", String::from_utf8_lossy(&out.stderr));
    model
}

/// Rank of `target` in the digest's file list for `query`, or `None` when the
/// request does not surface it within [`FOUND_WITHIN`].
fn rank(model: &Path, query: &str, target: &str) -> Option<usize> {
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["digest", model.to_str().expect("model"), "--query", query])
        .output()
        .expect("run digest");
    assert!(out.status.success(), "digest failed: {}", String::from_utf8_lossy(&out.stderr));
    let res: serde_json::Value = serde_json::from_slice(&out.stdout).expect("digest emits JSON");
    let files: Vec<&str> = res["files"]
        .as_array()
        .map(|a| a.iter().filter_map(|f| f.as_str()).collect())
        .unwrap_or_default();
    files.iter().position(|f| *f == target).filter(|i| *i < FOUND_WITHIN)
}

/// One request, phrased in each language, against one known target.
struct Ask {
    en: &'static str,
    pt: &'static str,
    target: &'static str,
}

/// The English-identifier corpus and how each request reads in both languages.
const ASKS_EN_CODE: &[Ask] = &[
    Ask { en: "authorize payment processor", pt: "autorizar processador pagamento", target: "src/payment_processor.rs" },
    Ask { en: "post invoice ledger entry", pt: "lancar fatura livro lancamento", target: "src/invoice_ledger.rs" },
];

/// The Portuguese-identifier corpus — same domain, same requests.
const ASKS_PT_CODE: &[Ask] = &[
    Ask { en: "authorize payment processor", pt: "autorizar processador pagamento", target: "src/processador_pagamento.rs" },
    Ask { en: "post invoice ledger entry", pt: "lancar fatura livro lancamento", target: "src/livro_faturas.rs" },
];

/// How many of `asks` the given phrasing finds.
fn hits(model: &Path, asks: &[Ask], phrase: impl Fn(&Ask) -> &'static str) -> usize {
    asks.iter().filter(|a| rank(model, phrase(a), a.target).is_some()).count()
}

#[test]
fn the_four_cells_of_prompt_language_by_code_language() {
    let en_model = model_of("i18n_code_en");
    let pt_model = model_of("i18n_code_pt");

    let same_en = hits(&en_model, ASKS_EN_CODE, |a| a.en);
    let same_pt = hits(&pt_model, ASKS_PT_CODE, |a| a.pt);
    let cross_en_prompt = hits(&pt_model, ASKS_PT_CODE, |a| a.en);
    let cross_pt_prompt = hits(&en_model, ASKS_EN_CODE, |a| a.pt);
    let total = ASKS_EN_CODE.len();

    println!("prompt EN / code EN: {same_en}/{total}   (same language)");
    println!("prompt PT / code PT: {same_pt}/{total}   (same language)");
    println!("prompt EN / code PT: {cross_en_prompt}/{total}   (cross)");
    println!("prompt PT / code EN: {cross_pt_prompt}/{total}   (cross)");

    // FLOOR. A request phrased in the language the code is written in must find
    // the code — in EITHER language. An engine that only serves English asks
    // would pass one of these and fail the other, which is exactly the blind
    // spot the older instruments had.
    assert_eq!(same_en, total, "an English request must find English-named code");
    assert_eq!(same_pt, total, "a Portuguese request must find Portuguese-named code");

    // RECORD. These are the gap, measured. They are equalities rather than
    // floors on purpose: an unexplained IMPROVEMENT should stop the suite too,
    // so the number in this file always states what the engine really does.
    assert_eq!(
        cross_en_prompt, CROSS_EN_PROMPT_PT_CODE_BASELINE,
        "cross-language retrieval (English request over Portuguese code) changed — \
         if this went UP the bridge started working: re-baseline the constant and say so"
    );
    assert_eq!(
        cross_pt_prompt, CROSS_PT_PROMPT_EN_CODE_BASELINE,
        "cross-language retrieval (Portuguese request over English code) changed — \
         if this went UP the bridge started working: re-baseline the constant and say so"
    );
}
