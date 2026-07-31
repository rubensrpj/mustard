//! Retrieval regression net: the corpus is its own ground truth.
//!
//! Every tuning decision in the digest — a tier, a threshold, a stopword floor,
//! which declarations are units — moves where `feature` points. Until this test
//! existed there was no instrument that could see any of it: the only check was
//! a human running a handful of queries by hand and reading the answers. That
//! is not a measurement, and it hid a real hole. When the equivalence table was
//! caught aliasing `cargo` to `position` and `api` to `bees`, removing 62 such
//! entries moved a ten-query probe by 0.003 MRR — a single rank, inside the
//! noise of a sample that small. Correct by construction, unprovable by
//! instrument.
//!
//! ## Why self-recall, and why it needs no curated question list
//!
//! A curated benchmark is a maintenance debt and a bias: someone writes the
//! questions, and the engine gets tuned to those questions. This one is
//! DERIVED. For each sampled module, the query is built from that module's own
//! most distinctive declaration names, and the module itself is the answer.
//! If a file cannot be found by the words it declares, retrieval is broken for
//! every caller that phrases a request in the code's terms — which is exactly
//! what `/feature`, `/bugfix` and `/task` do after lapidating an intent.
//!
//! It is a FLOOR, not a ceiling: passing does not mean retrieval is good at
//! natural-language questions, which is a harder problem this cannot speak to.
//! Failing means it is broken at the easy case.
//!
//! ## Determinism
//!
//! The sample is the N modules with the most declarations, ties broken by path
//! — no clock, no randomness, no ordering that shifts between runs. Test and
//! fixture terrain is excluded (never an actionable target). Modules whose
//! declarations are all shared vocabulary contribute no distinctive query and
//! are skipped rather than counted as failures: the test measures retrieval,
//! not how unique a file's names happen to be.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;

/// How many modules the sample carries. Large enough that one rank moving is
/// not the whole signal, small enough that the suite stays fast.
const SAMPLE: usize = 40;

/// How many of a module's own terms make its query. Enough to name the file,
/// few enough that the query stays a request rather than a fingerprint.
const TERMS_PER_QUERY: usize = 4;

/// A term shared by more than this share of sampled modules is not
/// distinctive — it names the workspace, not the file.
const MAX_SHARE: f32 = 0.15;

/// The floor. Self-recall below this means a request phrased in the code's own
/// vocabulary does not find the code — the failure every downstream flow
/// inherits. Deliberately set under the current measurement so ordinary tuning
/// does not trip it; it catches collapse, not drift.
const MIN_TOP1: f32 = 0.70;

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Split an identifier into lowercase word tokens (camel, snake, kebab alike).
fn tokens(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() && prev_lower && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            cur.push(ch.to_ascii_lowercase());
            prev_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            prev_lower = false;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.retain(|t| t.len() >= 3);
    out
}

#[test]
fn a_module_is_found_by_the_words_it_declares() {
    let root = crate_dir().join("..").join("..");
    let model_path = root.join(".claude").join("grain.model.json");
    let Ok(raw) = std::fs::read_to_string(&model_path) else {
        eprintln!("retrieval_self_recall: no model at {} — skipping", model_path.display());
        return;
    };
    let model: serde_json::Value = serde_json::from_str(&raw).expect("valid model JSON");
    let modules = model["modules"].as_array().expect("model.modules");

    // Sample: most declarations first, path breaking ties. Production terrain
    // only — a fixture is never something a caller asks to change.
    let mut ranked: Vec<(usize, &str, Vec<String>)> = modules
        .iter()
        .filter_map(|m| {
            let path = m["path"].as_str()?;
            if mustard_core::domain::ast::is_test_path(path) {
                return None;
            }
            let decls = m["declarations"].as_array()?;
            let mut terms: Vec<String> = decls
                .iter()
                .filter_map(|d| d["name"].as_str())
                .flat_map(tokens)
                .collect();
            terms.sort();
            terms.dedup();
            (!terms.is_empty()).then_some((decls.len(), path, terms))
        })
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
    ranked.truncate(SAMPLE);
    assert!(ranked.len() >= 10, "the model must offer a real sample, got {}", ranked.len());

    // How many sampled modules each term appears in — the distinctiveness
    // signal, derived from the sample itself rather than from any list.
    let mut share: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, _, terms) in &ranked {
        for t in terms {
            *share.entry(t.as_str()).or_default() += 1;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let cap = (ranked.len() as f32 * MAX_SHARE).ceil() as usize;

    let mut asked = 0usize;
    let mut top1 = 0usize;
    let mut top5 = 0usize;
    let mut misses: Vec<String> = Vec::new();
    let mut seen_queries: BTreeSet<String> = BTreeSet::new();

    for (_, path, terms) in &ranked {
        // RAREST first, not alphabetically first: the query has to be the words
        // that name THIS file. Sorting by name instead produced requests like
        // "aborted added additions after" — four tokens the file happens to
        // contain and nothing a caller would ever type — and measured the
        // engine against a question no one asks.
        let mut candidates: Vec<&str> = terms
            .iter()
            .filter(|t| share.get(t.as_str()).copied().unwrap_or(0) <= cap)
            .map(String::as_str)
            .collect();
        candidates.sort_by(|a, b| {
            share.get(a).cmp(&share.get(b)).then(b.len().cmp(&a.len())).then(a.cmp(b))
        });
        let distinctive: Vec<&str> = candidates.into_iter().take(TERMS_PER_QUERY).collect();
        if distinctive.len() < TERMS_PER_QUERY {
            continue; // nothing distinctive to ask with — not a retrieval failure
        }
        let query = distinctive.join(" ");
        if !seen_queries.insert(query.clone()) {
            continue;
        }
        let out = Command::new(env!("CARGO_BIN_EXE_scan"))
            .args(["digest", model_path.to_str().expect("model path"), "--query", &query])
            .output()
            .expect("run digest");
        assert!(out.status.success(), "digest failed: {}", String::from_utf8_lossy(&out.stderr));
        let res: serde_json::Value =
            serde_json::from_slice(&out.stdout).expect("digest emits JSON");
        let files: Vec<&str> =
            res["files"].as_array().map(|a| a.iter().filter_map(|f| f.as_str()).collect()).unwrap_or_default();

        asked += 1;
        match files.iter().position(|f| *f == *path) {
            Some(0) => {
                top1 += 1;
                top5 += 1;
            }
            Some(i) if i < 5 => top5 += 1,
            _ => misses.push(format!("{path}  ← \"{query}\"")),
        }
    }

    assert!(asked >= 10, "too few askable modules to measure: {asked}");
    #[allow(clippy::cast_precision_loss)]
    let (r1, r5) = (top1 as f32 / asked as f32, top5 as f32 / asked as f32);
    println!("self-recall over {asked} modules: top1={r1:.3} top5={r5:.3}");
    assert!(
        r1 >= MIN_TOP1,
        "self-recall collapsed: a request phrased in the code's own words no longer finds the \
         code. top1={r1:.3} (floor {MIN_TOP1}), top5={r5:.3}, over {asked} modules.\n  \
         not first:\n  {}",
        misses.join("\n  ")
    );
}
