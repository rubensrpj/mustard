//! Token-budget primitive for prompt pruning.
//!
//! Spec A v4 / W6 — supports `resume_bootstrap`'s ≤10k-token discipline.
//! Estimator uses the 4-chars-per-token heuristic shared by the rest of the
//! crate (see `agent_prompt_render::chars_to_tokens`, W8.T8.9 — kept private
//! there because it is a leaf helper; we re-state the same arithmetic here so
//! the budget primitive does not have to reach into another module's internals).
//!
//! ## API
//!
//! - [`estimate_tokens`] — pure function: chars / 4 (round up).
//! - [`PrioritizedItem`] — one candidate text + its priority (higher = keep first).
//! - [`prune_to_budget`] — given a candidate list **already ordered by priority**,
//!   return the prefix that fits within `budget` tokens.
//!
//! ## Design
//!
//! - No I/O, no clock, no allocation beyond the returned `Vec<&PrioritizedItem>`.
//! - Caller is responsible for ordering: this primitive never re-sorts. The
//!   reason is composability — the same prune call backs different orderings
//!   (priority, recency, wikilink-match score) without duplicating the cap loop.
//! - Greedy + monotonic: once a candidate is dropped, every subsequent one is
//!   inspected but kept only if it still fits. This preserves head ordering
//!   (the orchestrator's "must-have" prefix) while letting smaller tail items
//!   slip in when there is residual headroom.

/// Conventional 4-chars-per-token heuristic.
///
/// Mirrors `agent_prompt_render::chars_to_tokens` (private there). Counts
/// `chars()` not `len()` so multi-byte UTF-8 (PT-BR diacritics, CJK) is not
/// over-estimated by a factor of 2-4×.
#[must_use]
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// One candidate slot in [`prune_to_budget`]'s input.
///
/// `priority` is informational — callers MUST sort by it before calling
/// [`prune_to_budget`]. We keep the field public so consumers can inspect /
/// debug the ordering they passed in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrioritizedItem {
    /// The candidate text (one summary file, one memory note, …).
    pub text: String,
    /// Caller-assigned priority. Higher = more important. Not used by
    /// [`prune_to_budget`]; documented as a contract: the caller sorts.
    pub priority: u8,
}

impl PrioritizedItem {
    /// Convenience constructor.
    #[must_use]
    pub fn new(text: impl Into<String>, priority: u8) -> Self {
        Self {
            text: text.into(),
            priority,
        }
    }
}

/// Greedily select candidates from `candidates` whose combined estimated
/// token count fits within `budget`.
///
/// `candidates` must already be ordered by priority (highest first). The
/// returned slice is a borrow of the original — no allocation of the texts
/// themselves. The first item that does not fit is **skipped**; later, smaller
/// items can still slip in. This keeps the head prefix intact while filling
/// any residual budget with tail content.
///
/// ## Edge cases
///
/// - `budget == 0` → empty result.
/// - A single item that, alone, exceeds the budget → dropped (never partially
///   emitted). Callers worried about a "single must-keep item too big" case
///   should pre-clip the text or raise the budget.
#[must_use]
pub fn prune_to_budget<'a>(
    candidates: &'a [PrioritizedItem],
    budget: usize,
) -> Vec<&'a PrioritizedItem> {
    let mut kept = Vec::with_capacity(candidates.len());
    let mut used: usize = 0;
    for item in candidates {
        let cost = estimate_tokens(&item.text);
        if used.saturating_add(cost) <= budget {
            kept.push(item);
            used = used.saturating_add(cost);
        }
        // else: skip but keep scanning — a later, smaller item may still fit.
    }
    kept
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_uses_four_chars_per_token() {
        // 4 chars → 1 token, 5 chars → 2 tokens (round up).
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens(&"x".repeat(40)), 10);
    }

    #[test]
    fn estimate_tokens_counts_chars_not_bytes() {
        // Each `ç` is 2 bytes in UTF-8 but one char — the heuristic must use
        // char count so multi-byte PT-BR text is not over-estimated.
        let s = "ç".repeat(8); // 16 bytes, 8 chars → 2 tokens.
        assert_eq!(estimate_tokens(&s), 2);
    }

    #[test]
    fn prune_to_budget_keeps_prefix_within_cap() {
        let items: Vec<PrioritizedItem> = (0..5)
            .map(|n| PrioritizedItem::new("x".repeat(40), 100 - n))
            .collect();
        // Each item ~10 tokens; budget 25 → first 2 fit (20), third would push to 30.
        let kept = prune_to_budget(&items, 25);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn prune_to_budget_zero_budget_returns_empty() {
        let items = vec![PrioritizedItem::new("anything", 1)];
        let kept = prune_to_budget(&items, 0);
        assert!(kept.is_empty());
    }

    #[test]
    fn prune_to_budget_skips_oversize_then_picks_smaller_tail() {
        let items = vec![
            // First item huge (way over budget).
            PrioritizedItem::new("x".repeat(400), 100),
            // Smaller tail items that fit.
            PrioritizedItem::new("y".repeat(20), 50),
            PrioritizedItem::new("z".repeat(20), 40),
        ];
        // Budget = 15 tokens → first item (~100t) skipped; two tail items
        // (5t + 5t = 10t) fit comfortably.
        let kept = prune_to_budget(&items, 15);
        assert_eq!(kept.len(), 2);
        assert!(kept[0].text.starts_with('y'));
        assert!(kept[1].text.starts_with('z'));
    }

    #[test]
    fn prune_to_budget_empty_input() {
        let kept = prune_to_budget(&[], 1_000);
        assert!(kept.is_empty());
    }

    #[test]
    fn prune_to_budget_borrows_not_clones() {
        // The returned slice points at the original items — verifying via
        // ptr equality that no allocation of the text strings happened.
        let items = vec![PrioritizedItem::new("abcd", 1)];
        let kept = prune_to_budget(&items, 1);
        assert_eq!(kept.len(), 1);
        assert!(std::ptr::eq(kept[0], &items[0]));
    }
}
