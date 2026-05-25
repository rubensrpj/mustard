//! Pure folds over `&[HarnessEvent]` — one function per `ViewModel`.
//!
//! Every projection here is total (always returns *something*) and
//! deterministic (same input → same output). They take a slice of events and
//! return a typed view; they never read the filesystem, never touch the
//! event store, and never panic.
//!
//! This is what makes the crate testable without IO: a test seeds a `Vec`,
//! calls the projection, asserts the view. The [`reader`](crate::reader)
//! layer is a thin shim that just supplies the slice from `SQLite`.

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

use crate::model::view::Phase;
use crate::model::event::HarnessEvent;

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
