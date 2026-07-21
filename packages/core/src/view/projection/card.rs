//! [`project_spec_view`] — fold the event stream for one spec into a
//! [`SpecView`].
//!
//! Per the dashboard audit (2026-05-20), the dashboard's `spec_views.rs`
//! returned literal `"unknown"` whenever the SQL fallback fired. This fold
//! replaces that path with a deterministic projection over typed events:
//!
//! - `pipeline.scope` populates `scope`, `lang`, `model`, `is_wave_plan`,
//!   `total_waves`.
//! - `pipeline.status` transitions `state` (the legacy status vocabulary is
//!   folded straight into a [`SpecState`] via `state_from_status_word`).
//! - `pipeline.phase` updates `phase`.
//! - `pipeline.task.complete` accumulates `files_touched` (deduplicated).
//! - `pipeline.wave.complete` extends `completed_waves` and recomputes
//!   `current_wave`.
//! - `qa.result` overwrites `ac_passed`/`ac_total`/`ac_failed` from the
//!   latest event (newer wins; folds in chronological order).
//! - `tool.use` bumps `tools_used`.
//! - `agent.start` bumps `agents_dispatched`.
//!
//! Events with `spec != Some(spec_name)` are filtered out before the fold —
//! callers that pre-filtered (e.g. `store.query(Some(name))`) pay zero cost.
//!
//! ## Sidecar / header fallback (Wave 1, 2026-05-21)
//!
//! When the event stream is empty and a `spec.md` path is supplied, the fold
//! seeds the [`SpecView`] from the filesystem. **`meta.json` is the single
//! source of truth**: the sidecar beside the spec is read first
//! (`stage` / `outcome` / `phase` / `scope` / `lang`); only an un-migrated spec
//! without a sidecar falls back to the legacy `### Status:` / `### Phase:` /
//! `### Scope:` / `### Lang:` header lines. This is the cross-collaborator path:
//! the event log is per-machine and never versioned, but `meta.json` (and, for
//! legacy specs, the header) *is* checked into git. A teammate who pulls the
//! repo sees a populated dashboard without re-emitting any events.
//!
//! The fallback is opt-in (`spec_md_path = None` disables it). Timestamps stay
//! `None` because the header alone cannot prove when work started or last
//! happened.

use crate::domain::model::view::{Flags, Outcome, Phase, Scope, SpecState, SpecView, Stage};
use crate::domain::model::event::{
    HarnessEvent, PipelineScopePayload, PipelineTaskCompletePayload, PipelineWaveCompletePayload,
};
use std::collections::BTreeSet;
use std::path::Path;

use super::extract_to_phase;
use crate::platform::time::iso_diff_ms;

/// Fold `events` into a [`SpecView`] for `spec_name`.
///
/// Thin wrapper around [`project_spec_view_with_header`] that disables the
/// header fallback. Kept for callers (workspace projection, tests) that have
/// no `spec.md` path to resolve and never want the synthetic-emit side effect.
#[must_use]
pub fn project_spec_view(spec_name: &str, events: &[HarnessEvent]) -> SpecView {
    project_spec_view_with_header(spec_name, events, None)
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
/// When `events` is non-empty the path is ignored — the event log is
/// authoritative.
///
/// W8A-4 drop: the optional `emit_sink` parameter (a Wave 5 SQLite backfill
/// hook for the legacy `EventSink`) is gone. With the NDJSON-only store,
/// header-derived state is computed on demand by every reader and there is
/// no second log to seed.
#[must_use]
pub fn project_spec_view_with_header(
    spec_name: &str,
    events: &[HarnessEvent],
    spec_md_path: Option<&Path>,
) -> SpecView {
    // Event stream wins whenever it has anything for this spec. Filter first
    // so a stream full of other-spec noise still triggers the fallback.
    let scoped_count = events
        .iter()
        .filter(|e| e.spec.as_deref() == Some(spec_name))
        .count();
    if scoped_count == 0 {
        if let Some(path) = spec_md_path {
            if let Some(view) = view_from_header(spec_name, path) {
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
            "pipeline.wave.start" => apply_wave_start(&mut view),
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

    view
}

/// Map a legacy `pipeline.status` word (`"implementing"`, `"draft"`,
/// `"closed-followup"`, …) straight onto the canonical [`SpecState`] — the
/// same vocabulary the retired flat `SpecStatus::parse` accepted, folded into
/// `(stage, outcome, flags)` in one step. Returns `None` for unknown words so
/// the fold leaves the state unchanged (fail-open).
fn state_from_status_word(raw: &str) -> Option<SpecState> {
    let (stage, outcome, flags) = match raw.trim().to_ascii_lowercase().as_str() {
        "no-events" | "planning" | "draft" | "approved" => {
            (Stage::Plan, Outcome::Active, Flags::default())
        }
        "implementing" | "in-progress" | "in_progress" => {
            (Stage::Execute, Outcome::Active, Flags::default())
        }
        "reviewing" | "qa" => (Stage::QaReview, Outcome::Active, Flags::default()),
        "closed-followup" | "closed_followup" => (
            Stage::Close,
            Outcome::Active,
            Flags { followup_open: true, ..Flags::default() },
        ),
        "completed" | "closed" => (Stage::Close, Outcome::Completed, Flags::default()),
        "cancelled" | "canceled" | "superseded" => {
            (Stage::Close, Outcome::Cancelled, Flags::default())
        }
        "abandoned" | "orphan" => (Stage::Close, Outcome::Abandoned, Flags::default()),
        "blocked" | "paused" => (
            Stage::Plan,
            Outcome::Active,
            Flags { blocked: true, ..Flags::default() },
        ),
        "wave-failed" | "wave_failed" => (
            Stage::Execute,
            Outcome::Active,
            Flags { wave_failed: true, ..Flags::default() },
        ),
        _ => return None,
    };
    Some(SpecState::new(stage, outcome, flags).unwrap_or(SpecState {
        stage: Stage::Plan,
        outcome: Outcome::Active,
        flags: Flags::default(),
    }))
}

/// `pipeline.scope` — first observation of a spec's metadata. Records
/// scope/lang/model/waves; state transitions happen via `pipeline.status`.
fn apply_scope(view: &mut SpecView, ev: &HarnessEvent) {
    if let Ok(payload) = serde_json::from_value::<PipelineScopePayload>(ev.payload.clone()) {
        view.scope = Scope::parse(&payload.scope);
        view.lang = payload.lang;
        view.model = payload.model;
        view.is_wave_plan = payload.is_wave_plan.unwrap_or(false);
        view.total_waves = payload.total_waves;
    }
}

/// `pipeline.status` — typed transitions. Unknown strings leave the state
/// unchanged rather than dropping back to the empty default.
fn apply_status(view: &mut SpecView, ev: &HarnessEvent) {
    let Some(to) = ev.payload.get("to").and_then(serde_json::Value::as_str) else {
        return;
    };
    if let Some(state) = state_from_status_word(to) {
        view.state = state;
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

/// `pipeline.wave.start` — a wave began executing, so the spec has entered
/// EXECUTE. Advance `view.state` to `Execute` **forward-only**: only from
/// `Analyze`/`Plan`, never regressing a spec already in `QaReview`/`Close`.
/// Without this the event fold sat at `Plan` (from the last `pipeline.status:
/// approved`) through the whole first wave — the projection twin of the parent
/// `meta.json` gap that `emit_wave_start` / `sync_parent_started` closes.
fn apply_wave_start(view: &mut SpecView) {
    if !matches!(view.state.stage, Stage::Analyze | Stage::Plan) {
        return; // already Execute or later — never regress.
    }
    if let Ok(state) = SpecState::new(Stage::Execute, view.state.outcome, view.state.flags.clone()) {
        view.state = state;
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
    if let Ok(state) = SpecState::new(
        Stage::Execute,
        Outcome::Active,
        Flags { wave_failed: true, ..Flags::default() },
    ) {
        view.state = state;
    }
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
/// Seed a [`SpecView`] from the `meta.json` sidecar beside `path` — the single
/// source of truth for lifecycle metadata, including the qualifier
/// [`Flags`](crate::Flags) carried in `meta.json#flags`. Returns `None` when the
/// sidecar is absent / unparseable or carries no usable `stage` (so the caller
/// falls back to the legacy `.md` header). `started_at` / `last_event_at` stay
/// `None` — the sidecar is not evidence of *when* work happened.
fn view_from_meta(spec_name: &str, path: &Path) -> Option<SpecView> {
    let meta = crate::domain::meta::read_meta_beside(path)?;
    let stage = Stage::parse(meta.stage.as_deref()?)?;
    let outcome = meta
        .outcome
        .as_deref()
        .and_then(Outcome::parse)
        .unwrap_or(Outcome::Active);
    // Qualifier flags come from `meta.json#flags` (the canonical home since the
    // sidecar gained a `flags` field). If the resulting `(stage, outcome,
    // flags)` triple is illegal — e.g. a stale sidecar pairs `wave_failed` with
    // a non-Execute stage — fall back to the all-false flags so the read still
    // yields a legal state rather than dropping the spec.
    let flags: Flags = meta.flags.clone().into();
    let state = SpecState::new(stage, outcome, flags)
        .or_else(|_| SpecState::new(stage, outcome, Flags::default()))
        .ok()?;

    let mut view = SpecView::empty(spec_name);
    view.state = state;
    if let Some(phase) = meta.phase.as_deref().and_then(Phase::parse) {
        view.phase = Some(phase);
    }
    if let Some(scope) = meta.scope.as_deref().and_then(Scope::parse) {
        view.scope = Some(scope);
    }
    if let Some(lang) = meta.lang.filter(|s| !s.trim().is_empty()) {
        view.lang = Some(lang.trim().to_string());
    }
    Some(view)
}

fn view_from_header(spec_name: &str, path: &Path) -> Option<SpecView> {
    // `meta.json` is the single source of truth. Prefer the sidecar beside the
    // spec; fall back to the legacy in-`.md` header only for un-migrated specs.
    if let Some(view) = view_from_meta(spec_name, path) {
        return Some(view);
    }
    let raw = crate::io::fs::read_to_string(path).ok()?;
    let header = parse_header_fields(&raw);
    if header.is_empty() {
        return None;
    }

    let mut view = SpecView::empty(spec_name);

    // New canonical header (`### Stage:` / `### Outcome:` / `### Flags:`) takes
    // precedence when a `### Stage:` line is present. The legacy `### Status:`
    // / `### Phase:` block remains the fallback for specs not yet rewritten
    // (rewrite is Wave 7).
    if let Some(state) = state_from_new_header(&header) {
        view.state = state;
        if let Some(phase_raw) = header.get("phase") {
            if let Some(phase) = Phase::parse(phase_raw) {
                view.phase = Some(phase);
            }
        }
        seed_header_metadata(&mut view, &header);
        return Some(view);
    }

    if let Some(status_raw) = header.get("status") {
        if let Some(state) = state_from_status_word(status_raw) {
            view.state = state;
        }
    }
    if let Some(phase_raw) = header.get("phase") {
        if let Some(phase) = Phase::parse(phase_raw) {
            view.phase = Some(phase);
        }
    }
    seed_header_metadata(&mut view, &header);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::event::{Actor, ActorKind, SCHEMA_VERSION};
    use crate::domain::model::view::Phase;
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
        assert_eq!(view.state.stage, Stage::Plan);
        assert_eq!(view.state.outcome, Outcome::Active);
        assert_eq!(view.state.flags, Flags::default());
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
    fn scope_event_records_metadata() {
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
        assert_eq!(view.state.stage, Stage::Plan);
        assert_eq!(view.state.outcome, Outcome::Active);
        assert_eq!(view.scope, Some(Scope::Full));
        assert_eq!(view.lang.as_deref(), Some("pt"));
        assert_eq!(view.model.as_deref(), Some("opus"));
        assert!(view.is_wave_plan);
        assert_eq!(view.total_waves, Some(4));
    }

    #[test]
    fn wave_start_advances_stage_to_execute_forward_only() {
        // `approved` leaves the fold at Plan; a `wave.start` means EXECUTE began,
        // so the fold must reflect it (the projection twin of the parent-meta fix).
        let events = vec![
            event("feat", "2026-05-20T10:00:00Z", "pipeline.status", json!({ "to": "approved" })),
            event("feat", "2026-05-20T10:01:00Z", "pipeline.wave.start", json!({ "wave": 1 })),
        ];
        assert_eq!(project_spec_view("feat", &events).state.stage, Stage::Execute);

        // Forward-only: a wave.start must never regress a later stage.
        let events = vec![
            event("feat", "2026-05-20T10:00:00Z", "pipeline.status", json!({ "to": "reviewing" })),
            event("feat", "2026-05-20T10:01:00Z", "pipeline.wave.start", json!({ "wave": 2 })),
        ];
        assert_eq!(project_spec_view("feat", &events).state.stage, Stage::QaReview);
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
        assert_eq!(view.state.stage, Stage::Close);
        assert_eq!(view.state.outcome, Outcome::Completed);
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
        // transition. With only this event the state stays at the Plan+Active
        // default — the authoritative source is `pipeline.status`.
        let events = vec![event(
            "auth",
            "2026-05-20T10:00:00Z",
            "pipeline.complete",
            json!({ "closedAt": "2026-05-20T10:00:00Z" }),
        )];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.state.stage, Stage::Plan);
        assert_eq!(view.state.outcome, Outcome::Active);
        assert_eq!(view.state.flags, Flags::default());
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
        assert_eq!(view.state.stage, Stage::Close);
        assert_eq!(view.state.outcome, Outcome::Active);
        assert!(view.state.flags.followup_open);
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
        assert_eq!(view.state.stage, Stage::Close);
        assert_eq!(view.state.outcome, Outcome::Completed);
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
        assert_eq!(view.state.stage, Stage::Execute);
        assert!(view.state.flags.wave_failed);
        assert_eq!(view.failed_waves, vec![3]);
    }

    // ---------------------------------------------------------------------------
    // Header fallback (Wave 1 of 2026-05-21-flatten-spec-layout-and-multi-collab)
    //
    // W8A-4 (no-sqlite Wave 8) deleted the `EventSink`-backed synthetic-emit
    // hook plus its `CapturingSink` test double. Header fallback is now a
    // pure read: caller passes `Some(&path)` to opt in, gets back a typed
    // view derived from the spec.md header. No second store, nothing to
    // backfill.
    // ---------------------------------------------------------------------------

    /// Write `body` to `path.join(file_name)` and return the full path.
    fn write_spec_md(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
        let path = dir.join("spec.md");
        crate::io::fs::write_atomic(&path, body.as_bytes()).expect("temp spec.md write");
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
        let view = project_spec_view_with_header("flatten", &[], Some(path.as_path()));
        assert_eq!(view.spec, "flatten");
        assert_eq!(view.state.stage, Stage::Close);
        assert_eq!(view.state.outcome, Outcome::Completed);
        assert_eq!(view.phase, Some(Phase::Close));
        assert_eq!(view.scope, Some(Scope::Full));
        assert_eq!(view.lang.as_deref(), Some("pt"));
        // Header alone cannot prove WHEN work happened.
        assert!(view.started_at.is_none());
        assert!(view.last_event_at.is_none());
    }

    #[test]
    fn project_spec_view_prefers_meta_json_over_md_header() {
        // A header-less spec.md with a meta.json sidecar — the sidecar is the
        // single source of truth and seeds the view.
        let tmp = tempfile::tempdir().unwrap();
        let path = write_spec_md(tmp.path(), "# Achatamento\n\n## Resumo\n…");
        crate::io::fs::write_atomic(
            &tmp.path().join("meta.json"),
            br#"{"stage":"Close","outcome":"Completed","phase":"close","scope":"full","lang":"pt-BR"}"#,
        )
        .unwrap();
        let view = project_spec_view_with_header("flatten", &[], Some(path.as_path()));
        assert_eq!(view.state.stage, Stage::Close);
        assert_eq!(view.state.outcome, Outcome::Completed);
        assert_eq!(view.phase, Some(Phase::Close));
        assert_eq!(view.scope, Some(Scope::Full));
        assert_eq!(view.lang.as_deref(), Some("pt-BR"));
    }

    #[test]
    fn project_spec_view_meta_json_wins_over_legacy_header() {
        // meta.json says Execute/Active; a stale legacy header says completed.
        // The sidecar wins.
        let tmp = tempfile::tempdir().unwrap();
        let path = write_spec_md(
            tmp.path(),
            "# Auth\n\n### Status: completed\n### Phase: close\n",
        );
        crate::io::fs::write_atomic(
            &tmp.path().join("meta.json"),
            br#"{"stage":"Execute","outcome":"Active"}"#,
        )
        .unwrap();
        let view = project_spec_view_with_header("auth", &[], Some(path.as_path()));
        assert_eq!(view.state.stage, Stage::Execute);
        assert_eq!(view.state.outcome, Outcome::Active);
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
        let view = project_spec_view_with_header("auth", &events, Some(path.as_path()));
        assert_eq!(view.state.stage, Stage::Execute);
        assert_eq!(view.state.outcome, Outcome::Active);
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
            project_spec_view_with_header("ghost", &[], Some(missing.as_path()));
        assert_eq!(view.spec, "ghost");
        assert_eq!(view.state.stage, Stage::Plan);
        assert_eq!(view.state.outcome, Outcome::Active);
        assert_eq!(view.state.flags, Flags::default());
        assert!(view.phase.is_none());
    }

    #[test]
    fn header_fallback_returns_empty_when_path_is_none() {
        // The opt-in shape: without a path the fallback is fully disabled.
        let view = project_spec_view_with_header("nobody", &[], None);
        assert_eq!(view.state.stage, Stage::Plan);
        assert_eq!(view.state.outcome, Outcome::Active);
        assert_eq!(view.state.flags, Flags::default());
    }
}
