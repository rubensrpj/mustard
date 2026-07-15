//! `aho` — the `aho-corasick` engine wrapper that backs [`VocabularyMatcher`]
//! and the framework-signal detector.
//!
//! This module isolates the dependency on the `aho-corasick` crate behind one
//! generic primitive, [`KeyedAutomaton`], that owns the automaton plus the
//! parallel tables mapping a pattern id back to its owning *key* and original
//! term string. The split exists so:
//!
//! - The public [`crate::domain::vocabulary::VocabularyMatcher`] surface stays
//!   stable even if a future wave swaps the underlying engine (e.g. to
//!   `regex-automata`).
//! - The engine is built **once**: the regression matcher keys hits on
//!   [`Layer`], the framework detector keys hits on a category — both reuse
//!   the same [`KeyedAutomaton`] construction + scan path instead of each
//!   wiring its own `AhoCorasick`.
//! - Tests can exercise the bare automaton without going through the
//!   document/file-IO layer.
//!
//! The matcher uses leftmost-first semantics with case-sensitive matching
//! and no overlap. Case sensitivity is intentional: half of the seed
//! vocabulary is code patterns (`None`, `Vec::new()`, `Default::default()`)
//! where case carries meaning. Key ranking — for the regression layers,
//! semantic > pattern > keyword > noise — is enforced at construction time by
//! deduplicating cross-key collisions in declaration order (first key wins).
//!
//! [`VocabularyMatcher`]: crate::domain::vocabulary::VocabularyMatcher
//! [`AhoMatcher`]: self::AhoMatcher
//! [`KeyedAutomaton`]: self::KeyedAutomaton

use super::{Layer, ScanHit, VocabError, VocabLayer};
use aho_corasick::{AhoCorasick, AhoCorasickKind, MatchKind};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// One generic match emitted by [`KeyedAutomaton::scan`]: the key the term
/// belongs to, the original term, and the byte span in the haystack.
pub(super) struct KeyedHit<K> {
    pub(super) key: K,
    pub(super) term: String,
    pub(super) start: usize,
    pub(super) end: usize,
}

/// The shared `aho-corasick` engine, generic over the *key* each term is
/// tagged with. The regression matcher instantiates `K = Layer`. Construction
/// deduplicates terms within a key and across keys (first key wins), then
/// builds a single leftmost-first DFA over the surviving terms.
///
/// This is the only place in the crate that touches the `aho-corasick` API —
/// every multi-pattern scan goes through here so the engine is never
/// duplicated.
pub(super) struct KeyedAutomaton<K> {
    ac: AhoCorasick,
    // Parallel to the patterns handed to `AhoCorasick::new`. Index by
    // `Match::pattern().as_usize()` to recover the original term + key.
    table: Vec<(K, String)>,
    // Per-key counts, captured once at construction so `term_count_for`
    // is O(1).
    counts_by_key: HashMap<K, usize>,
}

impl<K: Copy + Eq + Hash> KeyedAutomaton<K> {
    /// Build the automaton from an ordered list of `(key, terms)` groups.
    ///
    /// Dedup policy: within one group, repeated terms collapse; across groups,
    /// the *first* occurrence of a term wins (so callers that want a ranking —
    /// e.g. severity, or "most specific category first" — simply pass the
    /// groups in priority order). Empty / whitespace-only terms are skipped:
    /// they would compile into an automaton that matches at every byte
    /// boundary, which is silently lethal for performance and correctness.
    ///
    /// Returns [`VocabError::NoTerms`] when no non-empty term survives.
    pub(super) fn from_groups(
        groups: impl IntoIterator<Item = (K, Vec<String>)>,
    ) -> Result<Self, VocabError> {
        let mut seen_terms: HashSet<String> = HashSet::new();
        let mut table: Vec<(K, String)> = Vec::new();
        let mut counts_by_key: HashMap<K, usize> = HashMap::new();

        for (key, terms) in groups {
            let mut dedup_within_group: HashSet<String> = HashSet::new();
            for term in terms {
                let trimmed = term.trim().to_string();
                if trimmed.is_empty() {
                    continue;
                }
                // Within one group: deduplicate.
                if !dedup_within_group.insert(trimmed.clone()) {
                    continue;
                }
                // Across groups: keep the first occurrence (group order is the
                // caller's priority order).
                if seen_terms.contains(&trimmed) {
                    continue;
                }
                seen_terms.insert(trimmed.clone());
                table.push((key, trimmed));
                *counts_by_key.entry(key).or_insert(0) += 1;
            }
        }

        if table.is_empty() {
            return Err(VocabError::NoTerms);
        }

        let patterns: Vec<&str> = table.iter().map(|(_, t)| t.as_str()).collect();
        // `LeftmostFirst` matches the priority order of the patterns passed
        // in (already sorted by the caller's group order above, so the first
        // hit wins). `DFA` is the fastest variant for static term lists.
        let ac = AhoCorasick::builder()
            .match_kind(MatchKind::LeftmostFirst)
            .kind(Some(AhoCorasickKind::DFA))
            .build(patterns)
            .map_err(|e| VocabError::InvalidToml(format!("aho-corasick build: {e}")))?;

        Ok(Self {
            ac,
            table,
            counts_by_key,
        })
    }

    /// Scan a haystack and emit one [`KeyedHit`] per match, left to right.
    pub(super) fn scan(&self, haystack: &str) -> Vec<KeyedHit<K>> {
        self.ac
            .find_iter(haystack)
            .filter_map(|m| {
                let idx = m.pattern().as_usize();
                let (key, term) = self.table.get(idx)?;
                Some(KeyedHit {
                    key: *key,
                    term: term.clone(),
                    start: m.start(),
                    end: m.end(),
                })
            })
            .collect()
    }

    /// Total number of distinct terms across every key.
    pub(super) fn term_count(&self) -> usize {
        self.table.len()
    }

    /// Number of distinct terms tagged with one key.
    pub(super) fn term_count_for(&self, key: K) -> usize {
        self.counts_by_key.get(&key).copied().unwrap_or(0)
    }
}

/// The regression-gate matcher: a [`KeyedAutomaton`] keyed on [`Layer`]. Not
/// exposed at the crate root — consumers go through
/// [`crate::domain::vocabulary::VocabularyMatcher`].
pub(super) struct AhoMatcher {
    inner: KeyedAutomaton<Layer>,
}

impl AhoMatcher {
    /// Build the automaton. See [`crate::domain::vocabulary::VocabularyMatcher::from_layers`]
    /// for the public contract — this delegates the dedup + construction
    /// policy to [`KeyedAutomaton::from_groups`], passing layers in declaration
    /// order so the most-severe layer wins a cross-layer term collision.
    pub(super) fn from_layers(layers: Vec<VocabLayer>) -> Result<Self, VocabError> {
        let groups = layers.into_iter().map(|l| (l.kind, l.terms));
        Ok(Self {
            inner: KeyedAutomaton::from_groups(groups)?,
        })
    }

    /// Scan a haystack and emit one [`ScanHit`] per match.
    pub(super) fn scan(&self, haystack: &str) -> Vec<ScanHit> {
        self.inner
            .scan(haystack)
            .into_iter()
            .map(|h| ScanHit {
                layer: h.key,
                term: h.term,
                start: h.start,
                end: h.end,
            })
            .collect()
    }

    /// Total number of distinct terms across every layer.
    pub(super) fn term_count(&self) -> usize {
        self.inner.term_count()
    }

    /// Number of distinct terms inside one layer.
    pub(super) fn term_count_for(&self, layer: Layer) -> usize {
        self.inner.term_count_for(layer)
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
