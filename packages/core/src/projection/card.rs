//! [`project_spec_view`] — fold the event stream for one spec into a
//! [`SpecView`].
//!
//! Per the dashboard audit (2026-05-20), the dashboard's `spec_views.rs`
//! returned literal `"unknown"` whenever the SQL fallback fired. This fold
//! replaces that path with a deterministic projection over typed events:
//!
//! - `pipeline.scope` populates `scope`, `lang`, `model`, `is_wave_plan`,
//!   `total_waves`. It also transitions `status` away from `NoEvents`.
//! - `pipeline.status` transitions `status` (parsed via [`SpecStatus::parse`]).
//! - `pipeline.phase` updates `phase`.
//! - `pipeline.task.complete` accumulates `files_touched` (deduplicated).
//! - `pipeline.wave.complete` extends `completed_waves` and recomputes
//!   `current_wave`.
//! - `pipeline.complete` flips the status to `Completed`.
//! - `qa.result` overwrites `ac_passed`/`ac_total`/`ac_failed` from the
//!   latest event (newer wins; folds in chronological order).
//! - `tool.use` bumps `tools_used`.
//! - `agent.start` bumps `agents_dispatched`.
//!
//! Events with `spec != Some(spec_name)` are filtered out before the fold —
//! callers that pre-filtered (e.g. `store.query(Some(name))`) pay zero cost.
//!
//! ## Header fallback (Wave 1, 2026-05-21)
//!
//! When the event stream is empty and a `spec.md` path is supplied, the fold
//! parses the `### Status:` / `### Phase:` / `### Scope:` / `### Lang:` lines
//! from the header and seeds the [`SpecView`] from them. This is the
//! cross-collaborator path: the `SQLite` event log is per-machine and never
//! versioned, but the `spec.md` header *is* checked into git. A teammate who
//! pulls the repo sees a populated dashboard without re-emitting any events.
//!
//! The fallback is opt-in (`spec_md_path = None` disables it). Timestamps stay
//! `None` because the header alone cannot prove when work started or last
//! happened.

#[allow(deprecated)] // the fold still computes the legacy SpecStatus, then derives SpecState from it.
use crate::model::view::SpecStatus;
use crate::model::view::{Flags, Outcome, Phase, Scope, SpecState, SpecView, Stage};
use crate::model::event::{
    Actor, ActorKind, HarnessEvent, PipelineScopePayload, PipelineTaskCompletePayload,
    PipelineWaveCompletePayload, SCHEMA_VERSION,
};
use crate::store::event_store::EventSink;
use std::collections::BTreeSet;
use std::path::Path;

use super::{extract_to_phase, iso_diff_ms};

/// Fold `events` into a [`SpecView`] for `spec_name`.
///
/// Thin wrapper around [`project_spec_view_with_header`] that disables the
/// header fallback. Kept for callers (workspace projection, tests) that have
/// no `spec.md` path to resolve and never want the synthetic-emit side effect.
#[must_use]
pub fn project_spec_view(spec_name: &str, events: &[HarnessEvent]) -> SpecView {
    project_spec_view_with_header(spec_name, events, None, None)
}

/// Fold `events` into a [`SpecView`] for `spec_name`, with an optional
/// `spec.md` header fallback when the event stream is empty.
///
/// `spec_md_path` is the on-disk path to the spec's `spec.md`. When supplied
/// **and** `events` is empty, the fold reads the file, parses the `###
/// Status:`, `### Phase:`, `### Scope:`, `### Lang:` header lines, and seeds
/// the view from them. `started_at` and `last_event_at` stay `None` — the
/// header alone is not evidence of *when* work happened.
///
/// `emit_sink`, when supplied, receives a single synthetic
/// `pipeline.status` event when the header fallback fires with a parseable
/// status. This is the Wave 5 backfill hook: the next read becomes O(1)
/// because the event log no longer needs a re-parse. The emit is fail-open;
/// a sink error is swallowed so the projection still returns a usable view.
///
/// When `events` is non-empty the path and sink are ignored — the event log
/// is authoritative.
#[must_use]
pub fn project_spec_view_with_header(
    spec_name: &str,
    events: &[HarnessEvent],
    spec_md_path: Option<&Path>,
    emit_sink: Option<&dyn EventSink>,
) -> SpecView {
    // Event stream wins whenever it has anything for this spec. Filter first
    // so a stream full of other-spec noise still triggers the fallback.
    let scoped_count = events
        .iter()
        .filter(|e| e.spec.as_deref() == Some(spec_name))
        .count();
    if scoped_count == 0 {
        if let Some(path) = spec_md_path {
            if let Some(view) = view_from_header(spec_name, path, emit_sink) {
                return view;
            }
        }
        return SpecView::empty(spec_name);
    }
    project_from_events(spec_name, events)
}

/// Core event-stream fold — assumes `events` has at least one row scoped to
/// `spec_name`. Extracted from [`project_spec_view_with_header`] so the
/// fallback path can stay readable.
#[must_use]
fn project_from_events(spec_name: &str, events: &[HarnessEvent]) -> SpecView {
    let mut view = SpecView::empty(spec_name);
    let mut files: BTreeSet<String> = BTreeSet::new();

    for ev in events.iter().filter(|e| e.spec.as_deref() == Some(spec_name)) {
        // Time bookkeeping — every event refreshes `last_event_at` and may
        // seed `started_at`. Done before the per-event match so even Other
        // events anchor the timeline correctly.
        if view.started_at.is_none() {
            view.started_at = Some(ev.ts.clone());
        }
        view.last_event_at = Some(ev.ts.clone());

        match ev.event.as_str() {
            "pipeline.scope" => apply_scope(&mut view, ev),
            "pipeline.status" => apply_status(&mut view, ev),
            "pipeline.phase" => apply_phase(&mut view, ev),
            "pipeline.task.complete" => apply_task_complete(ev, &mut files),
            "pipeline.wave.complete" => apply_wave_complete(&mut view, ev),
            "pipeline.wave.failed" => apply_wave_failed(&mut view, ev),
            // `pipeline.complete` is a temporal audit marker emitted by
            // `complete_spec::mark_followup` alongside `pipeline.status:
            // closed-followup`. Treating it as a status transition to
            // `Completed` would clobber the ClosedFollowup state set by the
            // paired status event and bury follow-up specs in the wrong
            // dashboard bucket. The authoritative status source is
            // `pipeline.status` — `pipeline.complete` only carries `closedAt`
            // and the affected files list. Leave the status alone here.
            "qa.result" => apply_qa_result(&mut view, ev),
            "tool.use" => view.tools_used = view.tools_used.saturating_add(1),
            "agent.start" => view.agents_dispatched = view.agents_dispatched.saturating_add(1),
            _ => {}
        }
    }

    view.files_touched = u32::try_from(files.len()).unwrap_or(u32::MAX);

    // Duration: only meaningful when both timestamps exist.
    if let (Some(start), Some(end)) = (view.started_at.as_deref(), view.last_event_at.as_deref()) {
        view.duration_ms = iso_diff_ms(start, end);
    }

    // current_wave: max completed + 1, capped at total_waves.
    if let Some(total) = view.total_waves {
        let max_completed = view.completed_waves.iter().copied().max().unwrap_or(0);
        view.current_wave = Some((max_completed + 1).min(total));
    }

    // Derive the canonical SpecState from the legacy fold. Wave 1 keeps the
    // event fold expressed in the legacy `SpecStatus` vocabulary (see
    // `fold_legacy_status`) and lifts it into `SpecState` as a final step, so
    // both fields stay consistent for every projection branch.
    sync_state(&mut view);

    view
}

/// Keep [`SpecView::state`] in sync with the legacy [`SpecView::status`] fold.
///
/// The Wave 1 projection still folds the event stream in the flat
/// [`SpecStatus`] vocabulary — that fold is now named [`fold_legacy_status`]
/// for clarity — and derives the canonical [`SpecState`] from it here. Later
/// waves can flip the relationship (fold straight into `SpecState`) without
/// touching every per-event helper.
#[allow(deprecated)] // bridges the deprecated `status` field into `state`.
fn sync_state(view: &mut SpecView) {
    view.state = SpecState::from(fold_legacy_status(view));
}

/// The legacy event fold expressed in [`SpecStatus`] terms.
///
/// Retained as a named, documented entry point for the bridge in
/// [`sync_state`]: the per-event helpers (`apply_status`, `apply_scope`, …)
/// mutate `view.status`, and this is the value that fold produces. Marked
/// `#[allow(deprecated)]` because reading the deprecated field is intentional
/// here, not a misuse.
#[must_use]
#[allow(deprecated)]
pub fn fold_legacy_status(view: &SpecView) -> SpecStatus {
    view.status
}

/// `pipeline.scope` — first observation of a spec's metadata. Promotes the
/// view from `NoEvents` to `Planning` and records scope/lang/model.
#[allow(deprecated)] // mutates the legacy `status` fold; `state` is derived later.
fn apply_scope(view: &mut SpecView, ev: &HarnessEvent) {
    if let Ok(payload) = serde_json::from_value::<PipelineScopePayload>(ev.payload.clone()) {
        view.scope = Scope::parse(&payload.scope);
        view.lang = payload.lang;
        view.model = payload.model;
        view.is_wave_plan = payload.is_wave_plan.unwrap_or(false);
        view.total_waves = payload.total_waves;
        // First scope event → leaves NoEvents behind. Status transitions
        // beyond Planning happen via `pipeline.status`.
        if view.status == SpecStatus::NoEvents {
            view.status = SpecStatus::Planning;
        }
    }
}

/// `pipeline.status` — typed transitions. Unknown strings leave status
/// unchanged rather than dropping back to `NoEvents`.
#[allow(deprecated)] // mutates the legacy `status` fold; `state` is derived later.
fn apply_status(view: &mut SpecView, ev: &HarnessEvent) {
    let Some(to) = ev.payload.get("to").and_then(serde_json::Value::as_str) else {
        return;
    };
    if let Some(parsed) = SpecStatus::parse(to) {
        view.status = parsed;
    }
}

/// `pipeline.phase` — current phase. Parsed via [`Phase::parse`].
fn apply_phase(view: &mut SpecView, ev: &HarnessEvent) {
    if let Some(phase) = extract_to_phase(ev) {
        view.phase = Some(phase);
    }
}

/// `pipeline.task.complete` — accumulates `files_touched` (deduplicated
/// across all tasks). Decoding failures skip the row, matching the rest of
/// the harness's fail-open style.
fn apply_task_complete(ev: &HarnessEvent, files: &mut BTreeSet<String>) {
    let Ok(payload) = serde_json::from_value::<PipelineTaskCompletePayload>(ev.payload.clone()) else {
        return;
    };
    if let Some(modified) = payload.files_modified {
        files.extend(modified);
    }
}

/// `pipeline.wave.complete` — track the wave number.
fn apply_wave_complete(view: &mut SpecView, ev: &HarnessEvent) {
    let Ok(payload) = serde_json::from_value::<PipelineWaveCompletePayload>(ev.payload.clone())
    else {
        return;
    };
    if !view.completed_waves.contains(&payload.wave) {
        view.completed_waves.push(payload.wave);
        view.completed_waves.sort_unstable();
    }
}

/// `pipeline.wave.failed` — track failed waves. The event has no typed
/// payload struct in `mustard-core` yet, so we read the `wave` field directly.
#[allow(deprecated)] // mutates the legacy `status` fold; `state` is derived later.
fn apply_wave_failed(view: &mut SpecView, ev: &HarnessEvent) {
    let Some(wave) = ev
        .payload
        .get("wave")
        .and_then(serde_json::Value::as_u64)
        .and_then(|w| u32::try_from(w).ok())
    else {
        return;
    };
    if !view.failed_waves.contains(&wave) {
        view.failed_waves.push(wave);
        view.failed_waves.sort_unstable();
    }
    view.status = SpecStatus::WaveFailed;
}

/// `qa.result` — overwrite the AC counts with the latest event's numbers.
/// Folds in chronological order so the last one wins.
fn apply_qa_result(view: &mut SpecView, ev: &HarnessEvent) {
    // Two payload shapes exist in the wild: the original `qa_run.rs`
    // emits a `criteria` array; some earlier emitters embedded `passed`/
    // `total` directly. Try the array form first.
    if let Some(criteria) = ev.payload.get("criteria").and_then(serde_json::Value::as_array) {
        let mut passed = 0u32;
        let mut failed = 0u32;
        for entry in criteria {
            let status = entry
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            match status {
                "pass" => passed = passed.saturating_add(1),
                "fail" | "error" => failed = failed.saturating_add(1),
                _ => {}
            }
        }
        let total = u32::try_from(criteria.len()).unwrap_or(u32::MAX);
        view.ac_passed = passed;
        view.ac_failed = failed;
        view.ac_total = total;
        return;
    }

    // Legacy / shorthand payload form: numeric `passed`/`total`/`failed`.
    if let Some(passed) = ev
        .payload
        .get("passed")
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
    {
        view.ac_passed = passed;
    }
    if let Some(total) = ev
        .payload
        .get("total")
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
    {
        view.ac_total = total;
    }
    if let Some(failed) = ev
        .payload
        .get("failed")
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
    {
        view.ac_failed = failed;
    }
}

/// Parse the spec.md header (the first contiguous block of `### Key: value`
/// lines after the leading `# Title`) and build a [`SpecView`] from whatever
/// values are recognised. Returns `None` when the file is missing or every
/// header field is unrecognised — the caller falls back to [`SpecView::empty`].
///
/// When a status is parsed and `emit_sink` is provided, a single synthetic
/// `pipeline.status` event is appended. Failures are swallowed: telemetry is
/// never load-bearing.
#[allow(deprecated)] // seeds the legacy `status` field from the header; `state` derived after.
fn view_from_header(
    spec_name: &str,
    path: &Path,
    emit_sink: Option<&dyn EventSink>,
) -> Option<SpecView> {
    let raw = crate::fs::read_to_string(path).ok()?;
    let header = parse_header_fields(&raw);
    if header.is_empty() {
        return None;
    }

    let mut view = SpecView::empty(spec_name);

    // New canonical header (`### Stage:` / `### Outcome:` / `### Flags:`) takes
    // precedence when a `### Stage:` line is present. The legacy `### Status:`
    // / `### Phase:` block remains the fallback for specs not yet rewritten
    // (rewrite is Wave 7). When the new header parses into a legal SpecState we
    // seed both `state` (canonical) and `status` (derived, for the synthetic
    // emit and back-compat readers).
    if let Some(state) = state_from_new_header(&header) {
        view.state = state.clone();
        if let Ok(status) = SpecStatus::try_from(state) {
            view.status = status;
        }
        if let Some(phase_raw) = header.get("phase") {
            if let Some(phase) = Phase::parse(phase_raw) {
                view.phase = Some(phase);
            }
        }
        seed_header_metadata(&mut view, &header);
        return Some(view);
    }

    if let Some(status_raw) = header.get("status") {
        if let Some(status) = SpecStatus::parse(status_raw) {
            view.status = status;
            // Opt-in synthetic emit. Backfills the local SQLite log so the
            // next read is O(1) — see Wave 5 in
            // 2026-05-21-flatten-spec-layout-and-multi-collab.
            if let Some(sink) = emit_sink {
                let synthetic = HarnessEvent {
                    v: SCHEMA_VERSION,
                    ts: header_emit_timestamp(),
                    session_id: "header-fallback".to_string(),
                    wave: 0,
                    actor: Actor {
                        kind: ActorKind::Hook,
                        id: Some("card-projection".to_string()),
                        actor_type: None,
                    },
                    event: "pipeline.status".to_string(),
                    payload: serde_json::json!({ "from": null, "to": status_raw }),
                    spec: Some(spec_name.to_string()),
                };
                // Fail-open: a failing sink must not break the projection.
                let _ = sink.append(&synthetic);
            }
        }
    }
    if let Some(phase_raw) = header.get("phase") {
        if let Some(phase) = Phase::parse(phase_raw) {
            view.phase = Some(phase);
        }
    }
    seed_header_metadata(&mut view, &header);

    // Derive the canonical state from the legacy `status` the header seeded.
    sync_state(&mut view);

    Some(view)
}

/// Seed `scope` and `lang` from the header map. Shared by the new-format and
/// legacy-format paths in [`view_from_header`].
fn seed_header_metadata(
    view: &mut SpecView,
    header: &std::collections::BTreeMap<String, String>,
) {
    if let Some(scope_raw) = header.get("scope") {
        if let Some(scope) = Scope::parse(scope_raw) {
            view.scope = Some(scope);
        }
    }
    if let Some(lang_raw) = header.get("lang") {
        let trimmed = lang_raw.trim();
        if !trimmed.is_empty() {
            view.lang = Some(trimmed.to_string());
        }
    }
}

/// Build a [`SpecState`] from the new canonical header fields, when present.
///
/// Requires a `### Stage:` line — that is the marker that distinguishes the new
/// format from the legacy `### Status:` block. `### Outcome:` defaults to
/// `Active` when absent (a running spec rarely writes it); `### Flags:` is an
/// optional comma/space-separated list. Returns `None` when no `Stage` line is
/// present, when the stage value is unparseable, or when the resulting triple
/// is illegal (so the caller falls back to the legacy header).
fn state_from_new_header(
    header: &std::collections::BTreeMap<String, String>,
) -> Option<SpecState> {
    let stage = Stage::parse(header.get("stage")?)?;
    let outcome = header
        .get("outcome")
        .and_then(|raw| Outcome::parse(raw))
        .unwrap_or(Outcome::Active);
    let flags = header.get("flags").map(|raw| Flags::parse(raw)).unwrap_or_default();
    SpecState::new(stage, outcome, flags).ok()
}

/// Walk the file and collect `### Key: value` pairs from the leading header
/// block. Stops at the first non-header content (a `##` line, plain prose,
/// fenced block, etc.) so a `### …` heading deep inside the PRD is never
/// mistaken for status metadata.
///
/// Keys are lowercased so the caller can look them up without worrying about
/// the original case (`### Status:` vs `### status:`).
fn parse_header_fields(raw: &str) -> std::collections::BTreeMap<String, String> {
    let mut out: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut seen_first_header_line = false;
    for line in raw.lines() {
        let trimmed = line.trim_start();
        // Skip the title and blank prelude before the first `### Key:` line.
        if !seen_first_header_line {
            if trimmed.starts_with("# ") || trimmed.is_empty() {
                continue;
            }
            // Any `## Section` heading or non-empty prose before a `### Key:`
            // line means the spec has no header block at all.
            if trimmed.starts_with("## ") || !trimmed.starts_with("### ") {
                if trimmed.starts_with("### ") {
                    // Fall through to the parse arm below.
                } else {
                    return out;
                }
            }
        }
        if let Some(rest) = trimmed.strip_prefix("### ") {
            if let Some((key, value)) = rest.split_once(':') {
                seen_first_header_line = true;
                out.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
                continue;
            }
            // A `### …` line without a colon ends the header block.
            if seen_first_header_line {
                break;
            }
            continue;
        }
        // First non-header line after we entered the header block → stop.
        if seen_first_header_line {
            break;
        }
    }
    out
}

/// Wall-clock now in ISO-8601 second precision for the synthetic emit.
///
/// Falls back to the Unix epoch (`1970-01-01T00:00:00Z`) on the rare
/// clock-error case so the emit still passes the store's schema validation.
/// Mirrors the formatting helper in `economy::sources::time::now_iso` — kept
/// inline because that helper is `pub(super)` to its module.
#[allow(
    clippy::cast_possible_truncation, // i64 → u32: day/month/hour/min/sec fit safely in u32
    clippy::cast_sign_loss,           // i64 → u32: calendar fields are always non-negative
    clippy::cast_possible_wrap,       // u64 → i64: seconds-since-epoch fits in i64 for millennia
    clippy::many_single_char_names    // Howard Hinnant's civil-to-date algorithm uses its canonical variable names
)]
fn header_emit_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    // Howard Hinnant's days-from-civil, inverted. Same algorithm used in
    // `economy::sources::time` — duplicated rather than re-exported because
    // the only other consumer keeps the helper module-private.
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    let h = (tod / 3600) as u32;
    let mi = ((tod % 3600) / 60) as u32;
    let s = (tod % 60) as u32;
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

#[cfg(test)]
#[allow(deprecated)] // these tests intentionally assert against the legacy SpecStatus path.
mod tests {
    use super::*;
    use crate::model::view::Phase;
    use crate::store::event_store::EventSink;
    use crate::error::Result as CoreResult;
    use std::cell::RefCell;
    use serde_json::json;

    /// Build a minimal event with given kind and payload, scoped to `spec`.
    fn event(spec: &str, ts: &str, kind: &str, payload: serde_json::Value) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.to_string(),
            session_id: "s1".into(),
            wave: 0,
            actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
            event: kind.into(),
            payload,
            spec: Some(spec.into()),
        }
    }

    #[test]
    fn empty_events_yield_empty_view() {
        let view = project_spec_view("feature-a", &[]);
        assert_eq!(view.spec, "feature-a");
        assert_eq!(view.status, SpecStatus::NoEvents);
        assert_eq!(view.tools_used, 0);
        assert!(view.started_at.is_none());
    }

    #[test]
    fn events_for_other_specs_are_skipped() {
        let events = vec![
            event("feature-a", "2026-05-20T10:00:00Z", "tool.use", json!({})),
            event("feature-b", "2026-05-20T10:01:00Z", "tool.use", json!({})),
            event("feature-a", "2026-05-20T10:02:00Z", "tool.use", json!({})),
        ];
        let view = project_spec_view("feature-a", &events);
        assert_eq!(view.tools_used, 2);
    }

    #[test]
    fn scope_event_transitions_status_and_records_metadata() {
        let events = vec![event(
            "feature-a",
            "2026-05-20T10:00:00Z",
            "pipeline.scope",
            json!({
                "scope": "full",
                "lang": "pt",
                "model": "opus",
                "is_wave_plan": true,
                "total_waves": 4
            }),
        )];
        let view = project_spec_view("feature-a", &events);
        assert_eq!(view.status, SpecStatus::Planning);
        assert_eq!(view.scope, Some(Scope::Full));
        assert_eq!(view.lang.as_deref(), Some("pt"));
        assert_eq!(view.model.as_deref(), Some("opus"));
        assert!(view.is_wave_plan);
        assert_eq!(view.total_waves, Some(4));
    }

    #[test]
    fn status_events_transition_lifecycle_with_unknown_values_ignored() {
        let events = vec![
            event(
                "auth",
                "2026-05-20T10:00:00Z",
                "pipeline.status",
                json!({ "to": "implementing" }),
            ),
            event(
                "auth",
                "2026-05-20T10:01:00Z",
                "pipeline.status",
                json!({ "to": "garbage-state" }), // unknown → ignored
            ),
            event(
                "auth",
                "2026-05-20T10:02:00Z",
                "pipeline.status",
                json!({ "to": "completed" }),
            ),
        ];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.status, SpecStatus::Completed);
    }

    #[test]
    fn phase_event_updates_phase() {
        let events = vec![event(
            "auth",
            "2026-05-20T10:00:00Z",
            "pipeline.phase",
            json!({ "to": "execute" }),
        )];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.phase, Some(Phase::Execute));
    }

    #[test]
    fn task_complete_accumulates_distinct_files() {
        let events = vec![
            event(
                "auth",
                "2026-05-20T10:00:00Z",
                "pipeline.task.complete",
                json!({ "name": "wave-1", "files_modified": ["src/a.rs", "src/b.rs"] }),
            ),
            event(
                "auth",
                "2026-05-20T10:05:00Z",
                "pipeline.task.complete",
                json!({ "name": "wave-2", "files_modified": ["src/b.rs", "src/c.rs"] }),
            ),
        ];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.files_touched, 3); // a, b, c deduplicated
    }

    #[test]
    fn wave_complete_extends_list_and_drives_current_wave() {
        let events = vec![
            event(
                "auth",
                "2026-05-20T10:00:00Z",
                "pipeline.scope",
                json!({ "scope": "full", "total_waves": 4 }),
            ),
            event(
                "auth",
                "2026-05-20T10:05:00Z",
                "pipeline.wave.complete",
                json!({ "wave": 1 }),
            ),
            event(
                "auth",
                "2026-05-20T10:10:00Z",
                "pipeline.wave.complete",
                json!({ "wave": 2 }),
            ),
        ];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.completed_waves, vec![1, 2]);
        assert_eq!(view.current_wave, Some(3));
        assert_eq!(view.total_waves, Some(4));
    }

    #[test]
    fn qa_result_with_criteria_array_counts_pass_fail_total() {
        let events = vec![event(
            "auth",
            "2026-05-20T10:00:00Z",
            "qa.result",
            json!({
                "criteria": [
                    { "id": "AC-1", "status": "pass" },
                    { "id": "AC-2", "status": "pass" },
                    { "id": "AC-3", "status": "fail" },
                    { "id": "AC-4", "status": "skip" },
                ]
            }),
        )];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.ac_total, 4);
        assert_eq!(view.ac_passed, 2);
        assert_eq!(view.ac_failed, 1);
    }

    #[test]
    fn qa_result_with_legacy_shorthand_counts_numeric_fields() {
        let events = vec![event(
            "auth",
            "2026-05-20T10:00:00Z",
            "qa.result",
            json!({ "passed": 5, "total": 7, "failed": 2 }),
        )];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.ac_total, 7);
        assert_eq!(view.ac_passed, 5);
        assert_eq!(view.ac_failed, 2);
    }

    #[test]
    fn tool_use_and_agent_start_bump_counters() {
        let events = vec![
            event("auth", "2026-05-20T10:00:00Z", "tool.use", json!({})),
            event("auth", "2026-05-20T10:00:01Z", "tool.use", json!({})),
            event("auth", "2026-05-20T10:00:02Z", "agent.start", json!({})),
        ];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.tools_used, 2);
        assert_eq!(view.agents_dispatched, 1);
    }

    #[test]
    fn duration_is_diff_between_first_and_last_event() {
        let events = vec![
            event("auth", "2026-05-20T10:00:00Z", "tool.use", json!({})),
            event("auth", "2026-05-20T10:00:30Z", "tool.use", json!({})),
        ];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.started_at.as_deref(), Some("2026-05-20T10:00:00Z"));
        assert_eq!(view.last_event_at.as_deref(), Some("2026-05-20T10:00:30Z"));
        assert_eq!(view.duration_ms, Some(30_000));
    }

    #[test]
    fn pipeline_complete_does_not_clobber_explicit_status() {
        // `pipeline.complete` is a temporal audit marker, not a status
        // transition. With only this event the status stays at NoEvents —
        // the authoritative source is `pipeline.status`.
        let events = vec![event(
            "auth",
            "2026-05-20T10:00:00Z",
            "pipeline.complete",
            json!({ "closedAt": "2026-05-20T10:00:00Z" }),
        )];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.status, SpecStatus::NoEvents);
    }

    #[test]
    fn mark_followup_pair_leaves_status_at_closed_followup() {
        // Mirrors `complete_spec::mark_followup`: pipeline.status:
        // closed-followup followed by pipeline.complete. The status must
        // remain ClosedFollowup so the dashboard's Follow-up bucket sees it.
        let events = vec![
            event(
                "feature-x",
                "2026-05-20T10:00:00Z",
                "pipeline.status",
                json!({ "from": "implementing", "to": "closed-followup" }),
            ),
            event(
                "feature-x",
                "2026-05-20T10:00:00.500Z",
                "pipeline.complete",
                json!({ "closedAt": "2026-05-20T10:00:00.500Z", "affectedFiles": [] }),
            ),
        ];
        let view = project_spec_view("feature-x", &events);
        assert_eq!(view.status, SpecStatus::ClosedFollowup);
    }

    #[test]
    fn pipeline_status_completed_after_followup_archives_to_completed() {
        // Stage-2 archive path: `pipeline.complete` lands first (during
        // mark_followup), then `pipeline.status: completed` arrives when
        // `archive()` runs. The later status event wins, as it should.
        let events = vec![
            event(
                "feature-x",
                "2026-05-20T10:00:00Z",
                "pipeline.status",
                json!({ "to": "closed-followup" }),
            ),
            event(
                "feature-x",
                "2026-05-20T10:00:00.500Z",
                "pipeline.complete",
                json!({ "closedAt": "2026-05-20T10:00:00.500Z" }),
            ),
            event(
                "feature-x",
                "2026-05-21T10:00:00Z",
                "pipeline.status",
                json!({ "to": "completed" }),
            ),
        ];
        let view = project_spec_view("feature-x", &events);
        assert_eq!(view.status, SpecStatus::Completed);
    }

    #[test]
    fn wave_failed_marks_status_and_records_wave() {
        let events = vec![event(
            "auth",
            "2026-05-20T10:00:00Z",
            "pipeline.wave.failed",
            json!({ "wave": 3, "reason": "build-broken" }),
        )];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.status, SpecStatus::WaveFailed);
        assert_eq!(view.failed_waves, vec![3]);
    }

    // ---------------------------------------------------------------------------
    // Header fallback (Wave 1 of 2026-05-21-flatten-spec-layout-and-multi-collab)
    // ---------------------------------------------------------------------------

    /// In-memory [`EventSink`] for the synthetic-emit assertions.
    struct CapturingSink {
        collected: RefCell<Vec<HarnessEvent>>,
    }

    impl CapturingSink {
        fn new() -> Self {
            Self {
                collected: RefCell::new(Vec::new()),
            }
        }
    }

    impl EventSink for CapturingSink {
        fn append(&self, ev: &HarnessEvent) -> CoreResult<()> {
            self.collected.borrow_mut().push(ev.clone());
            Ok(())
        }
    }

    /// Write `body` to `path.join(file_name)` and return the full path.
    fn write_spec_md(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
        let path = dir.join("spec.md");
        crate::fs::write_atomic(&path, body.as_bytes()).expect("temp spec.md write");
        path
    }

    #[test]
    fn project_spec_view_falls_back_to_header_when_events_empty() {
        // No events at all — the header is the only signal.
        let tmp = tempfile::tempdir().unwrap();
        let path = write_spec_md(
            tmp.path(),
            "# Achatamento\n\n### Status: completed\n### Phase: close\n### Scope: full\n### Lang: pt\n\n## Resumo\n…",
        );
        let view = project_spec_view_with_header("flatten", &[], Some(path.as_path()), None);
        assert_eq!(view.spec, "flatten");
        assert_eq!(view.status, SpecStatus::Completed);
        assert_eq!(view.phase, Some(Phase::Close));
        assert_eq!(view.scope, Some(Scope::Full));
        assert_eq!(view.lang.as_deref(), Some("pt"));
        // Header alone cannot prove WHEN work happened.
        assert!(view.started_at.is_none());
        assert!(view.last_event_at.is_none());
    }

    #[test]
    fn project_spec_view_prefers_events_over_header() {
        // Header says `completed` but the event log says `implementing` — the
        // event log wins because it is the per-machine source of truth.
        let tmp = tempfile::tempdir().unwrap();
        let path = write_spec_md(
            tmp.path(),
            "# Auth\n\n### Status: completed\n### Phase: close\n",
        );
        let events = vec![event(
            "auth",
            "2026-05-20T10:00:00Z",
            "pipeline.status",
            json!({ "to": "implementing" }),
        )];
        let view = project_spec_view_with_header("auth", &events, Some(path.as_path()), None);
        assert_eq!(view.status, SpecStatus::Implementing);
        // Timestamps come from the event row, not the header.
        assert_eq!(view.started_at.as_deref(), Some("2026-05-20T10:00:00Z"));
    }

    #[test]
    fn project_spec_view_handles_missing_header_file() {
        // Events empty AND the supplied `spec.md` path does not exist → the
        // fallback degrades to the empty view rather than panicking.
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist").join("spec.md");
        let view =
            project_spec_view_with_header("ghost", &[], Some(missing.as_path()), None);
        assert_eq!(view.spec, "ghost");
        assert_eq!(view.status, SpecStatus::NoEvents);
        assert!(view.phase.is_none());
    }

    #[test]
    fn header_fallback_emits_synthetic_pipeline_status_when_sink_supplied() {
        // Wave 5 backfill hook: the optional sink receives one synthetic
        // event so the next read can fold from SQLite directly.
        let tmp = tempfile::tempdir().unwrap();
        let path = write_spec_md(
            tmp.path(),
            "# X\n\n### Status: completed\n",
        );
        let sink = CapturingSink::new();
        let view =
            project_spec_view_with_header("x", &[], Some(path.as_path()), Some(&sink));
        assert_eq!(view.status, SpecStatus::Completed);
        let captured = sink.collected.borrow();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].event, "pipeline.status");
        assert_eq!(captured[0].spec.as_deref(), Some("x"));
        assert_eq!(
            captured[0]
                .payload
                .get("to")
                .and_then(serde_json::Value::as_str),
            Some("completed")
        );
    }

    #[test]
    fn header_fallback_skips_synthetic_emit_when_events_present() {
        // The sink must NOT be called when the event log already wins — only
        // the fallback path may emit.
        let tmp = tempfile::tempdir().unwrap();
        let path = write_spec_md(tmp.path(), "# X\n\n### Status: completed\n");
        let sink = CapturingSink::new();
        let events = vec![event(
            "x",
            "2026-05-20T10:00:00Z",
            "pipeline.status",
            json!({ "to": "implementing" }),
        )];
        let view = project_spec_view_with_header("x", &events, Some(path.as_path()), Some(&sink));
        assert_eq!(view.status, SpecStatus::Implementing);
        assert!(sink.collected.borrow().is_empty(), "no synthetic emit on event-log win");
    }

    #[test]
    fn header_fallback_returns_empty_when_path_is_none() {
        // The opt-in shape: without a path the fallback is fully disabled.
        let view = project_spec_view_with_header("nobody", &[], None, None);
        assert_eq!(view.status, SpecStatus::NoEvents);
    }
}
