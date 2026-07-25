// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! One word is one glossary entry, however many ways the digest spelled it.
//!
//! The coverage check scored every matched term independently, so `spec` and
//! `specs` — one word, one thing a human would define once — each consumed a
//! slot in `termsTotal` and each appeared in `uncovered`. A glossary with about
//! nine open terms reported eighteen, and the percentage was computed against a
//! denominator inflated by its own duplicates. Worse, defining `Spec` did not
//! close `specs`: the exact matcher never tried the other spelling.
//!
//! Asserted against the PUBLISHED JSON contract (the document the command
//! prints), so the fix cannot pass here while the report still lies.
//!
//! Lives in `tests/` rather than in-file because the acceptance criterion runs
//! `cargo test -p mustard-rt glossary_terms_collapse_inflections -- --exact`,
//! and libtest matches `--exact` against the FULL test path — which equals the
//! bare function name only at the root of an integration-test binary.

use mustard_rt::commands::glossary_coverage::score_terms;
use serde_json::json;

/// Build the matched-term list the digest would hand the scorer.
fn terms(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn glossary_terms_collapse_inflections() {
    // --- 1. The count is per word stem, not per inflection -----------------
    // Six matched terms, three words. The old scoring reported six open terms.
    let matched = terms(&["spec", "specs", "wave", "waves", "tenant", "tenants"]);
    let empty = score_terms(&matched, "");

    assert_eq!(
        empty["termsTotal"],
        json!(3),
        "six spellings of three words must count as three: {empty}"
    );
    assert_eq!(
        empty["uncovered"],
        json!(["spec", "wave", "tenant"]),
        "each open word is listed ONCE, under the first spelling seen: {empty}"
    );

    // --- 2. A definition of one spelling covers the whole word --------------
    // The glossary defines the singular; the digest matched the plural too.
    let glossary = "## Spec\nA unit of work.\n\n## Wave\nOne dispatch level.";
    let partial = score_terms(&matched, glossary);

    assert_eq!(partial["termsTotal"], json!(3), "the denominator is unchanged");
    assert_eq!(
        partial["termsCovered"],
        json!(2),
        "defining `Spec` closes `specs`; defining `Wave` closes `waves`: {partial}"
    );
    assert_eq!(
        partial["uncovered"],
        json!(["tenant"]),
        "only the genuinely undefined word stays open: {partial}"
    );

    // --- 3. The percentage is computed over the collapsed denominator -------
    // Uneven duplication is what exposes the inflated denominator: three terms
    // are two words, one of them defined. Per word: 1 of 2 = 50%. Per
    // inflection it read 2 of 3 = 66%, a number no human could reconcile with
    // the glossary in front of them.
    let uneven = score_terms(&terms(&["spec", "specs", "tenant"]), "## Spec\nA unit of work.");
    assert_eq!(uneven["termsTotal"], json!(2), "two words, not three terms");
    assert_eq!(uneven["termsCovered"], json!(1));
    assert_eq!(
        uneven["coveragePct"],
        json!(50),
        "1 of 2 words = 50%; scoring inflections gave 2 of 3 = 66%: {uneven}"
    );
}

#[test]
fn distinct_words_are_never_merged() {
    // The collapse must not swallow genuinely different terms — that would hide
    // open glossary entries, the opposite failure.
    let scored = score_terms(&terms(&["payable", "receivable", "ledger", "invoice"]), "");
    assert_eq!(scored["termsTotal"], json!(4), "four unrelated words stay four");
    assert_eq!(
        scored["uncovered"],
        json!(["payable", "receivable", "ledger", "invoice"]),
        "every open word is still reported: {scored}"
    );
}
