//! `aho` — the `aho-corasick` engine wrapper behind the stack-signature
//! detector.
//!
//! This module isolates the dependency on the `aho-corasick` crate behind one
//! generic primitive, [`KeyedAutomaton`], that owns the automaton plus the
//! parallel table mapping a pattern id back to its owning *key* and original
//! term string. The engine is built **once** and every multi-pattern scan goes
//! through the same construction + scan path instead of each caller wiring its
//! own `AhoCorasick`.
//!
//! The automaton uses leftmost-first semantics with case-sensitive matching and
//! no overlap. Case sensitivity is intentional: the terms are code patterns
//! where case carries meaning. Key ranking is enforced at construction time by
//! deduplicating cross-key collisions in declaration order (first key wins).
//!
//! [`KeyedAutomaton`]: self::KeyedAutomaton

use super::VocabError;
use aho_corasick::{AhoCorasick, AhoCorasickKind, MatchKind};
use std::collections::HashSet;

/// One generic match emitted by [`KeyedAutomaton::scan`]: the key the term
/// belongs to and the original term.
pub(crate) struct KeyedHit<K> {
    pub(crate) key: K,
    pub(crate) term: String,
}

/// The shared `aho-corasick` engine, generic over the *key* each term is
/// tagged with. Construction deduplicates terms within a key and across keys
/// (first key wins), then builds a single leftmost-first DFA over the
/// surviving terms.
///
/// This is the only place in the crate that touches the `aho-corasick` API —
/// every multi-pattern scan goes through here so the engine is never
/// duplicated.
pub(crate) struct KeyedAutomaton<K> {
    ac: AhoCorasick,
    // Parallel to the patterns handed to `AhoCorasick::new`. Index by
    // `Match::pattern().as_usize()` to recover the original term + key.
    table: Vec<(K, String)>,
}

impl<K: Copy> KeyedAutomaton<K> {
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
    pub(crate) fn from_groups(
        groups: impl IntoIterator<Item = (K, Vec<String>)>,
    ) -> Result<Self, VocabError> {
        let mut seen_terms: HashSet<String> = HashSet::new();
        let mut table: Vec<(K, String)> = Vec::new();

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

        Ok(Self { ac, table })
    }

    /// Scan a haystack and emit one [`KeyedHit`] per match, left to right.
    pub(crate) fn scan(&self, haystack: &str) -> Vec<KeyedHit<K>> {
        self.ac
            .find_iter(haystack)
            .filter_map(|m| {
                let idx = m.pattern().as_usize();
                let (key, term) = self.table.get(idx)?;
                Some(KeyedHit { key: *key, term: term.clone() })
            })
            .collect()
    }
}
