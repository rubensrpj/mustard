//! Deterministic PURPOSE search — find the files whose declarations' `purpose`
//! summaries answer a free-text intent, INDEPENDENT of the name index.
//!
//! ## Why a standalone command (not part of `digest --query`)
//!
//! The `digest` anchor pipeline ranks files by BM25F over the NAME term index,
//! which is capped (`MAX_TERMS`) and gated. A method whose NAME diverges from
//! the request vocabulary (the whole point of the `purpose` enrichment — e.g. a
//! PT request "efetivar" against `EffectivateAsync`) ranks far below the cut and
//! is evicted from the anchor list when weak name matches fill it. Folding the
//! purpose recall into that pipeline kept losing the file to the cap.
//!
//! So purpose recall is its own command. The orchestrator calls it ON A MISS:
//! it builds an UNCAPPED index over EVERY declaration that carries a `purpose`,
//! matches the intent's tokens against the purpose tokens through the SAME match
//! ladder `digest --query` uses (so the bridging is identical), and returns the
//! files ranked by the summed IDF (corpus rarity) of the query tokens each
//! answers — so a file bridging one rare/discriminative term outranks one that
//! merely shares a common word.
//!
//! ## Determinism — no LLM
//!
//! The purposes are already in the model (written once by `enrich-purpose
//! --apply`). This command only READS them. The ladder is the same deterministic
//! `matching::Ladder`; the output is byte-stable JSON (BTreeMap index, sorted
//! files, sorted matched terms). Fail-open: an unreadable / unparseable model
//! yields an empty result, never an error.
//!
//! ## The trigram rung is REQUIRED here
//!
//! Unlike the strict name ladder, purpose matching enables the T5 trigram RESCUE
//! rung (`ladder.tier(.., true)`). The English stemmer has gaps on the inflected
//! forms the summaries use — "efetiva"↔"efetivar" and "baixado"↔"baixa" both go
//! strict=None / trigram=Some — so without it ~half the real recall-holes never
//! bridge. Precision is not a concern: this command runs only when the name
//! search already missed, and its hits are a re-query suggestion, not an
//! authoritative anchor.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::matching::{Ladder, Sig};
use crate::model::ProjectModel;

/// Anchor cap — same handful the digest's per-query anchor list returns, so the
/// orchestrator reads a bounded set of files to confirm by reading.
const MAX_FILES: usize = 12;

/// One file the purpose index answered, with the query tokens its declarations'
/// purpose summaries bridged. `matched_terms` is sorted (byte-stable).
#[derive(Serialize)]
pub(crate) struct PurposeHit {
    pub file: String,
    #[serde(rename = "matchedTerms")]
    pub matched_terms: Vec<String>,
}

/// The byte-stable result of a purpose search.
#[derive(Serialize)]
pub struct PurposeResult {
    /// The query tokens actually searched (filtered + deduped), joined with ' '.
    pub intent: String,
    /// Files whose purpose answered ≥1 query token, ranked by distinct-token
    /// count desc then path asc, capped at [`MAX_FILES`].
    pub files: Vec<PurposeHit>,
}

/// Prepare the intent's query tokens exactly as `digest::query` does: trimmed,
/// lowercased, length-floored at 3, identifier-glue AND natural-language-glue
/// (ladder stoplist) filtered, then an order-preserving dedup. Same contract, so
/// a purpose search and a name search tokenise an intent identically.
fn query_tokens(terms: &[String], ladder: &Ladder) -> Vec<String> {
    let stop = crate::digest::stopwords();
    let mut ql: Vec<String> = terms
        .iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| s.len() >= 3 && !stop.contains(s) && !ladder.query_stopword(s))
        .collect();
    let mut seen = BTreeSet::new();
    ql.retain(|t| seen.insert(t.clone()));
    ql
}

/// Build the UNCAPPED purpose→file index and rank the files answering `terms`.
///
/// Pure given its inputs and the embedded ladder data: a BTreeMap keyed by file
/// path, query tokens compared through the ladder with the trigram rescue rung
/// ON, files ranked by summed token IDF desc then path asc (a total order, so
/// two runs over the same model are byte-identical).
pub fn search(model: &ProjectModel, terms: &[String]) -> PurposeResult {
    let ladder = Ladder::new();
    let ql = query_tokens(terms, &ladder);
    let qsigs: Vec<Sig> = ql.iter().map(|q| ladder.sig(q)).collect();

    // file path -> the ladder sigs of its declarations' purpose tokens. Built
    // from the FULL model (every purposed declaration) — NO term cap. Test and
    // machine-written files are excluded (you confirm a production file, never
    // its test). BTreeMap → deterministic file iteration order.
    let mut purpose_index: BTreeMap<&str, Vec<Sig>> = BTreeMap::new();
    for m in &model.modules {
        if mustard_core::domain::ast::is_test_path(&m.path)
            || !crate::classify::anchor_eligible(&m.file_class)
        {
            continue;
        }
        for d in &m.declarations {
            let Some(ref p) = d.purpose else { continue };
            if p.is_empty() {
                continue;
            }
            let entry = purpose_index.entry(m.path.as_str()).or_default();
            for tok in p.split(|ch: char| !ch.is_alphanumeric()) {
                if tok.len() >= 3 {
                    entry.push(ladder.sig(&tok.to_lowercase()));
                }
            }
        }
    }

    // First pass: per file, the DISTINCT query-token INDICES its purpose sigs
    // bridge (trigram RESCUE allowed — see the module note). The bridge direction
    // mirrors the name ladder: the purpose sig is the index side, the query sig is
    // the request side. Accumulate each token's DOCUMENT FREQUENCY (how many
    // purposed files answer it) so the rank can weight a rare term over a common.
    let n_docs = purpose_index.len();
    let mut df: Vec<usize> = vec![0; ql.len()];
    let mut per_file: Vec<(&str, Vec<usize>)> = Vec::new();
    for (file, psigs) in &purpose_index {
        let mut idxs: Vec<usize> = Vec::new();
        for (qi, qs) in qsigs.iter().enumerate() {
            if psigs.iter().any(|ps| ladder.tier(ps, qs, true).is_some()) {
                idxs.push(qi);
            }
        }
        if !idxs.is_empty() {
            for &qi in &idxs {
                df[qi] += 1;
            }
            per_file.push((*file, idxs));
        }
    }

    // Second pass: score each file by the SUM of its matched tokens' IDF — the
    // SAME `ranking::idf_x1024` (corpus rarity) the digest's BM25F anchor ranking
    // uses (DRY). A file that bridges ONE rare/discriminative term (e.g.
    // "conciliar", df≈1 → high idf) thus outranks one that merely shares common
    // words (e.g. "recebível"/"lançamento", high df → idf≈0) — fixing the bug
    // where equal-weight token COUNT buried the real target. Rank by score desc,
    // then path asc (total order, byte-stable).
    let mut hits: Vec<(u64, String, Vec<String>)> = per_file
        .into_iter()
        .map(|(file, idxs)| {
            let score: u64 = idxs
                .iter()
                .map(|&qi| mustard_core::domain::ranking::idf_x1024(df[qi], n_docs))
                .sum();
            let mut terms: Vec<String> = idxs.iter().map(|&qi| ql[qi].clone()).collect();
            terms.sort();
            (score, file.to_string(), terms)
        })
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    hits.truncate(MAX_FILES);

    PurposeResult {
        intent: ql.join(" "),
        files: hits
            .into_iter()
            .map(|(_, file, matched_terms)| PurposeHit { file, matched_terms })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Decl, Module};

    /// A module with one declaration carrying `purpose` (or none).
    fn module(path: &str, name: &str, purpose: Option<&str>) -> Module {
        Module {
            path: path.to_string(),
            language: "rust".to_string(),
            loc: 10,
            imports: Vec::new(),
            namespaces: Vec::new(),
            declarations: vec![Decl {
                kind: "function".to_string(),
                name: name.to_string(),
                line: 1,
                supertypes: Vec::new(),
                purpose: purpose.map(str::to_string),
                body_hash: None,
            }],
            file_class: String::new(),
            marker: String::new(),
            fan_in: 0,
        }
    }

    /// (a) UNCAPPED: a model far larger than the digest's MAX_TERMS cap, where
    /// ONE rare-named declaration carries a purpose with the query word, must
    /// still return that file — the cap that evicted it from the name pipeline
    /// does not exist here (the index is built straight from the model).
    #[test]
    fn purpose_search_is_uncapped_by_term_cap() {
        let mut modules: Vec<Module> = (0..160)
            .map(|i| module(&format!("src/common/handler_{i}.rs"), &format!("ProcessHandler{i}"), None))
            .collect();
        modules.push(module(
            "src/payments/xqzkflow.rs",
            "Xqzkflow", // a name that tokenizes to nothing the query matches
            Some("aprovado o título a pagar"),
        ));
        let model = ProjectModel { modules, ..Default::default() };
        let result = search(&model, &["aprovar".to_string()]);
        assert!(
            result.files.iter().any(|h| h.file == "src/payments/xqzkflow.rs"),
            "rare-named target's purpose must be found past the cap; got {:?}",
            result.files.iter().map(|h| h.file.as_str()).collect::<Vec<_>>(),
        );
    }

    /// (b) TRIGRAM RESCUE: the PT Snowball stemmer cannot bridge
    /// "efetiva"↔"efetivar" (strict=None), but the trigram rung does. Purpose
    /// "efetiva o título" queried with the base form "efetivar" must return the
    /// file — the failure mode the strict pass shipped at ~0/10.
    #[test]
    fn purpose_search_bridges_stemmer_gap_via_trigram() {
        let model = ProjectModel {
            modules: vec![module("src/payment.rs", "Xyz", Some("efetiva o título"))],
            ..Default::default()
        };
        let result = search(&model, &["efetivar".to_string()]);
        assert!(
            result.files.iter().any(|h| h.file == "src/payment.rs"),
            "trigram rung must bridge the PT stemmer gap; got {:?}",
            result.files.iter().map(|h| h.file.as_str()).collect::<Vec<_>>(),
        );
    }

    /// (c) RANKING: a file whose purpose answers TWO query tokens ranks above a
    /// file answering ONE.
    #[test]
    fn purpose_search_ranks_more_matches_first() {
        let model = ProjectModel {
            modules: vec![
                module("src/two.rs", "Aaa", Some("aprovado e baixado o título")),
                module("src/one.rs", "Bbb", Some("aprovado apenas")),
            ],
            ..Default::default()
        };
        let result = search(&model, &["aprovar".to_string(), "baixa".to_string()]);
        assert_eq!(result.files.first().map(|h| h.file.as_str()), Some("src/two.rs"), "two-match file heads: {:?}", result.files.iter().map(|h| (h.file.as_str(), h.matched_terms.len())).collect::<Vec<_>>());
        // The two-match file names both tokens, sorted.
        let two = result.files.iter().find(|h| h.file == "src/two.rs").expect("two.rs present");
        assert_eq!(two.matched_terms, vec!["aprovar".to_string(), "baixa".to_string()]);
    }

    /// (d) BYTE-STABILITY: two runs over the same model produce identical output.
    #[test]
    fn purpose_search_is_byte_stable() {
        let model = ProjectModel {
            modules: vec![
                module("src/b.rs", "B", Some("aprovado o título")),
                module("src/a.rs", "A", Some("aprovado o título a pagar")),
            ],
            ..Default::default()
        };
        let a = serde_json::to_string(&search(&model, &["aprovar".to_string()])).unwrap();
        let b = serde_json::to_string(&search(&model, &["aprovar".to_string()])).unwrap();
        assert_eq!(a, b, "two runs serialize identically");
        // Empty when nothing bridges — never an error.
        let none = search(&model, &["zzzznomatch".to_string()]);
        assert!(none.files.is_empty(), "no bridge → empty files");
    }

    /// (e) IDF RANKING: a file bridging ONE RARE/discriminative term outranks a
    /// file bridging TWO COMMON terms — the bug the equal-weight token COUNT had
    /// (a file sharing common words buried the real target). "conciliar" appears
    /// in ONE purpose (df=1 → high idf); "recebivel"/"lancamento" flood the corpus
    /// (df high → idf≈0), so the lone "conciliar" file must head the list above
    /// the file that matches both common words.
    #[test]
    fn purpose_search_ranks_rare_term_over_common_pair() {
        let mut modules: Vec<Module> = (0..20)
            .map(|i| module(&format!("src/filler/f{i}.rs"), &format!("Filler{i}"), Some("recebivel lancamento")))
            .collect();
        modules.push(module("src/target.rs", "Tgt", Some("conciliar a linha do extrato")));
        modules.push(module("src/common.rs", "Cmn", Some("recebivel e lancamento juntos")));
        let model = ProjectModel { modules, ..Default::default() };
        let result = search(
            &model,
            &["conciliar".to_string(), "recebivel".to_string(), "lancamento".to_string()],
        );
        assert_eq!(
            result.files.first().map(|h| h.file.as_str()),
            Some("src/target.rs"),
            "rare-term file must head over common-pair files; got {:?}",
            result.files.iter().map(|h| h.file.as_str()).collect::<Vec<_>>(),
        );
    }
}
