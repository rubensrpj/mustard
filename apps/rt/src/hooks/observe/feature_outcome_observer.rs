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
//! ## Window binding (research-scoped, deterministically closed)
//!
//! `feature::run` drops a small `active-research.json` marker in the session
//! (`.claude/.session/<id>/active-research.json`). The marker now carries a
//! research WINDOW (`opened_at` / `expires_at`, mirroring the `WindowState`
//! pattern of `amend_window_inject`) alongside the anchors + per-anchor terms.
//! This observer fires on EVERY `PostToolUse` and disciplines the window:
//!
//! - A Read/Edit/Write while the window is OPEN (not age-expired) correlates
//!   the touched file with the anchors and emits one `feature.outcome` event.
//!   The FIRST such emission flips the marker's `touched` flag to `true`.
//! - A tool that is NOT Read/Edit/Write closes the window (the marker is
//!   removed) ONLY once `touched == true` — i.e. only after the orchestrator
//!   has read at least one anchor. The `feature` command itself runs via the
//!   Bash tool, so its own `PostToolUse(Bash)` (and the ANALYZE Bash steps that
//!   precede the first read) would otherwise wipe the window before any read;
//!   gating on `touched` keeps the window alive until the research reads happen,
//!   then the next non-research tool closes it so the implementation phase that
//!   follows never leaks in as outcomes.
//! - An age-expired window (`now > expires_at`) is a backstop: the marker is
//!   removed and nothing is emitted.
//!
//! Binding the outcome to the marker (rather than correlating by a wall-clock
//! window in the projection) keeps the emitted events scoped to the research
//! that produced them: no marker on disk ⇒ no event. The marker carries the
//! anchors + the per-anchor terms, so the correlation is a pure set membership
//! test here — the projection never has to re-derive which file belonged to
//! which query.
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
use mustard_core::time::{now_iso8601, parse_iso_millis};
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
/// SIGNAL counts opens, so a read counts exactly like an edit. Any OTHER tool
/// is a research-phase boundary: it closes the window (the orchestrator has
/// moved on from reading anchors to acting on them).
fn is_research_tool(tool: Option<&str>) -> bool {
    matches!(tool, Some("Read" | "Edit" | "Write"))
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
/// `{ "opened_at": iso, "expires_at": iso, "anchors": [ { "file": "...",
/// "terms": ["..."] }, ... ] }`.
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

/// `true` when the marker's `expires_at` is in the past relative to `now_iso`.
/// An absent / unparsable `expires_at` is treated as NOT expired (the window
/// has no age backstop, so it stays open until a non-research tool closes it) —
/// markers written before this field existed still behave as before.
fn is_age_expired(raw: &str, now_iso: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(raw) else {
        return false;
    };
    let Some(expires_at) = v.get("expires_at").and_then(Value::as_str) else {
        return false;
    };
    let (Some(exp_ms), Some(now_ms)) = (parse_iso_millis(expires_at), parse_iso_millis(now_iso))
    else {
        return false;
    };
    now_ms > exp_ms
}

/// `true` when the marker's `touched` flag is set — i.e. at least one outcome
/// has been emitted inside this window (the orchestrator has read an anchor).
/// An absent / unparsable flag is treated as `false` (the window has not been
/// touched yet, so a non-research tool must NOT close it): legacy markers
/// written before this field existed stay open until age expiry or a touched
/// read closes them, which is the safe default for the loop we are measuring.
fn is_touched(raw: &str) -> bool {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|v| v.get("touched").and_then(Value::as_bool))
        .unwrap_or(false)
}

/// Persist `touched: true` back into the marker at `marker`, preserving every
/// other field. Best-effort: a missing / malformed marker or a failed write is
/// a silent no-op (the flag is correlation discipline, never correctness). Only
/// the FIRST emission needs to flip it; re-writing an already-true marker is
/// harmless, so the caller may skip the read-back and just call this once.
fn mark_touched(marker: &Path) {
    let Ok(raw) = fs::read_to_string(marker) else {
        return;
    };
    let Ok(mut v) = serde_json::from_str::<Value>(&raw) else {
        return;
    };
    let Some(obj) = v.as_object_mut() else {
        return;
    };
    obj.insert("touched".to_string(), Value::Bool(true));
    if let Ok(bytes) = serde_json::to_vec(&v) {
        let _ = fs::write_atomic(marker, &bytes);
    }
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

/// Remove the active-research marker at `marker` unconditionally — the
/// deterministic "window closed" signal. Best-effort: a missing file or a
/// failed unlink is a silent no-op (the next read just finds no marker and
/// no-ops anyway). Used by the age-expiry backstop, which closes regardless of
/// `touched`.
fn close_window_at(marker: &Path) {
    let _ = fs::remove_file(marker);
}

/// Close the window at `marker` from a NON-research tool, but ONLY once it has
/// been touched (an outcome was emitted). An untouched window survives: the
/// Bash that ran `feature` and the ANALYZE Bash steps before the first read do
/// not wipe the window. An unreadable marker degrades to "not touched" (no
/// removal) — fail-open, the age backstop or a later touched close handles it.
fn close_window_if_touched_at(marker: &Path) {
    let Ok(raw) = fs::read_to_string(marker) else {
        return;
    };
    if is_touched(&raw) {
        let _ = fs::remove_file(marker);
    }
}

/// Correlate a touched file against an EXPLICIT marker path and emit one
/// `feature.outcome` event. Returns the emitted payload (for tests); `None`
/// when nothing was emitted (no marker on disk / no anchors / window
/// age-expired). An age-expired window is also CLOSED here (marker removed) so
/// it cannot keep leaking outcomes. Fail-open: every IO step degrades to
/// `None`. Splitting the path resolution out (`marker_path_for`) keeps this
/// core testable without the process-global session env var.
fn correlate_and_emit_at(cwd: &str, marker: &Path, touched: &str) -> Option<Value> {
    let raw = fs::read_to_string(marker).ok()?;
    // Backstop: an age-expired window emits nothing and removes the marker.
    if is_age_expired(&raw, &now_iso8601()) {
        close_window_at(marker);
        return None;
    }
    let anchors = parse_anchors(&raw);
    if anchors.is_empty() {
        return None;
    }
    let payload = outcome_payload(touched, &anchors);
    economy::emit(cwd, ActorKind::Hook, "feature-outcome", EVENT_FEATURE_OUTCOME, None, payload.clone());
    // First read inside the window flips `touched` so the next non-research
    // tool is allowed to close it (the Bash that opened the window does not).
    mark_touched(marker);
    Some(payload)
}

/// The whole `PostToolUse` decision against an EXPLICIT marker path — the
/// testable core of [`Observer::observe`], free of the process-global session
/// resolution (`marker_path_for`). Drives the exact dispatch `observe()` runs:
///
/// - A NON-research tool closes the window only if it has been touched
///   (`close_window_if_touched_at`) — the Bash that ran `feature` survives.
/// - A research tool (`Read`/`Edit`/`Write`) with a `file_path` correlates and
///   emits, flipping `touched` on the first emission.
///
/// Returns the emitted payload when a research read emitted one, else `None`
/// (non-research tool, no path, or nothing to emit). Side-effect only.
fn step_window_at(cwd: &str, marker: &Path, tool: Option<&str>, file_path: Option<&str>) -> Option<Value> {
    if !is_research_tool(tool) {
        // Research burst is over (once touched) — close so the implementation
        // phase that follows is not counted as outcomes.
        close_window_if_touched_at(marker);
        return None;
    }
    let touched = file_path.filter(|s| !s.is_empty())?;
    correlate_and_emit_at(cwd, marker, touched)
}

impl Observer for FeatureOutcomeObserver {
    /// Discipline the research window on every `PostToolUse`:
    ///
    /// - A Read/Edit/Write while the window is open (not age-expired) emits one
    ///   `feature.outcome` for the touched file (flipping `touched` on the
    ///   first emission).
    /// - A NON-Read/Edit/Write tool closes the window (removes the marker) ONLY
    ///   once it has been touched — so the Bash that ran `feature` itself does
    ///   not wipe the window before any anchor is read.
    ///
    /// Pure side effect — never a verdict (a tool always proceeds), fail-open
    /// throughout.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return;
        }
        let cwd = ctx.project_dir_or_cwd(input);
        let Some(marker) = marker_path_for(&cwd) else {
            return;
        };
        let file_path = file_path_of(input);
        let _ = step_window_at(&cwd, &marker, input.tool_name.as_deref(), file_path.as_deref());
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
    fn step_no_marker_is_silent_noop() {
        // AC-4 (signal half): with no marker on disk, a research Read emits
        // nothing and returns None (the observe() caller then proceeds — a Read
        // is never blocked). Drives the explicit-marker core so it is free of
        // the process-global session resolution (`marker_path_for`).
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_str().unwrap();
        let absent = dir.path().join(ACTIVE_RESEARCH_MARKER); // never created
        assert!(
            step_window_at(cwd, &absent, Some("Read"), Some("/proj/src/refund.cs")).is_none(),
            "a Read against an absent marker emits nothing"
        );
        // A non-research tool against an absent marker is likewise a no-op
        // (close_window_if_touched_at fails the read → no removal, no panic).
        assert!(step_window_at(cwd, &absent, Some("Bash"), None).is_none());
        assert!(!absent.exists(), "no marker was created");
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

    // --- window discipline (age expiry) — pure, no FS / session -------------

    #[test]
    fn is_age_expired_compares_expires_at_to_now() {
        let raw = r#"{ "opened_at": "2026-06-20T00:00:00.000Z",
                       "expires_at": "2026-06-20T00:30:00.000Z", "anchors": [] }"#;
        // now AFTER expires_at → expired.
        assert!(is_age_expired(raw, "2026-06-20T00:30:01.000Z"));
        // now BEFORE expires_at → still open.
        assert!(!is_age_expired(raw, "2026-06-20T00:29:59.000Z"));
        // no expires_at field → not expired (legacy markers stay open).
        assert!(!is_age_expired(r#"{ "anchors": [] }"#, "2026-06-20T00:30:01.000Z"));
        // malformed JSON → not expired (fail-open, no panic).
        assert!(!is_age_expired("not json", "2026-06-20T00:30:01.000Z"));
    }

    // --- window discipline (observe path, marker-explicit) -----------------
    //
    // These drive the window-discipline core through the `_at` variants with an
    // explicit marker path, so they need NO process-global session env var
    // (`std::env::set_var` is `unsafe` in edition 2024 — the crate convention is
    // to inject the dependency instead). `marker_path_for` (the env-dependent
    // resolution it wraps) is covered separately by `correlate_no_marker_is_silent_noop`.

    /// Write an `active-research.json` marker at `marker` with the given
    /// `opened_at`/`expires_at` window and a single anchor.
    fn seed_marker(marker: &Path, opened_at: &str, expires_at: &str) {
        if let Some(parent) = marker.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let body = json!({
            "terms": ["refund"],
            "anchors": [ { "file": "src/refund.cs", "terms": ["refund"] } ],
            "ts": opened_at,
            "opened_at": opened_at,
            "expires_at": expires_at,
        });
        std::fs::write(marker, serde_json::to_vec(&body).unwrap()).unwrap();
    }

    /// `opened_at` = now, `expires_at` = now + 30 min — a fresh, open window.
    fn open_window_bounds() -> (String, String) {
        let now = now_iso8601();
        let future = mustard_core::time::millis_to_iso(
            mustard_core::time::parse_iso_millis(&now).unwrap_or(0) + 30 * 60 * 1000,
        );
        (now, future)
    }

    #[test]
    fn outcome_within_open_window_emits() {
        // AC-3: a Read inside an OPEN, non-expired window emits feature.outcome.
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_str().unwrap();
        let marker = dir.path().join(ACTIVE_RESEARCH_MARKER);
        let (opened, expires) = open_window_bounds();
        seed_marker(&marker, &opened, &expires);

        let emitted = correlate_and_emit_at(cwd, &marker, "/proj/src/refund.cs");
        assert!(emitted.is_some(), "open window emits an outcome");
        assert_eq!(emitted.unwrap()["wasAnchor"], json!(true), "the anchor was read");
        // The window stays open after a research read.
        assert!(marker.exists(), "marker still open after a Read");
    }

    #[test]
    fn outcome_after_window_close_is_silent() {
        // AC-4: the first NON-Read/Edit/Write tool closes the window (removes
        // the marker); a Read after that emits nothing. The observer routes a
        // non-research tool to `close_window`, modelled here by `close_window_at`.
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_str().unwrap();
        let marker = dir.path().join(ACTIVE_RESEARCH_MARKER);
        let (opened, expires) = open_window_bounds();
        seed_marker(&marker, &opened, &expires);
        assert!(marker.exists(), "precondition: window open");

        // A non-research tool closes the window.
        close_window_at(&marker);
        assert!(!marker.exists(), "non-research tool closes the window");

        // A subsequent Read finds no marker → emits nothing (read fails → None).
        assert!(
            correlate_and_emit_at(cwd, &marker, "/proj/src/refund.cs").is_none(),
            "no outcome after the window closed"
        );
    }

    #[test]
    fn outcome_after_age_expiry_is_silent() {
        // AC-5: a Read against an AGE-EXPIRED window emits nothing (and the
        // backstop removes the stale marker).
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_str().unwrap();
        let marker = dir.path().join(ACTIVE_RESEARCH_MARKER);
        // Window opened and expired in the past.
        seed_marker(&marker, "2020-01-01T00:00:00.000Z", "2020-01-01T00:30:00.000Z");
        assert!(marker.exists(), "precondition: stale marker present");

        assert!(
            correlate_and_emit_at(cwd, &marker, "/proj/src/refund.cs").is_none(),
            "no outcome after age expiry"
        );
        assert!(!marker.exists(), "age-expired marker is removed by the backstop");
    }

    // --- touched-gated close (the Bash-of-feature defect) -------------------
    //
    // `feature` runs via the Bash tool, so the very `PostToolUse(Bash)` of the
    // command that OPENED the window used to wipe it before any anchor read.
    // The fix: a non-research tool only closes the window once `touched` is set
    // (flipped on the first outcome emission). These drive the explicit-marker
    // dispatch core `step_window_at` (no process-global session env var).

    #[test]
    fn window_not_closed_before_first_read() {
        // AC-3: a non-research tool (Bash) with touched=false does NOT close the
        // window — simulates the Bash that ran `feature` firing right after the
        // marker was dropped, before the orchestrator read any anchor.
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_str().unwrap();
        let marker = dir.path().join(ACTIVE_RESEARCH_MARKER);
        let (opened, expires) = open_window_bounds();
        seed_marker(&marker, &opened, &expires); // seed_marker omits `touched` ⇒ false
        assert!(!is_touched(&std::fs::read_to_string(&marker).unwrap()), "precondition: untouched");

        // The Bash of `feature` itself: a non-research tool, marker untouched.
        let out = step_window_at(cwd, &marker, Some("Bash"), None);
        assert!(out.is_none(), "non-research tool emits nothing");
        assert!(marker.exists(), "untouched window SURVIVES the Bash that opened it");
    }

    #[test]
    fn window_closes_after_first_read_then_nonresearch() {
        // AC-4: once a Read has emitted an outcome (touched flips true), the next
        // non-research tool DOES close the window.
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_str().unwrap();
        let marker = dir.path().join(ACTIVE_RESEARCH_MARKER);
        let (opened, expires) = open_window_bounds();
        seed_marker(&marker, &opened, &expires);

        // First Read: emits + flips touched.
        let emitted = step_window_at(cwd, &marker, Some("Read"), Some("/proj/src/refund.cs"));
        assert!(emitted.is_some(), "the anchor read emits an outcome");
        assert!(marker.exists(), "window still open after the read");
        assert!(is_touched(&std::fs::read_to_string(&marker).unwrap()), "touched flipped true");

        // Now a non-research tool closes the touched window.
        let out = step_window_at(cwd, &marker, Some("Bash"), None);
        assert!(out.is_none(), "non-research tool emits nothing");
        assert!(!marker.exists(), "touched window closes on the next non-research tool");
    }

    #[test]
    fn outcome_window_lifecycle_open_read_close() {
        // AC-5: full lifecycle at the observe() dispatch level (step_window_at),
        // exercising the real Bash→Read ordering the parent's unit ACs missed:
        //   open → Bash (survives) → Read (counts + touched) → Bash (closes) → Read (no count).
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_str().unwrap();
        let marker = dir.path().join(ACTIVE_RESEARCH_MARKER);
        let (opened, expires) = open_window_bounds();
        seed_marker(&marker, &opened, &expires);

        // 1. Bash (the `feature` command itself) — untouched, must NOT close.
        assert!(step_window_at(cwd, &marker, Some("Bash"), None).is_none());
        assert!(marker.exists(), "step 1: Bash-of-feature does not close the window");

        // 2. Read of an anchor — emits and flips touched.
        let first = step_window_at(cwd, &marker, Some("Read"), Some("/proj/src/refund.cs"));
        assert_eq!(first.unwrap()["wasAnchor"], json!(true), "step 2: the anchor was counted");
        assert!(marker.exists() && is_touched(&std::fs::read_to_string(&marker).unwrap()), "step 2: touched");

        // 3. Bash — touched now, so the window closes.
        assert!(step_window_at(cwd, &marker, Some("Bash"), None).is_none());
        assert!(!marker.exists(), "step 3: touched window closes on a non-research tool");

        // 4. Read after close — no marker, nothing counted (implementation-phase read).
        assert!(
            step_window_at(cwd, &marker, Some("Read"), Some("/proj/src/order.cs")).is_none(),
            "step 4: a read after the window closed is not counted"
        );
    }
}
