//! Tiny date/time helpers shared by the W3 adapters.
//!
//! Each external-cost adapter (`otel`, `transcript`, `rtk`) needs to stamp
//! its records with an ISO-8601 wall-clock timestamp. Doing that without
//! pulling in `jiff` or `chrono` (both are too large for a hook's allotted
//! milliseconds) means re-deriving the calendar from a UNIX epoch via
//! Howard Hinnant's days-from-civil algorithm. The helper used to live
//! duplicated verbatim in all three adapters — this module is the single
//! source of truth so a clock or calendar bug only has to be fixed once.
//!
//! `now_iso` is `pub` so other crates (e.g. `mustard-rt`) can stamp savings
//! records with the same ISO-8601 format and the same Howard Hinnant
//! roundtrip the economy adapters use; `epoch_secs_to_ymdhms` stays
//! `pub(super)` since it is a calendar primitive only the local adapters
//! need.

/// Now, formatted ISO-8601 to second precision (UTC).
///
/// Falls back to the UNIX epoch (`1970-01-01T00:00:00Z`) when the system
/// clock is unset or runs before the epoch — the adapters treat that as a
/// recoverable degradation (the record still lands, it just sorts at the
/// bottom).
#[must_use]
pub fn now_iso() -> String {
    // cast_possible_wrap: UNIX epoch seconds fit comfortably in i64 until year 2262.
    #[allow(clippy::cast_possible_wrap)]
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let (y, mo, d, h, mi, s) = epoch_secs_to_ymdhms(now);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Howard Hinnant's days-from-civil algorithm, in reverse. Returns
/// `(year, month, day, hour, minute, second)` in UTC.
///
/// The algorithm is the same one `writer::iso_to_epoch_ms` uses in the
/// forward direction; pairing them avoids drift between ingest and
/// roundtrip queries.
// cast_sign_loss: Howard Hinnant's algorithm guarantees calendar values are non-negative.
// many_single_char_names: single-char names are idiomatic for this well-known algorithm.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::many_single_char_names)]
pub(super) fn epoch_secs_to_ymdhms(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
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
    (y, m, d, h, mi, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_zero_is_unix_epoch() {
        let (y, mo, d, h, mi, s) = epoch_secs_to_ymdhms(0);
        assert_eq!((y, mo, d, h, mi, s), (1970, 1, 1, 0, 0, 0));
    }

    #[test]
    fn now_iso_is_iso8601_shape() {
        let s = now_iso();
        // YYYY-MM-DDTHH:MM:SSZ is 20 chars.
        assert_eq!(s.len(), 20);
        assert!(s.ends_with('Z'));
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[10..11], "T");
    }
}
