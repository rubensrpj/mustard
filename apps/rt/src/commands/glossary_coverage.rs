//! `glossary-coverage` — deterministic check of how well a `CONTEXT.md` domain
//! glossary covers the repo-vocabulary terms a feature request touches.
//!
//! This is the Rust half of the `/feature` ANALYZE "grill nudge" (Selo 1): the
//! orchestrator runs it once, cheaply, before DECOMPOSE, and surfaces a single
//! dismissible suggestion to author/extend `CONTEXT.md` (via the `grill-with-docs`
//! skill) only when the glossary is `missing` or `weak`. It NEVER grills inline
//! and NEVER blocks.
//!
//! N (the denominator) is the digest's **matched terms** — the repo-vocabulary
//! terms the intent actually maps to — NOT the raw intent tokens (`domain_terms`
//! keeps stopwords like "the"). K is how many of those matched terms have a
//! covering block in `CONTEXT.md`, scored with the EXACT term matcher
//! `context-slice` uses (`parse_term_blocks` + `block_matches`), so the producer
//! and consumer of the glossary cannot drift.
//!
//! Output (stdout, byte-stable pretty JSON): `{ coveragePct, contextFile, present,
//! termsCovered, termsTotal, uncovered, verdict }`. The `uncovered` list IS the
//! actionable payload — the weak/missing domain terms the orchestrator hands to
//! the inline grill (`grill-capture`) so each confirmed definition lands in the
//! glossary; `contextFile` names the resolved target to write them into (the
//! first existing CONTEXT.md, or the first requested path when none resolved yet,
//! so a still-empty glossary still has a destination). Fail-open: a missing model
//! / unreadable glossary degrades to `verdict: "na"` (the SKILL then stays
//! silent); exit 0.

use std::collections::BTreeSet;
use std::path::Path;

use mustard_core::Scan;
use serde_json::json;

use crate::commands::economy::context_slice::{
    block_matches, parse_term_blocks, resolve_context_files, TermBlock,
};
use crate::commands::feature::domain_terms;

/// Below this covered-term percentage the glossary is `weak`.
const WEAK_COVERAGE_PCT: u64 = 50;
/// At or above this many uncovered matched terms the glossary is `weak`
/// regardless of percentage (an absolute floor catches wide features whose ratio
/// still looks healthy).
const WEAK_UNCOVERED_FLOOR: usize = 3;

/// Pure coverage scoring over already-resolved inputs — the unit-testable core.
struct Coverage {
    present: bool,
    total: usize,
    covered: usize,
    uncovered: Vec<String>,
    verdict: &'static str,
    /// The glossary file the orchestrator's inline grill should write confirmed
    /// terms into — the resolved CONTEXT.md, or the first requested path when
    /// none exists yet (so a `missing` verdict still has a destination). Empty
    /// when no `--context` was given.
    context_file: String,
}

impl Coverage {
    fn pct(&self) -> u64 {
        if self.total == 0 {
            100
        } else {
            (self.covered as u64 * 100) / self.total as u64
        }
    }
}

/// Score `matched` repo-vocabulary terms against the parsed glossary `blocks`.
/// `present` is whether any glossary file resolved at all (distinguishes the
/// "no `CONTEXT.md` authored" case from "authored but thin").
fn score(matched: &[String], blocks: &[TermBlock], present: bool) -> Coverage {
    let mut uncovered: Vec<String> = Vec::new();
    let mut covered = 0usize;
    for term in matched {
        let needle: BTreeSet<String> = std::iter::once(term.to_lowercase()).collect();
        if present && blocks.iter().any(|b| block_matches(b, &needle)) {
            covered += 1;
        } else {
            uncovered.push(term.clone());
        }
    }
    let total = matched.len();
    let mut c = Coverage {
        present,
        total,
        covered,
        uncovered,
        verdict: "ok",
        context_file: String::new(),
    };
    c.verdict = if total == 0 {
        // No domain terms touched → nothing a glossary could cover; never nudge.
        "ok"
    } else if !present {
        "missing"
    } else if c.pct() < WEAK_COVERAGE_PCT || c.uncovered.len() >= WEAK_UNCOVERED_FLOOR {
        "weak"
    } else {
        "ok"
    };
    c
}

/// Resolve the digest's matched terms + glossary blocks, then score. Fail-open:
/// returns `None` only when the scan digest is unavailable (the caller maps that
/// to `verdict: "na"`).
fn compute(intent: &str, context: &[String], root: &Path) -> Option<Coverage> {
    let terms = domain_terms(intent);
    let model = root.join(".claude").join("grain.model.json");

    // N = the repo-vocabulary terms the intent maps to (matched against the
    // grain model) — NOT the raw intent tokens, which keep stopwords.
    let matched: Vec<String> = match Scan::locate().digest_query(&model, &terms) {
        Ok(q) => q.matched_terms.iter().map(|t| t.term.clone()).collect(),
        Err(_) => return None,
    };

    // Parse the glossary through the SAME resolver `context-slice` uses
    // (CONTEXT-MAP.md expansion + silent skip of missing files).
    let resolved = resolve_context_files(context);
    let blocks: Vec<TermBlock> = resolved
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .flat_map(|text| parse_term_blocks(&text))
        .collect();
    let present = !blocks.is_empty();

    let mut coverage = score(&matched, &blocks, present);
    coverage.context_file = target_context_file(&resolved, context);
    Some(coverage)
}

/// The glossary path the orchestrator's inline grill writes confirmed terms
/// into. Prefer the first file that resolved on disk (an authored CONTEXT.md);
/// when none resolved (the `missing` case, or only a CONTEXT-MAP pointing at
/// absent files), fall back to the first non-empty requested `--context` path so
/// a still-empty glossary still names a concrete destination. Empty when no
/// `--context` was given at all.
fn target_context_file(resolved: &[std::path::PathBuf], requested: &[String]) -> String {
    if let Some(p) = resolved.first() {
        return p.display().to_string();
    }
    requested
        .iter()
        .find(|p| !p.is_empty())
        .cloned()
        .unwrap_or_default()
}

/// Render the coverage verdict as byte-stable JSON (deterministic key order).
fn to_json(c: &Coverage) -> serde_json::Value {
    json!({
        "verdict": c.verdict,
        "present": c.present,
        "termsTotal": c.total,
        "termsCovered": c.covered,
        "coveragePct": c.pct(),
        "uncovered": c.uncovered,
        "contextFile": c.context_file,
    })
}

/// Dispatch `mustard-rt run glossary-coverage`. Always exits 0.
pub fn run(intent: &str, context: &[String], root: &Path) {
    let payload = match compute(intent, context, root) {
        Some(c) => to_json(&c),
        // Digest unavailable → not-applicable; the SKILL nudge stays silent and
        // /feature continues unaffected.
        None => json!({
            "verdict": "na",
            "present": false,
            "termsTotal": 0,
            "termsCovered": 0,
            "coveragePct": 0,
            "uncovered": [],
            "contextFile": "",
        }),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into())
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_glossary_yields_missing_verdict() {
        let matched = vec!["payable".to_string(), "tenant".to_string()];
        let c = score(&matched, &[], false);
        assert_eq!(c.verdict, "missing");
        assert_eq!(c.covered, 0);
        assert_eq!(c.uncovered.len(), 2);
        assert_eq!(c.pct(), 0);
    }

    #[test]
    fn full_coverage_is_ok() {
        let blocks = parse_term_blocks("## Payable\nA bill owed.\n## Tenant\nAn org.");
        let matched = vec!["payable".to_string(), "tenant".to_string()];
        let c = score(&matched, &blocks, true);
        assert_eq!(c.verdict, "ok");
        assert_eq!(c.covered, 2);
        assert!(c.uncovered.is_empty());
        assert_eq!(c.pct(), 100);
    }

    #[test]
    fn thin_glossary_below_floor_is_weak() {
        // 1 of 4 covered → 25% < 50% AND 3 uncovered >= floor.
        let blocks = parse_term_blocks("## Payable\nA bill owed.");
        let matched = vec![
            "payable".to_string(),
            "tenant".to_string(),
            "ledger".to_string(),
            "invoice".to_string(),
        ];
        let c = score(&matched, &blocks, true);
        assert_eq!(c.verdict, "weak");
        assert_eq!(c.covered, 1);
        assert_eq!(c.uncovered, vec!["tenant", "ledger", "invoice"]);
    }

    #[test]
    fn uncovered_floor_trips_weak_even_above_percentage() {
        // 5 of 8 covered = 62% (>= 50%), but 3 uncovered hits the absolute floor.
        let blocks = parse_term_blocks(
            "## A\nx\n## B\nx\n## C\nx\n## D\nx\n## E\nx",
        );
        let matched = vec![
            "a".to_string(), "b".to_string(), "c".to_string(), "d".to_string(),
            "e".to_string(), "f".to_string(), "g".to_string(), "h".to_string(),
        ];
        let c = score(&matched, &blocks, true);
        assert_eq!(c.pct(), 62);
        assert_eq!(c.verdict, "weak");
    }

    #[test]
    fn no_domain_terms_is_ok_not_missing() {
        let c = score(&[], &[], false);
        assert_eq!(c.verdict, "ok");
        assert_eq!(c.pct(), 100);
    }

    #[test]
    fn target_context_file_prefers_resolved_then_falls_back_to_requested() {
        use std::path::PathBuf;
        // A resolved (on-disk) file wins.
        let resolved = vec![PathBuf::from("/repo/CONTEXT.md")];
        let requested = vec!["./CONTEXT.md".to_string()];
        assert_eq!(
            target_context_file(&resolved, &requested),
            "/repo/CONTEXT.md"
        );
        // Nothing resolved (the `missing` case) → first non-empty requested path.
        let requested = vec![String::new(), "docs/CONTEXT.md".to_string()];
        assert_eq!(
            target_context_file(&[], &requested),
            "docs/CONTEXT.md"
        );
        // No --context at all → empty (no destination to offer).
        assert!(target_context_file(&[], &[]).is_empty());
    }
}
