//! Retrieval shaping for `feature` — the projection cluster lifted out of
//! `feature.rs` so that module stays pure orchestration. Every function here is
//! PURE (no spawn, no IO): the digest already ran ONCE inside the
//! `scan feature-bundle` call, so these only shape its anchor audit.
//!
//! Two products, both ordered by the digest's own score (desc, path asc):
//!   * `insumos` — the top-[`INSUMOS_MAX`] short-list (`{file}`).
//!   * `candidates` — the wider [`POOL_MAX`] pool with per-file evidence, the
//!     in-session selection menu the orchestrator picks 5-10 files from.

use mustard_core::domain::scan::FileDetail;
use serde_json::{json, Value};

/// Short-list length (top 10 — the measured Acc@10 operating point).
pub(super) const INSUMOS_MAX: usize = 10;

/// Candidate-pool size published as `candidates` (wider than the deterministic
/// top-10 so the in-session selector sees past the short-list cut).
pub(super) const POOL_MAX: usize = 25;

/// How many candidates are PUBLISHED when the report came back `strong`.
///
/// The pool itself stays [`POOL_MAX`] wide — it feeds, crucially,
/// the `uncovered` absence radar, which must see every candidate or it reports
/// covered concepts as blind spots. Only the published slice narrows.
///
/// Twelve, because the instruction that consumes this field tells the reader to
/// pick "the 5-10 files a developer would open — never all ~25". Publishing 25
/// where at most 10 may be used charges the window for 15 rows that the contract
/// itself forbids using; publishing exactly 10 would leave the selector no
/// margin above its own ceiling.
pub(super) const STRONG_POOL_MAX: usize = 12;

/// Max matched terms rendered per candidate evidence line (payload budget).
const TERMS_SHOWN: usize = 6;

/// Order the digest's anchor audit into ONE ranked file list: max
/// `score_x1024` per file, score desc, tie → path asc; separators normalised to
/// `/`. Pure + byte-stable.
fn digest_ranked_files(detail: &[FileDetail]) -> Vec<String> {
    use std::collections::BTreeMap;
    let mut best: BTreeMap<String, u64> = BTreeMap::new();
    for d in detail {
        let file = d.file.replace('\\', "/");
        let e = best.entry(file).or_insert(0);
        *e = (*e).max(d.score_x1024);
    }
    let mut rows: Vec<(String, u64)> = best.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    rows.into_iter().map(|(f, _)| f).collect()
}

/// Build the `insumos` rows from the digest anchor audit: the top-[`INSUMOS_MAX`]
/// files, in the audit's own order. Pure + byte-stable.
pub(super) fn insumos_rows(detail: &[FileDetail]) -> Vec<Value> {
    let mut files = digest_ranked_files(detail);
    files.truncate(INSUMOS_MAX);
    files.into_iter().map(|file| json!({ "file": file })).collect()
}

/// One row of the candidate pool: the file plus its deterministic evidence —
/// the 1-based position in the digest audit and the matched terms that carry
/// it. Fields are `pub(super)` so `feature.rs` (the parent) can read the pool.
pub(super) struct Candidate {
    pub(super) file: String,
    pub(super) digest_pos: Option<usize>,
    pub(super) terms: Vec<String>,
}

/// Order the digest's anchor audit for the candidate pool: the same ordering
/// contract as [`digest_ranked_files`] (max `score_x1024` per file desc, path
/// asc; separators normalised) but keeping each file's matched-term evidence
/// (first-occurrence order across duplicates, deduped). Pure + byte-stable.
fn digest_pool(detail: &[FileDetail]) -> Vec<(String, Vec<String>)> {
    use std::collections::BTreeMap;
    let mut best: BTreeMap<String, u64> = BTreeMap::new();
    let mut terms: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for d in detail {
        let file = d.file.replace('\\', "/");
        let e = best.entry(file.clone()).or_insert(0);
        *e = (*e).max(d.score_x1024);
        let t = terms.entry(file).or_default();
        for term in &d.terms {
            if !t.contains(term) {
                t.push(term.clone());
            }
        }
    }
    let mut rows: Vec<(String, u64)> = best.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    rows.into_iter()
        .map(|(f, _)| {
            let t = terms.remove(&f).unwrap_or_default();
            (f, t)
        })
        .collect()
}

/// Build the candidate pool from the digest anchor audit, capped at
/// [`POOL_MAX`]: each row keeps its 1-based position and its matched terms.
/// Pure: the digest already ran inside the bundle.
pub(super) fn build_pool(detail: &[FileDetail]) -> Vec<Candidate> {
    digest_pool(detail)
        .into_iter()
        .enumerate()
        .take(POOL_MAX)
        .map(|(i, (file, terms))| Candidate { file, digest_pos: Some(i + 1), terms })
        .collect()
}

/// Project the pool into the `candidates` payload rows: per file, ONE compact
/// `evidence` line — the 1-based position plus up to [`TERMS_SHOWN`] matched
/// terms — so the in-session selector reads WHY each row is offered without any
/// second lookup. Pure + byte-stable: the pool order is preserved verbatim.
pub(super) fn candidates_rows(pool: &[Candidate]) -> Vec<Value> {
    pool.iter()
        .map(|c| {
            let mut ev: Vec<String> = Vec::new();
            if let Some(d) = c.digest_pos {
                ev.push(format!("digest#{d}"));
            }
            if !c.terms.is_empty() {
                let shown: Vec<&str> = c.terms.iter().take(TERMS_SHOWN).map(String::as_str).collect();
                ev.push(format!("terms={}", shown.join(",")));
            }
            json!({ "file": c.file, "evidence": ev.join(" ") })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_ranked_files_takes_max_per_file_desc_with_path_tiebreak() {
        // Duplicate file (backslash variant): max score wins; the 90-tie
        // between a.cs and dup.cs breaks by path asc; separators normalise.
        let detail: Vec<FileDetail> = serde_json::from_str(
            r#"[{"file":"src\\dup.cs","score_x1024":10,"terms":[]},
                {"file":"src/dup.cs","score_x1024":90,"terms":[]},
                {"file":"src/a.cs","score_x1024":90,"terms":[]},
                {"file":"src/z.cs","score_x1024":200,"terms":[]}]"#,
        )
        .expect("detail rows");
        assert_eq!(digest_ranked_files(&detail), vec!["src/z.cs", "src/a.cs", "src/dup.cs"]);
        assert!(digest_ranked_files(&[]).is_empty());
    }

    #[test]
    fn digest_pool_keeps_order_and_merges_term_evidence() {
        // Same ordering contract as digest_ranked_files (max score per file,
        // desc, path asc) with the per-file terms unioned across duplicates.
        let detail: Vec<FileDetail> = serde_json::from_str(
            r#"[{"file":"src\\dup.cs","score_x1024":10,"terms":["contrato"]},
                {"file":"src/dup.cs","score_x1024":90,"terms":["parcela","contrato"]},
                {"file":"src/z.cs","score_x1024":200,"terms":[]}]"#,
        )
        .expect("detail rows");
        let pool = digest_pool(&detail);
        assert_eq!(pool[0].0, "src/z.cs");
        assert_eq!(pool[0].1, Vec::<String>::new());
        assert_eq!(pool[1].0, "src/dup.cs");
        assert_eq!(pool[1].1, vec!["contrato".to_string(), "parcela".to_string()], "terms unioned, first-occurrence order");
        assert!(digest_pool(&[]).is_empty());
    }

}