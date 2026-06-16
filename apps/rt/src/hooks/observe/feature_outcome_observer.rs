//! `feature_outcome_observer` — the SIGNAL half of the digest-outcome loop.
//!
//! ## What it measures
//!
//! After a `mustard-rt run feature` research round, did the orchestrator open
//! the files the digest pointed at? The digest already emits the SUGGESTED set
//! (`feature.query` carries `report.terms[].files` — the anchors). The missing
//! half is the OBSERVED set: which of those anchors the orchestrator actually
//! Read/Edit/Write. This observer is that half — a `PostToolUse(Read|Edit|Write)`
//! side effect that, while a research window is open, correlates each touched
//! file with the window's anchors and emits one `feature.outcome` event:
//!
//! ```json
//! { "file": "src/refund.cs", "wasAnchor": true, "terms": ["refund"] }
//! ```
//!
//! The Wave 2 projection (`mustard-rt run digest-precision`) folds
//! `feature.query` (suggested) × `feature.outcome` (observed) into a recall /
//! precision metric — the deterministic CRITERION OF STOP for the locator
//! redesign.
//!
//! ## Window binding (inline, not time-windowed)
//!
//! `feature::run` drops a small `active-research.json` marker in the session
//! (`.claude/.session/<id>/active-research.json`, overwritten per query). This
//! observer reads it on every Read/Edit/Write. Binding the outcome to the
//! marker (rather than correlating by a wall-clock window in the projection)
//! keeps the emitted events scoped to the research that produced them: no
//! marker on disk ⇒ no event. The marker carries the anchors + the per-anchor
//! terms, so the correlation is a pure set membership test here — the
//! projection never has to re-derive which file belonged to which query.
//!
//! ## Observer contract (apps/rt/CLAUDE.md)
//!
//! Telemetry only: [`Observer::observe`] returns `()`, never a verdict — a
//! Read/Edit/Write ALWAYS proceeds. Fail-open at every IO step: a missing /
//! unreadable / malformed marker, an unresolved session, a path-less tool
//! input all degrade to a silent no-op. It NEVER panics.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use mustard_core::domain::model::event::ActorKind;
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};
use std::path::Path;

use crate::shared::events::economy;

/// The event this observer emits per touched file inside a research window.
const EVENT_FEATURE_OUTCOME: &str = "feature.outcome";

/// The marker `feature::run` writes per research round.
pub(crate) const ACTIVE_RESEARCH_MARKER: &str = "active-research.json";

/// The digest-outcome observer.
pub struct FeatureOutcomeObserver;

/// `true` if this is a Read / Edit / Write tool invocation — the three tools
/// that "open" a file (the orchestrator's reads of the digest's anchors). The
/// SIGNAL counts opens, so a read counts exactly like an edit.
fn is_open_tool(input: &HookInput) -> bool {
    matches!(input.tool_name.as_deref(), Some("Read" | "Edit" | "Write"))
}

/// The `file_path` of a Read/Edit/Write invocation — `file_path` for all three,
/// with `path` as the legacy alias (mirrors `post_edit::file_path_of`).
fn file_path_of(input: &HookInput) -> Option<String> {
    let ti = &input.tool_input;
    ti.get("file_path")
        .or_else(|| ti.get("path"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Resolve the active-research marker path for `cwd`'s current session:
/// `.claude/.session/<id>/active-research.json`. `None` when the session is
/// unresolved (then there is nothing to correlate — fail-open no-op).
pub(crate) fn marker_path_for(cwd: &str) -> Option<std::path::PathBuf> {
    let session = crate::shared::context::session_id();
    if session.is_empty() || session == "unknown" {
        return None;
    }
    Some(
        ClaudePaths::for_project(Path::new(cwd))
            .ok()?
            .claude_dir()
            .join(".session")
            .join(session)
            .join(ACTIVE_RESEARCH_MARKER),
    )
}

/// One anchor row parsed from the marker: the file the digest suggested plus
/// the query terms that named it.
struct Anchor {
    file: String,
    terms: Vec<String>,
}

/// Parse the marker JSON into the anchor set. Tolerant: a missing / malformed
/// `anchors` array yields an empty set (the caller then no-ops). The shape is
/// `{ "anchors": [ { "file": "...", "terms": ["..."] }, ... ] }`.
fn parse_anchors(raw: &str) -> Vec<Anchor> {
    let Ok(v) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let Some(rows) = v.get("anchors").and_then(Value::as_array) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let file = row.get("file").and_then(Value::as_str)?.to_string();
            let terms = row
                .get("terms")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            (!file.is_empty()).then_some(Anchor { file, terms })
        })
        .collect()
}

/// Match a touched file against an anchor path. Both are normalised to
/// forward-slash lowercase; an anchor matches when the touched path equals it,
/// ends with it (anchors are repo-relative, touches may be absolute), contains
/// it as a `/`-segment suffix, or shares the basename. Mirrors the path-anchor
/// strategy `post_edit::meta_item_matches_edit` uses for checklist items, so
/// the two correlators agree on "this edit is that declared file".
fn touch_matches_anchor(norm_touched: &str, touched_base: &str, anchor: &str) -> bool {
    let a = anchor.replace('\\', "/").to_ascii_lowercase();
    if a.is_empty() {
        return false;
    }
    norm_touched == a
        || norm_touched.ends_with(&a)
        || norm_touched.ends_with(&format!("/{a}"))
        || basename(&a) == touched_base
}

/// The basename (last `/`-separated segment) of an already-normalised path.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Build the `feature.outcome` payload for a touched file against the anchor
/// set: `{file, wasAnchor, terms}`. `terms` is the sorted union of every
/// matching anchor's terms (empty when `wasAnchor` is false). Pure — unit
/// testable without IO. The touched path is emitted normalised (forward-slash)
/// so the projection key is OS-stable.
fn outcome_payload(touched: &str, anchors: &[Anchor]) -> Value {
    let norm = touched.replace('\\', "/");
    let norm_lower = norm.to_ascii_lowercase();
    let touched_base = basename(&norm_lower).to_string();
    let mut terms: Vec<String> = Vec::new();
    let mut was_anchor = false;
    for anchor in anchors {
        if touch_matches_anchor(&norm_lower, &touched_base, &anchor.file) {
            was_anchor = true;
            for t in &anchor.terms {
                if !terms.contains(t) {
                    terms.push(t.clone());
                }
            }
        }
    }
    terms.sort();
    json!({ "file": norm, "wasAnchor": was_anchor, "terms": terms })
}

/// Correlate a single touched file against the marker on disk and emit one
/// `feature.outcome` event. Returns the emitted payload (for tests); `None`
/// when nothing was emitted (no marker / no session / no anchors). Fail-open:
/// every IO step degrades to `None`.
fn correlate_and_emit(cwd: &str, touched: &str) -> Option<Value> {
    let marker = marker_path_for(cwd)?;
    let raw = fs::read_to_string(&marker).ok()?;
    let anchors = parse_anchors(&raw);
    if anchors.is_empty() {
        return None;
    }
    let payload = outcome_payload(touched, &anchors);
    economy::emit(cwd, ActorKind::Hook, "feature-outcome", EVENT_FEATURE_OUTCOME, None, payload.clone());
    Some(payload)
}

impl Observer for FeatureOutcomeObserver {
    /// Emit `feature.outcome` for the touched file when a research window is
    /// open. Pure side effect — never a verdict, fail-open throughout.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return;
        }
        if !is_open_tool(input) {
            return;
        }
        let Some(touched) = file_path_of(input) else {
            return;
        };
        let cwd = ctx.project_dir_or_cwd(input);
        let _ = correlate_and_emit(&cwd, &touched);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn anchors(rows: &[(&str, &[&str])]) -> Vec<Anchor> {
        rows.iter()
            .map(|(file, terms)| Anchor {
                file: (*file).to_string(),
                terms: terms.iter().map(|s| (*s).to_string()).collect(),
            })
            .collect()
    }

    #[test]
    fn parse_anchors_reads_file_and_terms_tolerantly() {
        let raw = r#"{ "terms": ["refund"], "anchors": [
            { "file": "src/refund.cs", "terms": ["refund", "order"] },
            { "file": "src/tail.cs" },
            { "terms": ["orphan"] },
            { "file": "" }
        ] }"#;
        let got = parse_anchors(raw);
        assert_eq!(got.len(), 2, "blank/file-less rows dropped");
        assert_eq!(got[0].file, "src/refund.cs");
        assert_eq!(got[0].terms, vec!["refund", "order"]);
        assert_eq!(got[1].file, "src/tail.cs");
        assert!(got[1].terms.is_empty(), "missing terms degrades to empty");

        // Malformed JSON / missing anchors → empty (fail-open, no panic).
        assert!(parse_anchors("not json").is_empty());
        assert!(parse_anchors(r#"{"foo":1}"#).is_empty());
    }

    #[test]
    fn outcome_payload_marks_anchor_hit_with_union_of_terms() {
        // AC-1 (signal half): a Read of an anchor is marked wasAnchor=true.
        let ax = anchors(&[("src/refund.cs", &["refund"]), ("src/order.cs", &["order"])]);
        let v = outcome_payload("/proj/src/refund.cs", &ax);
        assert_eq!(v["wasAnchor"], json!(true), "anchor suffix-match is a hit: {v}");
        assert_eq!(v["terms"], json!(["refund"]));
        assert_eq!(v["file"], json!("/proj/src/refund.cs"), "path normalised, not basename");
    }

    #[test]
    fn outcome_payload_non_anchor_read_is_not_a_hit() {
        // AC-3 (signal half): a Read of a non-anchor file is wasAnchor=false
        // with no terms — the projection counts it as a non-suggested read.
        let ax = anchors(&[("src/refund.cs", &["refund"])]);
        let v = outcome_payload("/proj/src/unrelated.cs", &ax);
        assert_eq!(v["wasAnchor"], json!(false), "non-anchor is not a hit: {v}");
        assert_eq!(v["terms"], json!([]));
    }

    #[test]
    fn outcome_payload_unions_and_sorts_terms_across_matching_anchors() {
        // The same basename declared by two anchors → union of their terms,
        // sorted (byte-stability for the projection's per-term fold).
        let ax = anchors(&[
            ("a/shared.cs", &["zeta", "alpha"]),
            ("b/shared.cs", &["alpha", "mu"]),
        ]);
        let v = outcome_payload("/proj/a/shared.cs", &ax);
        assert_eq!(v["wasAnchor"], json!(true));
        // basename match pulls BOTH anchors; terms deduped + sorted.
        assert_eq!(v["terms"], json!(["alpha", "mu", "zeta"]), "sorted union: {v}");
    }

    #[test]
    fn outcome_payload_normalises_backslashes_in_file_key() {
        let ax = anchors(&[("src/refund.cs", &["refund"])]);
        let v = outcome_payload(r"C:\proj\src\refund.cs", &ax);
        assert_eq!(v["file"], json!("C:/proj/src/refund.cs"), "windows path normalised");
        assert_eq!(v["wasAnchor"], json!(true), "match survives normalisation");
    }

    #[test]
    fn correlate_no_marker_is_silent_noop() {
        // AC-4 (signal half): with no marker on disk, nothing is emitted and
        // the call returns None (the observe() caller then proceeds — a Read
        // is never blocked). marker_path_for resolves the session via env/FS,
        // and a fresh tempdir has neither a session dir nor a marker.
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_str().unwrap();
        assert!(correlate_and_emit(cwd, "/proj/src/refund.cs").is_none());
    }

    #[test]
    fn touch_matches_anchor_strategies() {
        // exact, suffix, /-segment, basename — and a clean miss.
        assert!(touch_matches_anchor("src/refund.cs", "refund.cs", "src/refund.cs"));
        assert!(touch_matches_anchor("/proj/src/refund.cs", "refund.cs", "src/refund.cs"));
        assert!(touch_matches_anchor("/proj/x/refund.cs", "refund.cs", "refund.cs"));
        assert!(!touch_matches_anchor("/proj/src/other.cs", "other.cs", "src/refund.cs"));
        // empty anchor never matches.
        assert!(!touch_matches_anchor("src/refund.cs", "refund.cs", ""));
    }
}
