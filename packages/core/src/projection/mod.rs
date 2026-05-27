//! Pure folds over `&[HarnessEvent]` — one function per `ViewModel`.
//!
//! Every projection here is total (always returns *something*) and
//! deterministic (same input → same output). They take a slice of events and
//! return a typed view; they never read the filesystem, never touch the
//! event store, and never panic.
//!
//! This is what makes the crate testable without IO: a test seeds a `Vec`,
//! calls the projection, asserts the view. Production callers in `apps/rt`
//! and `apps/dashboard` supply the slice via
//! [`read_workspace_events`] (NDJSON walker) before invoking the projection.

mod card;
mod quality;
mod timeline;
mod waves;
mod workspace;

pub use card::{project_spec_view, project_spec_view_with_header};
pub use quality::project_quality;
pub use timeline::{
    project_timeline, project_timeline_from_ndjson_dir, read_harness_events_from_ndjson_dir,
};
pub use waves::project_waves;
pub use workspace::project_workspace;

use crate::claude_paths::ClaudePaths;
use crate::events::{Event, EventReader};
use crate::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use crate::model::view::Phase;
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
pub fn ndjson_to_harness(e: Event) -> HarnessEvent {
    let raw = &e.raw;
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
        payload: e.payload,
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
/// W8A-2 (no-sqlite Wave 8): the single canonical event-slice loader for the
/// crate. Both `apps/rt` and the dashboard Tauri layer consume this — keeping
/// it in `mustard-core` keeps the projection inputs identical across the two
/// consumers (the regression W6 caught when the dashboard had its own copy).
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

/// Difference between two ISO-8601 timestamps in milliseconds. Returns `None`
/// if either side fails to parse — used to compute durations without a
/// dedicated date library in this crate. Pre-1970 timestamps clamp to zero.
#[must_use]
pub fn iso_diff_ms(start_iso: &str, end_iso: &str) -> Option<i64> {
    let start = parse_iso_millis(start_iso)?;
    let end = parse_iso_millis(end_iso)?;
    Some(end.saturating_sub(start))
}

/// Parse the `YYYY-MM-DDThh:mm:ss[.fff]Z` prefix into epoch milliseconds.
///
/// Conservative: only the seconds part is required; everything after a
/// trailing `.fff` is ignored. This is the same algorithm used by
/// `apps/rt/src/hooks/tracker.rs::parse_iso_millis` — kept inline to avoid
/// pulling jiff into a domain crate.
#[must_use]
pub fn parse_iso_millis(iso: &str) -> Option<i64> {
    let bytes = iso.as_bytes();
    if bytes.len() < 19 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    let num = |s: &str| -> Option<i64> { s.parse().ok() };
    let year = num(&iso[0..4])?;
    let month = num(&iso[5..7])?;
    let day = num(&iso[8..10])?;
    let hh = num(&iso[11..13])?;
    let mm = num(&iso[14..16])?;
    let ss = num(&iso[17..19])?;

    // Howard Hinnant's days_from_civil — same routine the tracker hook uses,
    // copied here to keep the projection crate dependency-free.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + hh * 3600 + mm * 60 + ss;
    Some(secs.max(0).saturating_mul(1000))
}

#[cfg(test)]
mod tests {
    use super::*;

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
