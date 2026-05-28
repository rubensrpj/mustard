//! `aho` — the `aho-corasick` wrapper that backs [`VocabularyMatcher`].
//!
//! This module isolates the dependency on the `aho-corasick` crate behind
//! [`AhoMatcher`], a small type that owns the automaton plus the parallel
//! tables used to map a pattern id back to its owning layer and original
//! term string. The split exists so:
//!
//! - The public [`crate::domain::vocabulary::VocabularyMatcher`] surface stays
//!   stable even if a future wave swaps the underlying engine (e.g. to
//!   `regex-automata`).
//! - Tests can exercise the bare automaton without going through the
//!   document/file-IO layer.
//!
//! The matcher uses leftmost-first semantics with case-sensitive matching
//! and no overlap. Case sensitivity is intentional: half of the seed
//! vocabulary is code patterns (`None`, `Vec::new()`, `Default::default()`)
//! where case carries meaning. Layer ranking — semantic > pattern > keyword
//! > noise — is enforced at construction time by deduplicating cross-layer
//! collisions in declaration order (most severe wins).
//!
//! [`VocabularyMatcher`]: crate::domain::vocabulary::VocabularyMatcher
//! [`AhoMatcher`]: self::AhoMatcher

use super::{Layer, ScanHit, VocabError, VocabLayer};
use aho_corasick::{AhoCorasick, AhoCorasickKind, MatchKind};
use std::collections::HashMap;

/// Internal matcher type. Not exposed at the crate root — consumers go
/// through [`crate::domain::vocabulary::VocabularyMatcher`].
pub(super) struct AhoMatcher {
    ac: AhoCorasick,
    // Parallel to the patterns handed to `AhoCorasick::new`. Index by
    // `Match::pattern().as_usize()` to recover the original term + layer.
    table: Vec<(Layer, String)>,
    // Per-layer counts, captured once at construction so `term_count_for`
    // is O(1).
    counts_by_layer: HashMap<Layer, usize>,
}

impl AhoMatcher {
    /// Build the automaton. See [`crate::domain::vocabulary::VocabularyMatcher::from_layers`]
    /// for the public contract — this function implements the deduplication
    /// and construction policy.
    pub(super) fn from_layers(layers: Vec<VocabLayer>) -> Result<Self, VocabError> {
        let mut seen_terms: HashMap<String, Layer> = HashMap::new();
        let mut table: Vec<(Layer, String)> = Vec::new();
        let mut counts_by_layer: HashMap<Layer, usize> = HashMap::new();

        for layer in layers {
            let mut dedup_within_layer: HashMap<String, ()> = HashMap::new();
            for term in layer.terms {
                let trimmed = term.trim().to_string();
                if trimmed.is_empty() {
                    // Empty terms would compile into an automaton that
                    // matches at every byte boundary — silently lethal
                    // for performance and correctness. Skip.
                    continue;
                }
                // Within one layer: deduplicate.
                if dedup_within_layer.insert(trimmed.clone(), ()).is_some() {
                    continue;
                }
                // Across layers: keep the first occurrence (severity wins
                // because layers were inserted in severity order at the
                // call site — `VocabularyDoc::layers` preserves TOML
                // declaration order and seed files list semantic first).
                if seen_terms.contains_key(&trimmed) {
                    continue;
                }
                seen_terms.insert(trimmed.clone(), layer.kind);
                table.push((layer.kind, trimmed));
                *counts_by_layer.entry(layer.kind).or_insert(0) += 1;
            }
        }

        if table.is_empty() {
            return Err(VocabError::NoTerms);
        }

        let patterns: Vec<&str> = table.iter().map(|(_, t)| t.as_str()).collect();
        // `LeftmostFirst` matches the priority order of the patterns passed
        // in (we already sorted by layer severity above, so the first hit
        // wins). `DFA` is the fastest variant for static term lists.
        let ac = AhoCorasick::builder()
            .match_kind(MatchKind::LeftmostFirst)
            .kind(Some(AhoCorasickKind::DFA))
            .build(patterns)
            .map_err(|e| VocabError::InvalidToml(format!("aho-corasick build: {e}")))?;

        Ok(Self {
            ac,
            table,
            counts_by_layer,
        })
    }

    /// Scan a haystack and emit one [`ScanHit`] per match.
    pub(super) fn scan(&self, haystack: &str) -> Vec<ScanHit> {
        self.ac
            .find_iter(haystack)
            .filter_map(|m| {
                let idx = m.pattern().as_usize();
                let (layer, term) = self.table.get(idx)?;
                Some(ScanHit {
                    layer: *layer,
                    term: term.clone(),
                    start: m.start(),
                    end: m.end(),
                })
            })
            .collect()
    }

    /// Total number of distinct terms across every layer.
    pub(super) fn term_count(&self) -> usize {
        self.table.len()
    }

    /// Number of distinct terms inside one layer.
    pub(super) fn term_count_for(&self, layer: Layer) -> usize {
        self.counts_by_layer.get(&layer).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aho_matcher_skips_empty_terms() {
        let m = AhoMatcher::from_layers(vec![VocabLayer {
            kind: Layer::Semantic,
            terms: vec!["fail-open".into(), "".into(), "   ".into()],
        }])
        .unwrap();
        assert_eq!(m.term_count(), 1);
    }

    #[test]
    fn aho_matcher_is_case_sensitive() {
        let m = AhoMatcher::from_layers(vec![VocabLayer {
            kind: Layer::Pattern,
            terms: vec!["None".into()],
        }])
        .unwrap();
        // Lowercase `none` must not match the capitalised pattern.
        assert!(m.scan("none here").is_empty());
        assert_eq!(m.scan("None here").len(), 1);
    }

    #[test]
    fn aho_matcher_leftmost_first_picks_severe_layer_on_collision() {
        // Two layers contain the same term; the first (more severe) wins.
        let m = AhoMatcher::from_layers(vec![
            VocabLayer {
                kind: Layer::Semantic,
                terms: vec!["stub".into()],
            },
            VocabLayer {
                kind: Layer::Keyword,
                terms: vec!["stub".into()],
            },
        ])
        .unwrap();
        let hits = m.scan("we should stub this");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].layer, Layer::Semantic);
    }
}
