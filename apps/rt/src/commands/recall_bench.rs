//! `mustard-rt run recall-bench` — the Phase-0 NORTH-STAR metric runner.
//!
//! The product's only moat is retrieval-by-intent: finding `EffectivateAsync`
//! from "efetivar previsão" — cross-lingual, deterministic, no vector DB. This
//! command is the GENERATOR of the reproducible number that proves it
//! (`name-match X% → purpose-search Y%`); it is the source the dashboard's
//! Recall page reads and the only honest backing for any pitch.
//!
//! ## What it measures
//!
//! Given a labelled set of recall-holes (`labels.ndjson`, one
//! `{query, files}` per line — ground-truth verified by reading the code), two
//! DETERMINISTIC retrievals compete to surface each case's ground-truth file:
//!
//! - **name-match** — the digest's anchor list (`Scan::digest_query` → `files`).
//!   It ranks files by BM25F over the declaration-NAME index — the baseline a
//!   structured grep gives you.
//! - **purpose-search** — the uncapped purpose→file index
//!   (`Scan::purpose_search`), matching the intent against the per-method
//!   `purpose` summaries the enrich step wrote. The moat.
//!
//! `recall@k` = fraction of queries whose ground-truth file appears in the
//! top-k of a retrieval. We report @1 and @5.
//!
//! ## Determinism — no AI, no network
//!
//! Both retrievals are pure (the matching lives in the scan binary, the single
//! owner of the ladder). The intent is tokenised the SAME way `feature
//! --intent` / `purpose-search` does (`domain_terms`), so a bench run and a
//! real research lookup query identically. The output is byte-stable JSON
//! (struct field order, sorted nothing-volatile, recall rounded to 4 decimals
//! so an `insta` snapshot is reproducible across runs/platforms). Fail-open: an
//! unreadable labels file / unavailable scan / unenriched model degrades to an
//! empty-or-zero report, never an error — a benchmark must never panic.
//!
//! ## Pre-requisite
//!
//! The model must be ENRICHED with purposes (`enrich-purpose --apply`); without
//! them `purpose-search` returns empty and `purposeRecall` reads 0 — the bench
//! still runs and the zero is the honest signal that enrich has not happened.

use std::path::Path;

use mustard_core::Scan;
use serde::{Deserialize, Serialize};

use crate::commands::feature::domain_terms;

/// One labelled recall-hole, parsed from a `labels.ndjson` line. Extra fields
/// the contract documents (`lang`, `note`) are informational and ignored here
/// (serde drops unknown keys) — the retrieval's language is resolved by the
/// scan binary from the model root's `mustard.json`, not per case.
#[derive(Debug, Clone, Deserialize)]
struct Label {
    /// The user's words (the INTENT), possibly in another language than the code.
    query: String,
    /// Ground-truth file(s); a hit is ANY of them in the top-k.
    files: Vec<String>,
}

/// The shape `scan purpose-search` emits — we only need the ranked file paths.
#[derive(Debug, Deserialize)]
struct PurposeOut {
    #[serde(default)]
    files: Vec<PurposeFile>,
}

#[derive(Debug, Deserialize)]
struct PurposeFile {
    file: String,
}

/// One case's outcome. `*_rank` is the 1-based position of the first
/// ground-truth file in that retrieval's ranked list, or `null` on a miss.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct CaseResult {
    query: String,
    files: Vec<String>,
    #[serde(rename = "nameRank")]
    name_rank: Option<usize>,
    #[serde(rename = "purposeRank")]
    purpose_rank: Option<usize>,
}

/// Aggregate recall@k per retrieval. The `@1`/`@5` keys are the documented
/// contract the dashboard reads — `serde(rename)` carries the non-ident names.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct Summary {
    #[serde(rename = "nameRecall@1")]
    name_recall_1: f64,
    #[serde(rename = "nameRecall@5")]
    name_recall_5: f64,
    #[serde(rename = "purposeRecall@1")]
    purpose_recall_1: f64,
    #[serde(rename = "purposeRecall@5")]
    purpose_recall_5: f64,
}

/// The byte-stable bench report.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct Report {
    n: usize,
    summary: Summary,
    cases: Vec<CaseResult>,
}

/// CLI face: `mustard-rt run recall-bench --labels <ndjson> --model <path>`.
///
/// Reads the labels, runs both retrievals per case through `Scan`, assembles
/// the report, and prints it as pretty byte-stable JSON. Fail-open throughout.
pub fn run(labels: &Path, model: &Path) {
    let text = std::fs::read_to_string(labels).unwrap_or_default();
    let cases = parse_labels(&text);
    let scan = Scan::locate();

    let rows: Vec<CaseResult> = cases
        .iter()
        .map(|label| {
            let terms = domain_terms(&label.query);
            let name_files = scan.digest_query(model, &terms).map(|dq| dq.files).unwrap_or_default();
            let purpose_files = purpose_files(&scan, model, &terms);
            CaseResult {
                query: label.query.clone(),
                files: label.files.clone(),
                name_rank: rank_of(&name_files, &label.files),
                purpose_rank: rank_of(&purpose_files, &label.files),
            }
        })
        .collect();

    let report = Report { n: rows.len(), summary: summarize(&rows), cases: rows };
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
}

/// Run the purpose search and extract its ranked file paths. Fail-open: an
/// unavailable scan / unparseable JSON / empty result → an empty list.
fn purpose_files(scan: &Scan, model: &Path, terms: &[String]) -> Vec<String> {
    scan.purpose_search(model, terms)
        .ok()
        .and_then(|out| serde_json::from_str::<PurposeOut>(&out).ok())
        .map(|out| out.files.into_iter().map(|f| f.file).collect())
        .unwrap_or_default()
}

// --- pure logic (unit-testable without the scan binary) ---------------------

/// Parse a `labels.ndjson` body: one JSON object per line. Blank lines and
/// `#` / `//` comment lines are skipped; a non-comment line that fails to parse
/// is skipped (a stderr warning keeps stdout byte-stable) so one malformed
/// label never voids the whole bench.
fn parse_labels(text: &str) -> Vec<Label> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("//"))
        .filter_map(|l| match serde_json::from_str::<Label>(l) {
            Ok(label) => Some(label),
            Err(e) => {
                eprintln!("recall-bench: skipping unparseable label line: {e}");
                None
            }
        })
        .collect()
}

/// 1-based rank of the first retrieved file matching ANY ground-truth path, or
/// `None` if no ground-truth file appears in the list.
fn rank_of(retrieved: &[String], truth: &[String]) -> Option<usize> {
    retrieved
        .iter()
        .position(|got| truth.iter().any(|want| path_eq(got, want)))
        .map(|i| i + 1)
}

/// Path equality tolerant of separator + a leading-`./` / root-prefix gap:
/// normalised exact match, OR one path is a `/`-bounded suffix of the other (a
/// label authored relative to a subproject still matches a model-root path).
fn path_eq(a: &str, b: &str) -> bool {
    let (a, b) = (normalize(a), normalize(b));
    a == b || a.ends_with(&format!("/{b}")) || b.ends_with(&format!("/{a}"))
}

/// Forward slashes, trimmed, leading `./` stripped.
fn normalize(p: &str) -> String {
    p.trim().replace('\\', "/").trim_start_matches("./").to_string()
}

/// Aggregate the per-case ranks into the recall@1 / recall@5 summary.
fn summarize(rows: &[CaseResult]) -> Summary {
    let names: Vec<Option<usize>> = rows.iter().map(|r| r.name_rank).collect();
    let purposes: Vec<Option<usize>> = rows.iter().map(|r| r.purpose_rank).collect();
    Summary {
        name_recall_1: recall_at(&names, 1),
        name_recall_5: recall_at(&names, 5),
        purpose_recall_1: recall_at(&purposes, 1),
        purpose_recall_5: recall_at(&purposes, 5),
    }
}

/// Fraction of cases whose rank is present AND `<= k`, rounded to 4 decimals
/// (byte-stable). Empty input → `0.0` (no division by zero).
fn recall_at(ranks: &[Option<usize>], k: usize) -> f64 {
    if ranks.is_empty() {
        return 0.0;
    }
    let hits = ranks.iter().filter(|r| matches!(r, Some(rank) if *rank <= k)).count();
    round4(hits as f64 / ranks.len() as f64)
}

/// Round to 4 decimals so `7/10` serialises as `0.7`, not `0.7000000001`.
fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(name_rank: Option<usize>, purpose_rank: Option<usize>) -> CaseResult {
        CaseResult { query: "q".to_string(), files: vec!["f".to_string()], name_rank, purpose_rank }
    }

    #[test]
    fn parse_labels_reads_ndjson_skips_blanks_and_comments() {
        let body = r#"
            # a comment
            {"query": "efetivar previsão", "files": ["src/Forecast.cs"], "lang": "pt-BR", "note": "EffectivateAsync"}

            // another comment
            {"query": "quitar fatura", "files": ["src/Invoice.cs", "src/Billing.cs"]}
        "#;
        let labels = parse_labels(body);
        assert_eq!(labels.len(), 2, "two data lines, comments + blanks skipped");
        assert_eq!(labels[0].query, "efetivar previsão");
        assert_eq!(labels[0].files, vec!["src/Forecast.cs"]);
        assert_eq!(labels[1].files.len(), 2, "multi-file ground truth preserved");
    }

    #[test]
    fn parse_labels_skips_one_malformed_line_without_voiding_the_rest() {
        let body = "{\"query\": \"ok\", \"files\": [\"a.rs\"]}\nnot json at all\n{\"query\": \"two\", \"files\": [\"b.rs\"]}";
        let labels = parse_labels(body);
        assert_eq!(labels.len(), 2, "the malformed middle line is dropped, the valid ones survive");
    }

    #[test]
    fn rank_of_is_one_based_and_finds_first_truth() {
        let retrieved = vec!["a.rs".to_string(), "b.rs".to_string(), "c.rs".to_string()];
        assert_eq!(rank_of(&retrieved, &["b.rs".to_string()]), Some(2));
        assert_eq!(rank_of(&retrieved, &["a.rs".to_string()]), Some(1));
        // ANY ground-truth file counts — the earliest position wins.
        assert_eq!(rank_of(&retrieved, &["c.rs".to_string(), "a.rs".to_string()]), Some(1));
        // miss → None.
        assert_eq!(rank_of(&retrieved, &["z.rs".to_string()]), None);
    }

    #[test]
    fn path_eq_tolerates_separators_and_suffix() {
        assert!(path_eq("src\\Billing\\Invoice.cs", "src/Billing/Invoice.cs"), "backslash vs forward");
        assert!(path_eq("./src/a.rs", "src/a.rs"), "leading ./ stripped");
        assert!(path_eq("apps/cli/src/main.rs", "src/main.rs"), "suffix match across root prefix");
        assert!(!path_eq("src/abc.rs", "src/xyz.rs"), "distinct files do not match");
        assert!(!path_eq("src/billing.rs", "src/sub_billing.rs"), "suffix must be /-bounded, not a substring");
    }

    #[test]
    fn recall_at_counts_hits_within_k_over_total() {
        // ranks: 1, 3, miss, 6, 1 → @1 = 2/5 = 0.4 ; @5 = 3/5 = 0.6
        let ranks = vec![Some(1), Some(3), None, Some(6), Some(1)];
        assert_eq!(recall_at(&ranks, 1), 0.4);
        assert_eq!(recall_at(&ranks, 5), 0.6);
        assert_eq!(recall_at(&[], 5), 0.0, "empty input never divides by zero");
    }

    #[test]
    fn summarize_measures_the_moat_name_zero_purpose_perfect() {
        // The shape the roadmap proves: name-match 0/10, purpose-search 10/10.
        let rows: Vec<CaseResult> = (0..10).map(|_| case(None, Some(1))).collect();
        let s = summarize(&rows);
        assert_eq!(s.name_recall_1, 0.0);
        assert_eq!(s.name_recall_5, 0.0);
        assert_eq!(s.purpose_recall_1, 1.0);
        assert_eq!(s.purpose_recall_5, 1.0);
    }

    #[test]
    fn report_is_byte_stable_across_runs() {
        let rows = vec![case(None, Some(1)), case(Some(4), Some(2))];
        let report = Report { n: rows.len(), summary: summarize(&rows), cases: rows };
        let a = serde_json::to_string_pretty(&report).unwrap();
        let b = serde_json::to_string_pretty(&report).unwrap();
        assert_eq!(a, b, "two serialisations are identical");
        // The contract keys are present with their `@k` names.
        assert!(a.contains("\"purposeRecall@5\""), "contract key present: {a}");
        assert!(a.contains("\"nameRank\""), "per-case rank key present");
    }
}
