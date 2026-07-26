// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! What the glossary check OFFERS the user, as the published JSON says it.
//!
//! Two defects of the same shape: the report answered a question nobody asked.
//! A project with no `CONTEXT.md` was handed the whole matched vocabulary to
//! interrogate — but absence asks for a FIRST glossary, one decision about the
//! project, while thinness asks for more entries and only there is a term list
//! the answer. And the list itself carried words the repository never published:
//! identifier fragments from scan's stem tier (`split201`, `completedat`) that
//! no human ever typed and nobody can define.
//!
//! Asserted against the PUBLISHED JSON contract (the document the command
//! prints), so the fix cannot pass here while the report still lies.
//!
//! Lives in `tests/` rather than in-file because an acceptance criterion runs
//! `cargo test -p mustard-rt <name> -- --exact`, and libtest matches `--exact`
//! against the FULL test path — which equals the bare function name only at the
//! root of an integration-test binary.

use mustard_core::domain::scan::DigestTerm;
use mustard_rt::commands::glossary_coverage::{score_terms, score_terms_with_corpus};
use serde_json::json;

/// Build the matched-term list the digest would hand the scorer.
fn terms(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

/// A published term index, given as `(term, count, rarity_x1024)` — the rarity
/// is what the corpus judges on, so the fixture states it directly and
/// multiplies back into the `specificity_x1024` (TF·IDF) the model publishes.
fn corpus(rows: &[(&str, usize, u64)]) -> Vec<DigestTerm> {
    rows.iter()
        .map(|(term, count, rarity)| DigestTerm {
            term: (*term).to_string(),
            count: *count,
            specificity_x1024: (*count as u64) * rarity,
            samples: Vec::new(),
            purpose: None,
        })
        .collect()
}

/// A repository whose published vocabulary spans both ends: `run`/`path` are
/// said everywhere, `ledger`/`payable` live in a corner.
fn sample_corpus() -> Vec<DigestTerm> {
    corpus(&[
        ("run", 200, 1000),
        ("path", 150, 2000),
        ("gate", 100, 3000),
        ("ledger", 40, 4000),
        ("payable", 30, 5000),
    ])
}

/// An authored glossary that defines none of the words under test — the THIN
/// case, the one where open terms are offered.
const UNRELATED_GLOSSARY: &str = "## Seed\nA starting record.";

#[test]
fn absent_glossary_offers_no_terms_to_grill() {
    let matched = terms(&["payable", "tenant", "ledger"]);

    let absent = score_terms(&matched, "");
    assert_eq!(absent["verdict"], json!("missing"), "{absent}");
    assert_eq!(absent["present"], json!(false));
    assert_eq!(
        absent["uncovered"],
        json!([]),
        "absence asks for a first glossary, not for a queue of words: {absent}"
    );
    // The shape is stable — every key is still there for a caller to read.
    assert_eq!(absent["termsTotal"], json!(3), "{absent}");
    assert_eq!(absent["termsCovered"], json!(0));
    assert_eq!(absent["coveragePct"], json!(0));
    assert_eq!(absent["contextFile"], json!(""));
    assert_eq!(absent["statedReason"], json!(""));

    // A THIN glossary asks the opposite question and still names its open terms.
    let thin = score_terms(&matched, "## Payable\nA bill owed.");
    assert_eq!(thin["verdict"], json!("weak"), "{thin}");
    assert_eq!(thin["present"], json!(true));
    assert_eq!(
        thin["uncovered"],
        json!(["tenant", "ledger"]),
        "an authored glossary is asked for MORE entries, by name: {thin}"
    );
}

#[test]
fn unpublished_fragments_are_not_offered_as_vocabulary() {
    let index = sample_corpus();

    // Real vocabulary mixed with stem-tier fragments: only the vocabulary the
    // corpus published survives into the offer.
    let mixed = terms(&["payable", "split201", "ledger", "completedat"]);
    let scored = score_terms_with_corpus(&mixed, UNRELATED_GLOSSARY, &index);
    assert_eq!(
        scored["uncovered"],
        json!(["payable", "ledger"]),
        "a word the index never published is not vocabulary anyone can define: {scored}"
    );
    assert_eq!(
        scored["verdict"],
        json!("weak"),
        "genuine domain terms still deserve the grill: {scored}"
    );

    // Fail-open: with no corpus to ask, nothing is judged unpublished and the
    // offer is exactly what coverage scored.
    let no_corpus = score_terms_with_corpus(&mixed, UNRELATED_GLOSSARY, &[]);
    assert_eq!(
        no_corpus["uncovered"],
        json!(["payable", "split201", "ledger", "completedat"]),
        "a missing model must not empty the offer: {no_corpus}"
    );

    // The same filter unblocks the decline: with the fragments out of the open
    // set, every word left is one the corpus judged repository-wide, so the
    // quorum is met and the verdict that was unreachable now fires.
    let generic = terms(&["run", "split201", "completedat"]);
    let declined = score_terms_with_corpus(&generic, UNRELATED_GLOSSARY, &index);
    assert_eq!(
        declined["verdict"],
        json!("declined"),
        "the fragments were both the noise and the blocker: {declined}"
    );
    let stated = declined["statedReason"].as_str().unwrap_or_default();
    assert!(stated.contains("run"), "the sentence names what it declines: {declined}");
    assert!(
        !stated.contains("split201"),
        "an unpublished fragment is never named as generic: {stated}"
    );
}
