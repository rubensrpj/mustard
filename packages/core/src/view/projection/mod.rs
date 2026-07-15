//! Pure folds over `&[HarnessEvent]` — one function per `ViewModel`.
//!
//! Every projection here is total (always returns *something*) and
//! deterministic (same input → same output). They take a slice of events and
//! return a typed view; they never read the filesystem, never touch the
//! event store, and never panic.
//!
//! This is what makes the crate testable without IO: a test seeds a `Vec`,
//! calls the projection, asserts the view. Production callers in `apps/rt`
//! supply the slice via [`read_workspace_events`] (NDJSON walker); callers
//! that already hold the raw records in memory (the dashboard's parsed-events
//! cache) convert them with [`harness_events_from_values`] instead — the disk
//! walk is the caller's responsibility, never repeated here.

mod capability;
mod card;
mod quality;
mod timeline;
mod waves;
mod workspace;

pub use capability::{project_capabilities, CapabilityRollup, CapabilityState};
pub use card::{project_spec_view, project_spec_view_with_header};
pub use quality::project_quality;
pub use timeline::{
    project_timeline, project_timeline_from_ndjson_dir, read_harness_events_from_ndjson_dir,
};
pub use waves::project_waves;
pub use workspace::project_workspace;

use crate::io::claude_paths::ClaudePaths;
use crate::io::events::{Event, EventReader};
use crate::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use crate::domain::model::view::Phase;
use serde_json::Value;
use std::path::Path;

/// Convert one NDJSON [`Event`] to a [`HarnessEvent`] for use by projections.
///
/// The NDJSON record stores the event name in `raw["event"]` and the logical
/// kind in the top-level `kind` field. All other harness fields (`ts`, `spec`,
/// `wave`, `session_id`, `actor`) are present in `raw` via the flatten.
/// Unknown / missing fields default safely (fail-open).
///
/// W8A-2 (no-sqlite Wave 8): lifted from `apps/rt/src/run/event_projections.rs`
/// to the shared core so both the rt run-face and the dashboard Tauri layer
/// can fold over the same canonical event slice without duplicating the
/// converter.
#[must_use]
pub(crate) fn ndjson_to_harness(e: Event) -> HarnessEvent {
    harness_from_raw(&e.raw, e.payload)
}

/// Convert one already-loaded raw NDJSON record (the full line as a
/// [`serde_json::Value`], including its `payload` key) into a [`HarnessEvent`].
///
/// Performance spec `performance-dashboard-rotas-lentas-cache` (wave 1): the
/// dashboard keeps the parsed workspace records in an in-memory cache and must
/// feed the projections WITHOUT re-walking the disk. This is the conversion
/// entry point for events the caller already holds; [`ndjson_to_harness`]
/// remains the entry point for records streamed off disk via [`EventReader`].
/// Both share one field-extraction body so the two paths can never drift.
#[must_use]
pub(crate) fn value_to_harness(record: &Value) -> HarnessEvent {
    let payload = record.get("payload").cloned().unwrap_or(Value::Null);
    harness_from_raw(record, payload)
}

/// Convert an iterator of already-loaded raw NDJSON records into the
/// `Vec<HarnessEvent>` slice every projection folds over. The cached caller
/// (dashboard) owns the disk walk; this function only converts.
#[must_use]
pub fn harness_events_from_values<'a, I>(records: I) -> Vec<HarnessEvent>
where
    I: IntoIterator<Item = &'a Value>,
{
    records.into_iter().map(value_to_harness).collect()
}

/// Shared field-extraction body for [`ndjson_to_harness`] /
/// [`value_to_harness`]. `raw` is the record's envelope (for an [`Event`] the
/// flatten catch-all, for a loaded [`Value`] the full line — the keys read
/// here are identical in both shapes); `payload` is passed separately because
/// the [`Event`] deserializer already split it out.
fn harness_from_raw(raw: &Value, payload: Value) -> HarnessEvent {
    let get_str = |key: &str| -> String {
        raw.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    HarnessEvent {
        v: raw.get("v").and_then(Value::as_u64).unwrap_or(u64::from(SCHEMA_VERSION)) as u32,
        ts: get_str("ts"),
        session_id: get_str("session_id"),
        wave: raw.get("wave").and_then(Value::as_u64).unwrap_or(0) as u32,
        actor: Actor {
            kind: ActorKind::Hook,
            id: raw.get("actor").and_then(Value::as_str).map(str::to_string),
            actor_type: None,
        },
        event: get_str("event"),
        payload,
        spec: raw.get("spec").and_then(Value::as_str).map(str::to_string),
    }
}

/// Collect every NDJSON event from `<project>/.claude/spec/*/.events/*.ndjson`
/// into one slice of [`HarnessEvent`].
///
/// Walks every spec directory, then every `.events/` subdirectory, streaming
/// each `.ndjson` file line-by-line via [`EventReader::stream`]. Fail-open:
/// unreadable files and malformed lines are silently skipped — telemetry is
/// never load-bearing, the projection callers always render *something*.
///
/// W8A-2 (no-sqlite Wave 8): the canonical disk-walking event-slice loader.
/// `apps/rt` (one-shot CLI) consumes this directly; the dashboard Tauri layer
/// now feeds the same projections from its incremental parsed-events cache via
/// [`harness_events_from_values`] instead of re-walking the disk per command
/// (spec `performance-dashboard-rotas-lentas-cache`, wave 1). Conversion stays
/// shared (`harness_from_raw`), so the projection inputs remain identical
/// across the two consumers (the regression W6 caught when the dashboard had
/// its own copy).
#[must_use]
pub fn read_workspace_events(project_root: &Path) -> Vec<HarnessEvent> {
    let Ok(paths) = ClaudePaths::for_project(project_root) else {
        return Vec::new();
    };
    let spec_root = paths.spec_dir();
    let Ok(spec_entries) = std::fs::read_dir(&spec_root) else {
        return Vec::new();
    };

    let mut events: Vec<HarnessEvent> = Vec::new();

    for spec_entry in spec_entries.flatten() {
        let spec_dir = spec_entry.path();
        if !spec_dir.is_dir() {
            continue;
        }
        let events_dir = spec_dir.join(".events");
        let Ok(ndjson_entries) = std::fs::read_dir(&events_dir) else {
            continue;
        };
        for ndjson_entry in ndjson_entries.flatten() {
            let p = ndjson_entry.path();
            if p.extension().and_then(|x| x.to_str()) != Some("ndjson") {
                continue;
            }
            for e in EventReader::stream(&p) {
                events.push(ndjson_to_harness(e));
            }
        }
    }

    events
}

/// Tiny helper used by multiple projections — extract the canonical "to" phase
/// from a `pipeline.phase` event payload. `pipeline.phase` carries both a
/// `from` and a `to` field; only `to` is interesting for the current-phase
/// fold.
pub(crate) fn extract_to_phase(ev: &HarnessEvent) -> Option<Phase> {
    ev.payload
        .get("to")
        .and_then(serde_json::Value::as_str)
        .and_then(Phase::parse)
}


// Parsing ISO-8601 → epoch millis now lives in the single canonical home

#[cfg(test)]
mod tests {
    use super::{harness_events_from_values, ndjson_to_harness, value_to_harness};
    use crate::io::events::Event;
    use crate::platform::time::iso_diff_ms;
    use serde_json::json;

    #[test]
    fn value_to_harness_matches_disk_streamed_conversion() {
        // The cached caller path (already-loaded Value) must produce the same
        // HarnessEvent as the disk-streamed path (EventReader line → Event).
        let line = json!({
            "v": 1,
            "ts": "2026-06-10T10:00:00Z",
            "session_id": "sess-1",
            "wave": 2,
            "actor": "metrics-tracker",
            "event": "tool.use",
            "kind": "tool",
            "spec": "alpha",
            "payload": { "tool": "Read", "target": { "file_path": "src/x.rs" } }
        });
        let from_value = value_to_harness(&line);
        let event: Event = serde_json::from_value(line).expect("valid NDJSON record");
        let from_event = ndjson_to_harness(event);
        assert_eq!(from_value.ts, from_event.ts);
        assert_eq!(from_value.session_id, from_event.session_id);
        assert_eq!(from_value.wave, from_event.wave);
        assert_eq!(from_value.event, from_event.event);
        assert_eq!(from_value.spec, from_event.spec);
        assert_eq!(from_value.payload, from_event.payload);
        assert_eq!(from_value.actor.id, from_event.actor.id);
    }

    #[test]
    fn harness_events_from_values_converts_a_loaded_slice_without_io() {
        // Sparse record: missing payload/spec/wave must default safely.
        let records = vec![
            json!({ "event": "pipeline.phase", "ts": "2026-06-10T10:00:00Z",
                    "spec": "alpha", "payload": { "to": "EXECUTE" } }),
            json!({ "event": "tool.use", "ts": "2026-06-10T10:01:00Z" }),
        ];
        let events = harness_events_from_values(records.iter());
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event, "pipeline.phase");
        assert_eq!(events[0].spec.as_deref(), Some("alpha"));
        assert_eq!(events[1].event, "tool.use");
        assert!(events[1].spec.is_none());
        assert!(events[1].payload.is_null());
        assert_eq!(events[1].wave, 0);
    }

    #[test]
    fn iso_diff_handles_zero_and_one_second() {
        let a = "2026-05-20T10:00:00Z";
        let b = "2026-05-20T10:00:01Z";
        assert_eq!(iso_diff_ms(a, a), Some(0));
        assert_eq!(iso_diff_ms(a, b), Some(1000));
    }

    #[test]
    fn iso_diff_returns_none_for_malformed_input() {
        assert!(iso_diff_ms("not-iso", "2026-05-20T10:00:00Z").is_none());
        assert!(iso_diff_ms("2026-05-20T10:00:00Z", "garbage").is_none());
    }

    #[test]
    fn iso_diff_handles_day_boundary() {
        // 24 hours = 86_400_000 ms
        let start = "2026-05-20T00:00:00Z";
        let end = "2026-05-21T00:00:00Z";
        assert_eq!(iso_diff_ms(start, end), Some(86_400_000));
    }
}
