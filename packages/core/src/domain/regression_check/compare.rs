//! Snapshot comparison — turn two [`Snapshot`] photographs into a [`Diff`].
//!
//! The contract is simple: union the qualifiers from both snapshots, then
//! emit one [`FunctionDelta`] per qualifier with the appropriate
//! [`ChangeKind`]. Bodies are compared by:
//!
//! - **AST mode (both sides):** normalise whitespace inside the body
//!   (collapse runs of whitespace to a single space, trim per-line) and
//!   compare via [`similar::TextDiff::from_lines`]. Trivia churn does not
//!   inflate the line-count metric.
//! - **Textual mode (either side):** use [`similar::TextDiff::from_lines`]
//!   over the raw bodies. The mode is tagged textual so the gate knows the
//!   line-count metric is lexical, not structural.
//!
//! The output [`Diff`] is sorted by qualifier so two runs over the same
//! input produce byte-identical serialised JSON — see also the canonical
//! BTreeMap layout of [`Snapshot::functions`].
//!
//! ## AC-A-12 — performance
//!
//! [`compare_snapshots`] must complete in <50ms for two snapshots of 100
//! function captures with ~20-line bodies each. The bench at the bottom of
//! this file asserts that budget. The implementation stays linear in the
//! total body bytes: each qualifier maps to one `TextDiff::from_lines` call
//! that itself is `O(N+M)` for unique lines; the union pass is `O(n log n)`
//! dominated by the BTreeMap iteration (already ordered).

use super::{
    CaptureMode, ChangeKind, Diff, DiffSummary, FunctionCapture, FunctionDelta, Snapshot,
};
use similar::{ChangeTag, TextDiff};
use std::collections::BTreeSet;

impl Snapshot {
    /// Compare `self` (before) against `other` (after) and produce a
    /// [`Diff`].
    ///
    /// The diff is ordered by qualifier — same ordering as
    /// [`Snapshot::functions`] — so two invocations on the same inputs
    /// produce byte-identical serialised JSON.
    #[must_use]
    pub fn compare_to(&self, other: &Snapshot) -> Diff {
        compare_snapshots(self, other)
    }
}

/// Free-function wrapper used by callers that hold two snapshots by
/// reference without wanting to pick a side. Equivalent to
/// `before.compare_to(after)`.
#[must_use]
pub fn compare_snapshots(before: &Snapshot, after: &Snapshot) -> Diff {
    // Union of qualifier keys, sorted (BTreeSet preserves order).
    let mut qualifiers: BTreeSet<&str> = BTreeSet::new();
    for k in before.functions.keys() {
        qualifiers.insert(k.as_str());
    }
    for k in after.functions.keys() {
        qualifiers.insert(k.as_str());
    }

    let mut deltas: Vec<FunctionDelta> = Vec::with_capacity(qualifiers.len());
    let mut summary = DiffSummary::default();

    for q in qualifiers {
        let bef = before.functions.get(q);
        let aft = after.functions.get(q);
        let delta = build_delta(q, bef, aft);
        match delta.change {
            ChangeKind::Added => summary.added += 1,
            ChangeKind::Removed => summary.removed += 1,
            ChangeKind::Modified { .. } => summary.modified += 1,
            ChangeKind::Unchanged => summary.unchanged += 1,
        }
        deltas.push(delta);
    }

    Diff { deltas, summary }
}

/// Build a single delta row given the before/after captures.
fn build_delta(
    qualifier: &str,
    before: Option<&FunctionCapture>,
    after: Option<&FunctionCapture>,
) -> FunctionDelta {
    match (before, after) {
        (None, None) => {
            // Defensive: the union iteration cannot produce this; emit
            // Unchanged with no captures so the row is still serialisable.
            FunctionDelta {
                qualifier: qualifier.to_string(),
                before: None,
                after: None,
                change: ChangeKind::Unchanged,
                mode: CaptureMode::Ast,
            }
        }
        (Some(b), None) => FunctionDelta {
            qualifier: qualifier.to_string(),
            before: Some(b.clone()),
            after: None,
            change: ChangeKind::Removed,
            mode: b.mode,
        },
        (None, Some(a)) => FunctionDelta {
            qualifier: qualifier.to_string(),
            before: None,
            after: Some(a.clone()),
            change: ChangeKind::Added,
            mode: a.mode,
        },
        (Some(b), Some(a)) => {
            let mode = b.mode.combine(a.mode);
            let line_changes = count_line_changes(&b.body, &a.body, mode);
            let change = if line_changes == 0 {
                ChangeKind::Unchanged
            } else {
                ChangeKind::Modified { line_changes }
            };
            FunctionDelta {
                qualifier: qualifier.to_string(),
                before: Some(b.clone()),
                after: Some(a.clone()),
                change,
                mode,
            }
        }
    }
}

/// Count line-level edits between two bodies.
///
/// When both captures came from AST mode, the bodies are first normalised
/// (whitespace collapsed) so insignificant formatting churn does not show
/// up as a regression. Textual mode compares the raw bodies — the gate
/// already knows the metric is lexical in that case.
fn count_line_changes(before: &str, after: &str, mode: CaptureMode) -> usize {
    let (b_norm, a_norm) = match mode {
        CaptureMode::Ast => (normalise_for_ast(before), normalise_for_ast(after)),
        CaptureMode::Textual => (before.to_string(), after.to_string()),
    };
    if b_norm == a_norm {
        return 0;
    }
    let diff = TextDiff::from_lines(&b_norm, &a_norm);
    let mut count = 0usize;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert | ChangeTag::Delete => count += 1,
            ChangeTag::Equal => {}
        }
    }
    count
}

/// Whitespace-tolerant normalisation for AST-mode bodies. Per line: trim
/// outer whitespace, collapse runs of whitespace inside the line to a
/// single space. Preserves line boundaries so the line-count metric stays
/// meaningful.
fn normalise_for_ast(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut first = true;
    for raw in body.lines() {
        if !first {
            out.push('\n');
        }
        first = false;

        let trimmed = raw.trim();
        let mut prev_ws = false;
        for ch in trimmed.chars() {
            if ch.is_whitespace() {
                if !prev_ws {
                    out.push(' ');
                    prev_ws = true;
                }
            } else {
                out.push(ch);
                prev_ws = false;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::regression_check::TextSpan;
    use std::path::PathBuf;

    fn capture(qualifier: &str, body: &str, mode: CaptureMode) -> FunctionCapture {
        FunctionCapture {
            qualifier: qualifier.to_string(),
            mode,
            signature: None,
            body: body.to_string(),
            span: TextSpan {
                start: 0,
                end: body.len(),
            },
        }
    }

    fn empty_snapshot() -> Snapshot {
        Snapshot::empty(PathBuf::from("spec.md"), "2026-05-27T00:00:00.000Z".into())
    }

    #[test]
    fn empty_vs_empty_is_clean() {
        let a = empty_snapshot();
        let b = empty_snapshot();
        let d = a.compare_to(&b);
        assert!(d.is_clean());
        assert_eq!(d.summary, DiffSummary::default());
    }

    #[test]
    fn added_when_only_in_after() {
        let mut before = empty_snapshot();
        let mut after = empty_snapshot();
        after.insert(capture("a::new", "fn new() {}", CaptureMode::Textual));

        let d = before.compare_to(&after);
        assert_eq!(d.summary.added, 1);
        assert_eq!(d.summary.removed, 0);
        let row = &d.deltas[0];
        assert!(matches!(row.change, ChangeKind::Added));
        assert!(row.before.is_none());
        assert!(row.after.is_some());

        // Symmetry — remove existing.
        before.insert(capture("a::old", "fn old() { 1 }", CaptureMode::Textual));
        let _ = &mut before; // silence
        let d2 = before.compare_to(&empty_snapshot());
        assert_eq!(d2.summary.removed, 1);
    }

    #[test]
    fn modified_counts_line_changes_textual() {
        let mut before = empty_snapshot();
        let mut after = empty_snapshot();
        before.insert(capture(
            "a::f",
            "fn f() {\n    let x = 1;\n    let y = 2;\n}\n",
            CaptureMode::Textual,
        ));
        after.insert(capture(
            "a::f",
            "fn f() {\n}\n",
            CaptureMode::Textual,
        ));
        let d = before.compare_to(&after);
        assert_eq!(d.summary.modified, 1);
        match d.deltas[0].change {
            ChangeKind::Modified { line_changes } => assert!(line_changes >= 2),
            _ => panic!("expected Modified"),
        }
        assert_eq!(d.deltas[0].mode, CaptureMode::Textual);
    }

    #[test]
    fn ast_mode_ignores_whitespace_only_diffs() {
        let mut before = empty_snapshot();
        let mut after = empty_snapshot();
        before.insert(capture(
            "a::f",
            "fn f() {  let  x = 1; }",
            CaptureMode::Ast,
        ));
        after.insert(capture(
            "a::f",
            "fn f() { let x = 1; }",
            CaptureMode::Ast,
        ));
        let d = before.compare_to(&after);
        assert_eq!(d.summary.modified, 0);
        assert_eq!(d.summary.unchanged, 1);
    }

    #[test]
    fn mode_combine_textual_wins() {
        let mut before = empty_snapshot();
        let mut after = empty_snapshot();
        before.insert(capture("a::f", "before", CaptureMode::Ast));
        after.insert(capture("a::f", "after", CaptureMode::Textual));
        let d = before.compare_to(&after);
        assert_eq!(d.deltas[0].mode, CaptureMode::Textual);
    }

    #[test]
    fn diff_is_sorted_by_qualifier() {
        let mut before = empty_snapshot();
        let mut after = empty_snapshot();
        for k in ["c::z", "a::a", "b::m"] {
            before.insert(capture(k, "body", CaptureMode::Textual));
            after.insert(capture(k, "body", CaptureMode::Textual));
        }
        let d = before.compare_to(&after);
        let keys: Vec<&str> = d.deltas.iter().map(|r| r.qualifier.as_str()).collect();
        assert_eq!(keys, vec!["a::a", "b::m", "c::z"]);
    }

    // ── AC-A-12 bench — 100 functions in <50ms ────────────────────────────
    //
    // Runs the synthetic comparison and asserts wall-clock budget. Held to
    // 200ms in debug profile (cargo test default) and 50ms in release; the
    // gate runs in release in production. The doc-comment notes this so
    // future tightening lives next to the assertion.

    /// Synthetic [`FunctionCapture`] with a ~20-line body. Body is unique per
    /// index so the AC-typing line-count delta is non-trivial.
    fn synthetic_capture(idx: usize, variant: &str) -> FunctionCapture {
        let mut body = format!("pub fn synth_{idx}() -> i32 {{\n");
        for j in 0..18 {
            body.push_str(&format!("    let v_{j} = {idx} + {j} + {variant:?};\n"));
        }
        body.push_str("    0\n}\n");
        FunctionCapture {
            qualifier: format!("module::synth_{idx}"),
            mode: CaptureMode::Textual,
            signature: None,
            body: body.clone(),
            span: TextSpan {
                start: 0,
                end: body.len(),
            },
        }
    }

    fn build_synthetic_snapshot(variant: &str) -> Snapshot {
        let mut s = Snapshot::empty(
            PathBuf::from("spec.md"),
            "2026-05-27T00:00:00.000Z".into(),
        );
        for i in 0..100 {
            s.insert(synthetic_capture(i, variant));
        }
        s
    }

    /// AC-A-12 — `compare_snapshots` over 100 captures of ~20-line bodies
    /// must complete in <50ms in release profile. In debug profile we
    /// relax to 200ms so the bench survives `cargo test` without --release;
    /// the production gate runs in release.
    #[test]
    fn compare_100_functions() {
        let before = build_synthetic_snapshot("a");
        let after = build_synthetic_snapshot("b");

        // Warm-up — first call allocates similar's internal buffers.
        let _ = compare_snapshots(&before, &after);

        let start = std::time::Instant::now();
        let diff = compare_snapshots(&before, &after);
        let elapsed = start.elapsed();

        // Sanity: 100 modified rows.
        assert_eq!(diff.summary.modified, 100);

        // Budget: 50ms release / 200ms debug. The constant below is
        // selected by `cfg!(debug_assertions)`.
        let budget_ms = if cfg!(debug_assertions) { 200 } else { 50 };
        // Emit the measurement so the gate review / CI capture sees the
        // actual wall-clock. Visible under `cargo test -- --nocapture`.
        eprintln!(
            "AC-A-12 compare_100_functions: {}µs ({}ms budget, mode: {})",
            elapsed.as_micros(),
            budget_ms,
            if cfg!(debug_assertions) { "debug" } else { "release" },
        );
        assert!(
            elapsed.as_millis() <= budget_ms,
            "AC-A-12 budget exceeded: {}ms > {}ms (mode: {})",
            elapsed.as_millis(),
            budget_ms,
            if cfg!(debug_assertions) { "debug" } else { "release" },
        );
    }
}
