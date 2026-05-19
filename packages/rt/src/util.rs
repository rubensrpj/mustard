//! Shared, dependency-free helpers used across the enforcement modules.
//!
//! ## Why a `util` module inside `mustard-rt`
//!
//! Through Waves 1-4 each module carried its own verbatim copy of
//! `now_iso8601` (8 copies) and `format_gate_message` (6 copies) — the spec
//! Concern "`now_iso8601` / `format_gate_message` duplication". The ideal home
//! is a `mustard-core` helper, but b2 (`mustard-core`) is out of bounds for
//! b3. This module is the in-bounds resolution: one copy inside the binary
//! crate, shared by every hook module. It is `mustard-rt`-local — it does not
//! touch `mustard-core`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// An RFC-3339 / ISO-8601 UTC timestamp string (`YYYY-MM-DDThh:mm:ss.sssZ`),
/// matching JavaScript `new Date().toISOString()`.
///
/// Uses Howard Hinnant's `civil_from_days` algorithm so it has no calendar
/// crate dependency.
#[must_use]
pub fn now_iso8601() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}.{millis:03}Z")
}

/// Assemble a gate message in the `formatGateMessage` shape:
/// `[gate] what. why. Saída: exit.`
///
/// Shared shape with the JS `_lib/gate-message.js`. Empty `what` / `why` /
/// `exit` are skipped; the body and tail are terminated with `.` when they do
/// not already end in sentence punctuation.
#[must_use]
pub fn format_gate_message(gate: &str, what: &str, why: &str, exit: &str) -> String {
    let mut body = String::new();
    if !what.is_empty() {
        body.push_str(what);
    }
    if !why.is_empty() {
        if !body.is_empty() {
            body.push_str(". ");
        }
        body.push_str(why);
    }
    if !body.is_empty() && !body.ends_with(['.', '!', '?', '…']) {
        body.push('.');
    }
    let mut msg = format!("[{gate}] {body}").trim().to_string();
    if !exit.is_empty() {
        let mut tail = exit.to_string();
        if !tail.ends_with(['.', '!', '?', '…']) {
            tail.push('.');
        }
        msg.push_str(&format!(" Saída: {tail}"));
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso8601_has_the_expected_shape() {
        let ts = now_iso8601();
        // `YYYY-MM-DDThh:mm:ss.sssZ` — 24 chars.
        assert_eq!(ts.len(), 24, "{ts}");
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[10..11], "T");
    }

    #[test]
    fn gate_message_assembles_all_parts() {
        let msg = format_gate_message("Gate", "did a thing", "because reasons", "do this");
        assert_eq!(msg, "[Gate] did a thing. because reasons. Saída: do this.");
    }

    #[test]
    fn gate_message_skips_empty_parts() {
        assert_eq!(format_gate_message("G", "what", "", ""), "[G] what.");
        assert_eq!(format_gate_message("G", "", "", ""), "[G]");
    }
}
