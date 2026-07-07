//! Shared decay math for the knowledge subsystem.
//!
//! A knowledge record's *worth* is not static: a high-confidence note captured
//! months ago and never re-used is a weaker bet than a fresh one. The decay
//! function below is the **single source of truth** for that arithmetic —
//! `memory search` consults THIS one function so the curve never diverges
//! (SOLID). It is pure and deterministic *given the clock*: the caller reads
//! "now" once and passes it in, so a test can pin the timestamp and the result
//! is byte-stable.

pub mod memory;

/// Days over which a record's confidence linearly decays to zero, measured from
/// its reference timestamp (`last_used` when the legacy agent store tracks it,
/// else `captured_at`). The single decay window for the whole knowledge
/// subsystem — `memory` derives from it.
pub const DECAY_WINDOW_DAYS: f64 = 30.0;

/// The *effective* confidence of a record: its stored `confidence` linearly
/// attenuated by how stale its reference timestamp `ts` is, relative to `now`.
///
/// - At `ts == now` the factor is `1.0` (no decay) → the full confidence.
/// - At `now - ts == DECAY_WINDOW_DAYS` the factor is `0.0` → confidence decays
///   to zero; further age stays clamped at zero (never negative).
/// - A missing/unparseable `ts` or `now` fails open to the un-decayed
///   `confidence` (clamped to `[0,1]`) — absence of a timestamp must never
///   *penalise* a record, only the explicit passage of time does.
///
/// Pure + deterministic given `now`: identical inputs always yield identical
/// output, so callers that read the wall clock once (and tests that pin it) get
/// a stable ordering. THE one decay curve — `memory` reuses it.
#[must_use]
pub fn effective_confidence(confidence: f64, ts: Option<&str>, now_iso: &str) -> f64 {
    let clamped = confidence.clamp(0.0, 1.0);
    let Some(ref_ms) =
        ts.and_then(|s| mustard_core::time::parse_iso_millis(s).map(|ms| ms / 1000))
    else {
        return clamped;
    };
    let Some(now) = mustard_core::time::parse_iso_millis(now_iso).map(|ms| ms / 1000) else {
        return clamped;
    };
    let days = ((now - ref_ms) as f64) / 86_400.0;
    let factor = 1.0 - (days / DECAY_WINDOW_DAYS);
    (clamped * factor.max(0.0)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_record_keeps_full_confidence() {
        // One hour of age over a 30-day window → factor ≈ 1.
        let now = "2026-06-15T01:00:00.000Z";
        let ts = "2026-06-15T00:00:00.000Z";
        let eff = effective_confidence(0.9, Some(ts), now);
        assert!(eff > 0.89, "fresh record barely decays: {eff}");
    }

    #[test]
    fn old_record_decays_toward_zero() {
        // 30 days of age → factor 0 → effective confidence 0.
        let now = "2026-07-15T00:00:00.000Z";
        let ts = "2026-06-15T00:00:00.000Z";
        let eff = effective_confidence(0.9, Some(ts), now);
        assert!(eff < 0.01, "a full-window-old record decays to ~0: {eff}");
    }

    #[test]
    fn beyond_window_clamps_at_zero_never_negative() {
        let now = "2026-09-15T00:00:00.000Z"; // ~92 days
        let ts = "2026-06-15T00:00:00.000Z";
        assert_eq!(effective_confidence(0.9, Some(ts), now), 0.0);
    }

    #[test]
    fn missing_timestamp_fails_open_to_undecayed() {
        // No reference timestamp → the record is not penalised for time.
        assert!((effective_confidence(0.7, None, "2026-06-15T00:00:00.000Z") - 0.7).abs() < 1e-9);
        // An unparseable timestamp behaves the same.
        assert!(
            (effective_confidence(0.7, Some("nope"), "2026-06-15T00:00:00.000Z") - 0.7).abs() < 1e-9
        );
    }

    #[test]
    fn confidence_is_clamped_into_unit_range() {
        let now = "2026-06-15T00:00:00.000Z";
        assert_eq!(effective_confidence(9.0, None, now), 1.0);
        assert_eq!(effective_confidence(-3.0, None, now), 0.0);
    }

    #[test]
    fn deterministic_given_the_clock() {
        let now = "2026-06-20T00:00:00.000Z";
        let ts = "2026-06-15T00:00:00.000Z";
        let a = effective_confidence(0.8, Some(ts), now);
        let b = effective_confidence(0.8, Some(ts), now);
        assert_eq!(a, b, "same inputs → identical output");
    }
}
