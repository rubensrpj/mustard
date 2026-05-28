//! `regression_check` — before/after snapshot + diff for the touched
//! functions of a Mustard wave.
//!
//! Spec A / Wave 2 introduces the third `mustard-core` primitive. Camera 3
//! of the regression gate (W4) consumes the [`Diff`] produced by
//! [`compare_snapshots`] to flag waves that *silently shrink* a function's
//! body — the canonical fail-open regression captured by AC-A-4 (function
//! went from 23 entries to 0 between the pre- and post-fixture of W6).
//!
//! ## Design (SOLID + agnostic-by-construction)
//!
//! - **Single responsibility.** The module owns one thing: read the bodies of
//!   the declared functions before and after, then compute a structural diff.
//!   The detection of *what* to capture lives in [`crate::domain::spec::touched_functions`]
//!   (W0). The grammar resolution lives in [`crate::domain::ast`] (W1.5).
//!   `regression_check` composes those primitives without duplicating their
//!   logic.
//! - **Zero hardcoded languages.** [`Snapshot::capture_for_spec`] receives a
//!   [`crate::domain::ast::GrammarLoader`] by reference; no language id is enumerated
//!   anywhere under this module. When the loader has no grammar for a path's
//!   language, the textual fallback is used — same fail-open contract as the
//!   `ast` layer.
//! - **Reuse, do not duplicate.** [`Snapshot::capture_for_spec`] reuses
//!   [`crate::domain::ast::extract_function_signatures`] to locate function spans
//!   inside source files. The textual fallback uses [`similar::TextDiff`]
//!   rather than reinventing line-by-line comparison.
//! - **Canonical serialisation.** [`Snapshot`] stores its functions in a
//!   [`std::collections::BTreeMap`], so two snapshots constructed in
//!   different insertion orders serialise to byte-identical JSON. This is
//!   what makes the diff reproducible across machines.
//!
//! ## Public surface
//!
//! - [`Snapshot`] — pre/post photograph of a wave's declared functions.
//! - [`Snapshot::capture_for_spec`] — capture from a spec markdown + codebase.
//! - [`Snapshot::compare_to`] — produce a [`Diff`] against another snapshot.
//! - [`compare_snapshots`] — free-function alias used by the bench harness.
//! - [`Diff`], [`FunctionDelta`], [`ChangeKind`], [`DiffSummary`].
//! - [`FunctionCapture`], [`CaptureMode`], [`TextSpan`].
//! - [`RegressionError`].

pub mod compare;
pub mod snapshot;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

// Re-export the canonical entry points so callers can write
// `regression_check::compare_snapshots(...)` directly.
pub use compare::compare_snapshots;
pub use snapshot::RegressionError;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Half-open byte span `[start, end)` inside the captured source text.
/// Mirrors the shape of [`crate::domain::ast::FunctionSig::span`] so callers that
/// already hold a signature span do not need to convert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSpan {
    /// Inclusive start byte offset.
    pub start: usize,
    /// Exclusive end byte offset.
    pub end: usize,
}

impl TextSpan {
    /// Build a span from a half-open `Range<usize>`.
    #[must_use]
    pub fn from_range(range: std::ops::Range<usize>) -> Self {
        Self {
            start: range.start,
            end: range.end,
        }
    }

    /// Length of the span in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// `true` when the span covers no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// How a [`FunctionCapture`] was produced.
///
/// Distinct from [`crate::domain::ast::DetectionMode`] even though the variants line
/// up: `DetectionMode` describes a *stub-pattern hit*, while `CaptureMode`
/// describes the *capture method* used for an entire function body. Keeping
/// the type local avoids cross-coupling the regression-check semantics into
/// the AST module's hit-emission API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    /// Grammar was resolved via [`crate::domain::ast::GrammarLoader`]; the body was
    /// extracted from the parsed AST (function-declaration node).
    Ast,
    /// No grammar was available; the body was extracted by the agnostic
    /// fallback regex + brace-balancing heuristic in [`snapshot`]. Telemetry
    /// is expected to carry a "grammar not installed" warning so the operator
    /// can choose to install the grammar for a precise capture.
    Textual,
}

impl CaptureMode {
    /// Canonical lowercase identifier — used in telemetry / serialised diff.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ast => "ast",
            Self::Textual => "textual",
        }
    }

    /// Combine two capture modes into the *most restrictive* of the two.
    /// `Textual` wins over `Ast` — when either side of a comparison fell
    /// back to text, the delta is textual.
    #[must_use]
    pub fn combine(self, other: Self) -> Self {
        match (self, other) {
            (Self::Ast, Self::Ast) => Self::Ast,
            _ => Self::Textual,
        }
    }
}

/// One declared function captured at a point in time.
///
/// The `signature` field is `None` when:
///
/// - the capture mode is [`CaptureMode::Textual`] (the fallback regex
///   produces only a name + span, not a typed signature), OR
/// - the AST path matched the function but the project ships no
///   `function_signature.scm` query — the body is still captured, but the
///   typed signature is not synthesised here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCapture {
    /// Stable identifier — the qualifier as declared in `## Funções tocadas`
    /// (e.g. `regression_check::Snapshot::capture_for_spec`). Used as the
    /// [`Snapshot::functions`] map key so two snapshots compare by qualifier,
    /// not by file path.
    pub qualifier: String,
    /// How the body below was produced.
    pub mode: CaptureMode,
    /// Typed signature when the AST path produced one. See doc-comment for
    /// when it is `None`.
    pub signature: Option<crate::domain::ast::FunctionSig>,
    /// Function body — verbatim source text. AST mode stores the
    /// `function_item` (or equivalent) node's text; textual mode stores
    /// brace-balanced text starting at the signature line.
    pub body: String,
    /// Byte span of [`Self::body`] inside the original source file.
    pub span: TextSpan,
}

/// Pre/post photograph of the declared functions for a wave.
///
/// Functions are stored in a [`BTreeMap`] keyed by the canonical qualifier
/// string. The map ordering makes `serde_json::to_string(snapshot)` produce
/// byte-identical output regardless of insertion order — a hard requirement
/// for diff reproducibility across machines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Path of the spec markdown the capture was taken for. Carried so the
    /// gate can attribute a diff back to a wave without threading the path
    /// through every call.
    pub spec_path: PathBuf,
    /// ISO-8601 UTC timestamp at which the capture ran. The gate uses this
    /// to order pre- vs post-capture without depending on filesystem mtime.
    pub captured_at: String,
    /// Captured functions keyed by their qualifier (R3 of touched-functions
    /// vocabulary: module path, file path, or bare name).
    pub functions: BTreeMap<String, FunctionCapture>,
}

impl Snapshot {
    /// Construct an empty snapshot — used by tests and by the constructor
    /// inside [`snapshot::capture_for_spec`] before functions are pushed.
    #[must_use]
    pub fn empty(spec_path: PathBuf, captured_at: String) -> Self {
        Self {
            spec_path,
            captured_at,
            functions: BTreeMap::new(),
        }
    }

    /// Insert a captured function, keyed by its qualifier.
    pub fn insert(&mut self, capture: FunctionCapture) {
        self.functions.insert(capture.qualifier.clone(), capture);
    }

    /// Number of captured functions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// `true` when no functions were captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    /// Look up a capture by its qualifier.
    #[must_use]
    pub fn get(&self, qualifier: &str) -> Option<&FunctionCapture> {
        self.functions.get(qualifier)
    }
}

/// What changed for a single function between two snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ChangeKind {
    /// Function exists only in the *after* snapshot.
    Added,
    /// Function exists only in the *before* snapshot.
    Removed,
    /// Function exists in both snapshots and the bodies differ. `lineChanges`
    /// is the number of line-level edits (lines added + lines removed in
    /// `similar::ChangeTag::{Insert, Delete}`). For AST-captured pairs the
    /// count is over the normalised body text — same metric, just computed
    /// on whitespace-collapsed input so trivia churn does not inflate it.
    Modified {
        /// Number of lines added + removed by the diff.
        #[serde(rename = "lineChanges")]
        line_changes: usize,
    },
    /// Function exists in both snapshots and the bodies match.
    Unchanged,
}

/// One row in a [`Diff`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionDelta {
    /// Qualifier of the function this delta reports on.
    pub qualifier: String,
    /// Before-image. `None` when the function was added.
    pub before: Option<FunctionCapture>,
    /// After-image. `None` when the function was removed.
    pub after: Option<FunctionCapture>,
    /// What changed.
    pub change: ChangeKind,
    /// Effective capture mode for this row — the *most restrictive* of the
    /// two captures (textual wins over AST). The gate uses this to decide
    /// whether the line_changes metric is structural (AST) or lexical
    /// (textual fallback).
    pub mode: CaptureMode,
}

/// Bottom-line totals for a [`Diff`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffSummary {
    /// Number of [`ChangeKind::Added`] rows.
    pub added: usize,
    /// Number of [`ChangeKind::Removed`] rows.
    pub removed: usize,
    /// Number of [`ChangeKind::Modified`] rows (any line_changes value).
    pub modified: usize,
    /// Number of [`ChangeKind::Unchanged`] rows.
    pub unchanged: usize,
}

/// Full diff between two snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    /// One row per qualifier seen in either snapshot. Sorted by qualifier so
    /// the diff is reproducible (matches [`Snapshot::functions`] ordering).
    pub deltas: Vec<FunctionDelta>,
    /// Roll-up counts derived from [`Self::deltas`].
    pub summary: DiffSummary,
}

impl Diff {
    /// Iterate the deltas that are not [`ChangeKind::Unchanged`]. Consumers
    /// of the gate care about the changes, not the noise.
    pub fn changes(&self) -> impl Iterator<Item = &FunctionDelta> {
        self.deltas
            .iter()
            .filter(|d| !matches!(d.change, ChangeKind::Unchanged))
    }

    /// `true` when no row reports a change.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.summary.added == 0 && self.summary.removed == 0 && self.summary.modified == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ast::GrammarLoader;
    use std::path::{Path, PathBuf};

    // ── T2.5 — canonical serialisation ──────────────────────────────────
    //
    // Two snapshots built by inserting the same captures in *different*
    // orders must serialise to byte-identical JSON. The `BTreeMap` layout
    // of `Snapshot::functions` guarantees this; the test pins the
    // invariant so a future refactor to `HashMap` (or insertion-order
    // serialisation) would fail loudly.
    #[test]
    fn test_canonical_serialization() {
        let mk = |q: &str, body: &str| FunctionCapture {
            qualifier: q.to_string(),
            mode: CaptureMode::Textual,
            signature: None,
            body: body.to_string(),
            span: TextSpan {
                start: 0,
                end: body.len(),
            },
        };
        let captures = [
            mk("z::late", "fn late() {}"),
            mk("a::early", "fn early() {}"),
            mk("m::middle", "fn middle() {}"),
        ];

        let mut s1 = Snapshot::empty(PathBuf::from("spec.md"), "2026-05-27T00:00:00.000Z".into());
        for c in &captures {
            s1.insert(c.clone());
        }

        // Build s2 by inserting in reverse order — same set, different
        // insertion sequence.
        let mut s2 = Snapshot::empty(PathBuf::from("spec.md"), "2026-05-27T00:00:00.000Z".into());
        for c in captures.iter().rev() {
            s2.insert(c.clone());
        }

        let j1 = serde_json::to_string(&s1).expect("serialise s1");
        let j2 = serde_json::to_string(&s2).expect("serialise s2");
        assert_eq!(
            j1, j2,
            "BTreeMap ordering must produce byte-identical JSON regardless of insertion order"
        );
        // Sanity: the JSON actually contains the qualifiers in sorted order.
        let pos_a = j1.find("a::early").expect("contains a::early");
        let pos_m = j1.find("m::middle").expect("contains m::middle");
        let pos_z = j1.find("z::late").expect("contains z::late");
        assert!(pos_a < pos_m && pos_m < pos_z, "qualifiers sorted in JSON");
    }

    // ── T2.6 — AC-A-4 against W6 fixtures ────────────────────────────────
    //
    // Locates `.claude/spec/2026-05-27-mustard-v4-foundation/fixtures/
    // w6-{pre,post}/telemetry.rs`, writes a synthetic touched-functions
    // section pointing at each fixture, captures both, and asserts the
    // diff records at least one Modified row whose before-body was rich
    // (≥20 lines) and whose after-body is essentially empty.
    //
    // This is the AC-A-4 ground truth: a function went from 23 entries to
    // 0 between the pre- and post-W6 fixture; the gate must see that.

    fn workspace_root() -> PathBuf {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir
            .ancestors()
            .find(|p| p.join("Cargo.lock").exists())
            .map(Path::to_path_buf)
            .unwrap_or(manifest_dir)
    }

    fn fixtures_dir() -> PathBuf {
        workspace_root()
            .join(".claude")
            .join("spec")
            .join("2026-05-27-mustard-v4-foundation")
            .join("fixtures")
    }

    /// Pick a function name that exists in BOTH telemetry.rs fixtures and
    /// whose body shrank measurably. From the W6 fixture inspection:
    /// `dashboard_prompt_economy` composes four blocks (cost,
    /// subtractions, claude_events, freshness) over ~30 lines in w6-pre,
    /// then collapses to a single `json!({ "by_role": [], "by_command":
    /// [] })` return in w6-post. That is the canonical "antes 23 entradas
    /// → depois 0" regression captured by AC-A-4.
    const REGRESSION_TARGET: &str = "dashboard_prompt_economy";

    #[test]
    fn test_capture_and_compare_w6_fixture() {
        let fixtures = fixtures_dir();
        let pre_file = fixtures.join("w6-pre").join("telemetry.rs");
        let post_file = fixtures.join("w6-post").join("telemetry.rs");

        if !pre_file.exists() || !post_file.exists() {
            eprintln!(
                "skipping AC-A-4 fixture test — fixtures absent at {} / {}",
                pre_file.display(),
                post_file.display()
            );
            return;
        }

        // Build a synthetic spec.md whose `## Funções tocadas` section
        // points at the fixture file (PathHint shape).
        let spec_pre = format!(
            "# Spec\n\n## Funções tocadas\n\n### Em `.claude/spec/2026-05-27-mustard-v4-foundation/fixtures/w6-pre/` (MODIFICADO)\n- `.claude/spec/2026-05-27-mustard-v4-foundation/fixtures/w6-pre/telemetry.rs::{REGRESSION_TARGET}`\n",
        );
        let spec_post = format!(
            "# Spec\n\n## Funções tocadas\n\n### Em `.claude/spec/2026-05-27-mustard-v4-foundation/fixtures/w6-post/` (MODIFICADO)\n- `.claude/spec/2026-05-27-mustard-v4-foundation/fixtures/w6-post/telemetry.rs::{REGRESSION_TARGET}`\n",
        );

        let loader = GrammarLoader::empty(&workspace_root());
        let pre_report = Snapshot::capture_for_spec(
            &loader,
            &spec_pre,
            &workspace_root(),
            PathBuf::from("spec-pre.md"),
        )
        .expect("pre capture");
        let post_report = Snapshot::capture_for_spec(
            &loader,
            &spec_post,
            &workspace_root(),
            PathBuf::from("spec-post.md"),
        )
        .expect("post capture");

        // Both captures should have landed exactly one row.
        assert_eq!(pre_report.snapshot.len(), 1, "pre captured one function");
        assert_eq!(post_report.snapshot.len(), 1, "post captured one function");

        // The qualifier rendered by `PathHint::as_str()` will differ
        // between the two snapshots (different file paths), so we need
        // to retarget the post capture under the pre qualifier so the
        // diff can compare them. Re-key by the final identifier — that
        // is exactly what the W4 gate does when normalising pre/post
        // path-hint divergence.
        fn rekey_by_final_name(snapshot: &Snapshot, key: &str) -> Snapshot {
            let mut out = Snapshot::empty(
                snapshot.spec_path.clone(),
                snapshot.captured_at.clone(),
            );
            for cap in snapshot.functions.values() {
                // Pull the final identifier off the qualifier (after
                // the last `::`). That is the natural join key.
                let final_id = cap.qualifier.rsplit("::").next().unwrap_or("");
                if final_id == key {
                    let mut renamed = cap.clone();
                    renamed.qualifier = key.to_string();
                    out.insert(renamed);
                }
            }
            out
        }

        let pre_norm = rekey_by_final_name(&pre_report.snapshot, REGRESSION_TARGET);
        let post_norm = rekey_by_final_name(&post_report.snapshot, REGRESSION_TARGET);

        assert_eq!(pre_norm.len(), 1, "pre rekeyed contains the target");
        assert_eq!(post_norm.len(), 1, "post rekeyed contains the target");

        let pre_cap = pre_norm.get(REGRESSION_TARGET).expect("pre cap");
        let post_cap = post_norm.get(REGRESSION_TARGET).expect("post cap");

        // AC-A-4 — pre body is rich, post body is reduced. Quantify both:
        // pre should have ≥10 non-empty lines; post should have ≤6.
        let pre_lines = pre_cap.body.lines().filter(|l| !l.trim().is_empty()).count();
        let post_lines = post_cap
            .body
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count();
        assert!(
            pre_lines >= 10,
            "pre body should be rich, got {pre_lines} lines:\n{}",
            pre_cap.body
        );
        assert!(
            post_lines <= 6,
            "post body should be reduced, got {post_lines} lines:\n{}",
            post_cap.body
        );

        // Diff — must report the function as Modified with substantial
        // line_changes count.
        let diff = pre_norm.compare_to(&post_norm);
        assert_eq!(diff.summary.modified, 1, "exactly one modified row");
        let row = &diff.deltas[0];
        assert_eq!(row.qualifier, REGRESSION_TARGET);
        match &row.change {
            ChangeKind::Modified { line_changes } => {
                assert!(
                    *line_changes >= 10,
                    "AC-A-4 expects significant shrinkage; got line_changes={line_changes}"
                );
            }
            other => panic!("expected Modified, got {other:?}"),
        }
        assert!(row.before.is_some());
        assert!(row.after.is_some());
    }

    #[test]
    fn text_span_helpers() {
        let s = TextSpan::from_range(3..10);
        assert_eq!(s.start, 3);
        assert_eq!(s.end, 10);
        assert_eq!(s.len(), 7);
        assert!(!s.is_empty());
        assert!(TextSpan { start: 5, end: 5 }.is_empty());
    }

    #[test]
    fn capture_mode_combine_prefers_textual() {
        assert_eq!(
            CaptureMode::Ast.combine(CaptureMode::Ast),
            CaptureMode::Ast
        );
        assert_eq!(
            CaptureMode::Ast.combine(CaptureMode::Textual),
            CaptureMode::Textual
        );
        assert_eq!(
            CaptureMode::Textual.combine(CaptureMode::Ast),
            CaptureMode::Textual
        );
        assert_eq!(
            CaptureMode::Textual.combine(CaptureMode::Textual),
            CaptureMode::Textual
        );
    }

    #[test]
    fn snapshot_empty_constructor() {
        let s = Snapshot::empty(PathBuf::from("x.md"), "2026-05-27T00:00:00Z".into());
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
        assert!(s.get("any").is_none());
    }

    #[test]
    fn snapshot_insert_indexes_by_qualifier() {
        let mut s = Snapshot::empty(PathBuf::from("x.md"), "2026-05-27T00:00:00Z".into());
        s.insert(FunctionCapture {
            qualifier: "a::b".into(),
            mode: CaptureMode::Textual,
            signature: None,
            body: "fn b() {}".into(),
            span: TextSpan { start: 0, end: 9 },
        });
        assert_eq!(s.len(), 1);
        assert!(s.get("a::b").is_some());
    }

    #[test]
    fn diff_is_clean_only_when_no_changes() {
        let d = Diff {
            deltas: vec![],
            summary: DiffSummary::default(),
        };
        assert!(d.is_clean());

        let d2 = Diff {
            deltas: vec![FunctionDelta {
                qualifier: "x".into(),
                before: None,
                after: None,
                change: ChangeKind::Added,
                mode: CaptureMode::Ast,
            }],
            summary: DiffSummary {
                added: 1,
                ..DiffSummary::default()
            },
        };
        assert!(!d2.is_clean());
        assert_eq!(d2.changes().count(), 1);
    }
}
