//! Shared, dependency-free helpers used across the enforcement modules.
//!
//! ## Why a `util` module inside `mustard-rt`
//!
//! Before this module, each hook module carried its own verbatim copy of
//! `now_iso8601` (8 copies) and `format_gate_message` (6 copies). The ideal
//! home is a `mustard-core` helper, but the core crate was out of reach when
//! the copies were joined. This module is that resolution: one copy inside
//! the binary crate, shared by every hook module. It is `mustard-rt`-local — it does not
//! touch `mustard-core`.

pub mod platform;
// The SHA-256 lives in `mustard-core`, where the build scripts and the
// measurement proof share it; the doctor reaches it by this name.
pub use mustard_core::io::sha256;

use std::fmt::Write as _;

// Timestamp helpers (`now_iso8601`, `now_unix_millis`) live in the single
// canonical home `mustard_core::time`, and the user's home folder in
// `mustard_core::platform::harness::home_dir` — call them directly, no
// rt-side alias.

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
        let _ = write!(msg, " Saída: {tail}");
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

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
