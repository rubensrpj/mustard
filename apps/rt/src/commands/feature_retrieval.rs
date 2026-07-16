//! Retrieval fusion for `feature` — the RRF / fusion cluster lifted out of
//! `feature.rs` so that module stays pure orchestration. Every function here is
//! PURE (no spawn, no IO): the ranker already ran ONCE inside the
//! `scan feature-bundle` call, so these fuse the pre-fetched rank pool with the
//! digest anchor audit — no subprocess, no cold start.
//!
//! Two products, both Reciprocal Rank Fusion (`score = Σ 1/(k + rank)`, k =
//! [`RRF_K`], score desc, path asc):
//!   * `insumos` — the top-[`INSUMOS_MAX`] short-list (`{file, source}`).
//!   * `candidates` — the wider [`POOL_MAX`] pool with per-file evidence, the
//!     in-session selection menu the orchestrator picks 5-10 files from.

use mustard_core::domain::scan::FileDetail;
use serde_json::{json, Value};

/// Fused short-list length (top 10 — the measured Acc@10 operating point).
pub(super) const INSUMOS_MAX: usize = 10;

/// Fused candidate-pool size published as `candidates` (wider than the
/// deterministic top-10 so the in-session selector sees past the RRF cut).
pub(super) const POOL_MAX: usize = 25;

/// The RRF constant (`k = 60`, the measured winner in the fusion benchmark).
const RRF_K: f64 = 60.0;

/// Max matched terms rendered per candidate evidence line (payload budget).
const TERMS_SHOWN: usize = 6;

/// Order the digest's anchor audit into ONE ranked file list for the fusion:
/// max `score_x1024` per file, score desc, tie → path asc; separators
/// normalised to `/` so the rank and digest keys join. Pure + byte-stable.
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

/// Reciprocal Rank Fusion of the two retrieval lists:
/// `score(f) = Σ_lists 1/(k + rank_f)` with 1-based ranks and `k` = [`RRF_K`];
/// sorted score desc, tie → path asc; capped at [`INSUMOS_MAX`]. Fully
/// deterministic: fixed accumulation order, no NaN possible, `total_cmp` keeps
/// the sort total. Each row carries its provenance — `rank`, `digest`, or
/// `both`. Pure (no spawn, no IO), unit-tested.
fn fuse_rrf(rank_list: &[String], digest_list: &[String]) -> Vec<(String, &'static str)> {
    use std::collections::BTreeMap;
    let mut acc: BTreeMap<String, (f64, bool, bool)> = BTreeMap::new();
    for (i, f) in rank_list.iter().enumerate() {
        let e = acc.entry(f.clone()).or_insert((0.0, false, false));
        e.0 += 1.0 / (RRF_K + (i + 1) as f64);
        e.1 = true;
    }
    for (i, f) in digest_list.iter().enumerate() {
        let e = acc.entry(f.clone()).or_insert((0.0, false, false));
        e.0 += 1.0 / (RRF_K + (i + 1) as f64);
        e.2 = true;
    }
    let mut rows: Vec<(String, f64, bool, bool)> =
        acc.into_iter().map(|(f, (s, r, d))| (f, s, r, d)).collect();
    rows.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    rows.truncate(INSUMOS_MAX);
    rows.into_iter()
        .map(|(f, _, r, d)| {
            let source = match (r, d) {
                (true, true) => "both",
                (true, false) => "rank",
                _ => "digest",
            };
            (f, source)
        })
        .collect()
}

/// Build the fused `insumos` rows from the PRE-FETCHED rank short-list (the
/// bundle's rank pool sliced to the top-[`INSUMOS_MAX`]) RRF-fused with the
/// digest anchor audit. Pure: the ranker already ran inside the bundle, so an
/// empty `rank_list` (no dictionary) degrades to the digest list alone.
pub(super) fn insumos_rows(rank_list: &[String], detail: &[FileDetail]) -> Vec<Value> {
    let digest_list = digest_ranked_files(detail);
    fuse_rrf(rank_list, &digest_list)
        .into_iter()
        .map(|(file, source)| json!({ "file": file, "source": source }))
        .collect()
}

/// One row of the fused candidate pool: the file plus its deterministic
/// evidence — which list(s) surfaced it (`rank` / `digest` / `both`), the
/// 1-based position in each, and the matched terms that carry it. Fields are
/// `pub(super)` so `feature.rs` (the parent) can read the pool and build one.
pub(super) struct Candidate {
    pub(super) file: String,
    pub(super) source: &'static str,
    pub(super) rank_pos: Option<usize>,
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

/// RRF-fuse the two evidence-carrying lists into the candidate pool: the same
/// arithmetic as [`fuse_rrf`] (`Σ 1/(k + rank)`, k = [`RRF_K`], score desc, path
/// asc) but keeping each row's provenance — 1-based position per list + the
/// union of matched terms (rank-side first, first-occurrence order) — capped at
/// `cap`. Pure + deterministic; [`fuse_rrf`] stays untouched (the `insumos`
/// byte contract).
fn fuse_pool(
    rank_list: &[(String, Vec<String>)],
    digest_list: &[(String, Vec<String>)],
    cap: usize,
) -> Vec<Candidate> {
    use std::collections::BTreeMap;
    struct Acc {
        score: f64,
        rank_pos: Option<usize>,
        digest_pos: Option<usize>,
        terms: Vec<String>,
    }
    let mut acc: BTreeMap<String, Acc> = BTreeMap::new();
    let mut add = |file: &str, pos: usize, terms: &[String], from_rank: bool| {
        let e = acc
            .entry(file.to_string())
            .or_insert(Acc { score: 0.0, rank_pos: None, digest_pos: None, terms: Vec::new() });
        e.score += 1.0 / (RRF_K + (pos + 1) as f64);
        if from_rank {
            e.rank_pos = Some(pos + 1);
        } else {
            e.digest_pos = Some(pos + 1);
        }
        for t in terms {
            if !e.terms.contains(t) {
                e.terms.push(t.clone());
            }
        }
    };
    for (i, (f, terms)) in rank_list.iter().enumerate() {
        add(f, i, terms, true);
    }
    for (i, (f, terms)) in digest_list.iter().enumerate() {
        add(f, i, terms, false);
    }
    let mut rows: Vec<(String, Acc)> = acc.into_iter().collect();
    rows.sort_by(|a, b| b.1.score.total_cmp(&a.1.score).then_with(|| a.0.cmp(&b.0)));
    rows.truncate(cap);
    rows.into_iter()
        .map(|(file, a)| Candidate {
            file,
            source: match (a.rank_pos.is_some(), a.digest_pos.is_some()) {
                (true, true) => "both",
                (true, false) => "rank",
                _ => "digest",
            },
            rank_pos: a.rank_pos,
            digest_pos: a.digest_pos,
            terms: a.terms,
        })
        .collect()
}

/// Build the candidate pool from the PRE-FETCHED rank rows (the bundle's rank
/// pool WITH per-file terms) RRF-fused with the digest side (the anchor audit),
/// capped at [`POOL_MAX`]. Pure: the ranker already ran inside the bundle. An
/// empty `rank_rows` degrades to the digest side alone; an empty digest to the
/// rank side alone.
pub(super) fn build_pool(rank_rows: &[(String, Vec<String>)], detail: &[FileDetail]) -> Vec<Candidate> {
    fuse_pool(rank_rows, &digest_pool(detail), POOL_MAX)
}

/// Project the fused pool into the `candidates` payload rows: per file, the
/// provenance `source` and ONE compact `evidence` line — 1-based position per
/// list plus up to [`TERMS_SHOWN`] matched terms — so the in-session selector
/// reads WHY each row is offered without any second lookup. Pure + byte-stable:
/// the pool order (RRF score desc, path asc) is preserved verbatim.
pub(super) fn candidates_rows(pool: &[Candidate]) -> Vec<Value> {
    pool.iter()
        .map(|c| {
            let mut ev: Vec<String> = Vec::new();
            if let Some(r) = c.rank_pos {
                ev.push(format!("rank#{r}"));
            }
            if let Some(d) = c.digest_pos {
                ev.push(format!("digest#{d}"));
            }
            if !c.terms.is_empty() {
                let shown: Vec<&str> = c.terms.iter().take(TERMS_SHOWN).map(String::as_str).collect();
                ev.push(format!("terms={}", shown.join(",")));
            }
            json!({ "file": c.file, "source": c.source, "evidence": ev.join(" ") })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrf_fusion_scores_ties_and_caps_deterministically() {
        // b rides in BOTH lists (1/62 + 1/62) and beats a (rank #1 alone,
        // 1/61) and c (digest #2 alone, 1/62); provenance is tagged per row.
        let fused = fuse_rrf(
            &["a".to_string(), "b".to_string()],
            &["b".to_string(), "c".to_string()],
        );
        assert_eq!(
            fused,
            vec![
                ("b".to_string(), "both"),
                ("a".to_string(), "rank"),
                ("c".to_string(), "digest"),
            ]
        );
        // An exact score tie (same rank, different lists) breaks by path asc.
        let tie = fuse_rrf(&["y".to_string()], &["x".to_string()]);
        assert_eq!(tie, vec![("x".to_string(), "digest"), ("y".to_string(), "rank")]);
        // Deterministic: the same inputs fuse to the same output, twice.
        let a = fuse_rrf(&["a".into(), "b".into()], &["b".into(), "c".into()]);
        let b = fuse_rrf(&["a".into(), "b".into()], &["b".into(), "c".into()]);
        assert_eq!(a, b);
        // Capped at INSUMOS_MAX; empty ∪ empty → empty.
        let many: Vec<String> = (0..15).map(|i| format!("f{i:02}")).collect();
        assert_eq!(fuse_rrf(&many, &[]).len(), INSUMOS_MAX);
        assert!(fuse_rrf(&[], &[]).is_empty());
    }

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

    #[test]
    fn fuse_pool_carries_positions_sources_terms_and_caps() {
        let rank: Vec<(String, Vec<String>)> = vec![
            ("a".into(), vec!["aging".into()]),
            ("b".into(), vec!["payable".into()]),
        ];
        let digest: Vec<(String, Vec<String>)> = vec![
            ("b".into(), vec!["vencimento".into(), "payable".into()]),
            ("c".into(), vec![]),
        ];
        let pool = fuse_pool(&rank, &digest, 25);
        // b rides both lists → top, source both, positions kept, terms unioned.
        assert_eq!(pool[0].file, "b");
        assert_eq!(pool[0].source, "both");
        assert_eq!(pool[0].rank_pos, Some(2));
        assert_eq!(pool[0].digest_pos, Some(1));
        assert_eq!(pool[0].terms, vec!["payable".to_string(), "vencimento".to_string()]);
        assert_eq!(pool[1].file, "a");
        assert_eq!(pool[1].source, "rank");
        assert_eq!(pool[1].digest_pos, None);
        assert_eq!(pool[2].file, "c");
        assert_eq!(pool[2].source, "digest");
        // The wide cap widens past INSUMOS_MAX and truncates at the requested cap.
        let many: Vec<(String, Vec<String>)> = (0..30).map(|i| (format!("f{i:02}"), Vec::new())).collect();
        assert_eq!(fuse_pool(&many, &[], 25).len(), 25);
        assert_eq!(fuse_pool(&many, &[], 3).len(), 3);
        // Deterministic: same inputs, same pool, twice.
        let a: Vec<String> = fuse_pool(&rank, &digest, 25).into_iter().map(|c| c.file).collect();
        let b: Vec<String> = fuse_pool(&rank, &digest, 25).into_iter().map(|c| c.file).collect();
        assert_eq!(a, b);
    }
}