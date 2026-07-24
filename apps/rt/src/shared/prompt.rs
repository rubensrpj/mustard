//! `prompt` — telling a person's prompt apart from the runtime's own notices.
//!
//! ## Why this exists
//!
//! `UserPromptSubmit` fires for everything that reaches the session through the
//! user channel — and the runtime speaks through that same channel. A finished
//! background command, a completed subagent and other lifecycle notices all
//! arrive as a "user prompt", carrying a machine banner and, in the subagent
//! case, an entire report.
//!
//! Every observer on that trigger therefore has the same blind spot, and each
//! one paid for it differently:
//!
//! - [`crate::hooks::observe::change_request_log`] recorded them as mid-pipeline
//!   change requests. Measured in the field: one unit's `change-log.md` reached
//!   20 KB over six entries, of which exactly one — 57 characters — was a person
//!   asking for something. Both that file and its NDJSON twin are versioned.
//! - [`crate::hooks::observe::prompt_observer`] emits `user.prompt` for EVERY
//!   prompt by design, so each notice landed in the per-spec event log as
//!   something the user said. That log is the source `metrics collect` reads,
//!   so the noise reaches the instruments too.
//! - [`crate::hooks::observe::amend_window_inject`] read them as amendment
//!   intent whenever a post-close window happened to be open.
//!
//! One predicate, one owner: three copies of a rule like this drift the first
//! time the runtime adds a banner.
//!
//! ## Contract
//!
//! Pure, total, allocation-free, never panics. The markers are runtime protocol
//! strings, not user-facing prose, so they stay English regardless of the
//! project's narrative locale — the same carve-out that keeps event names and
//! log strings English.

/// Prompt markers the RUNTIME authors, never a person.
///
/// Matched anywhere in the text rather than at its start: the runtime may
/// prepend its own framing before the banner, and a notice is sometimes wrapped
/// in a wider envelope.
const HARNESS_NOTICE_MARKERS: &[&str] =
    &["[SYSTEM NOTIFICATION - NOT USER INPUT]", "<task-notification>"];

/// `true` when `prompt` was authored by the runtime rather than by a person.
///
/// Deliberately narrow: it recognises the two banners the runtime actually
/// emits and nothing else. A heuristic ("looks machine-generated") would
/// eventually swallow a real request, and losing what someone asked for is the
/// worse error — an observer that records one notice too many is noisy, one
/// that drops a request is silently wrong.
#[must_use]
pub fn is_harness_notice(prompt: &str) -> bool {
    HARNESS_NOTICE_MARKERS.iter().any(|m| prompt.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both banners are recognised, wherever they sit in the text.
    #[test]
    fn recognises_every_runtime_banner() {
        assert!(is_harness_notice("[SYSTEM NOTIFICATION - NOT USER INPUT]\nbody"));
        assert!(is_harness_notice("<task-notification>\n<status>completed</status>"));
        // Wrapped in a wider envelope — still a notice.
        assert!(is_harness_notice("prefix\n\n<task-notification>x</task-notification>\n"));
    }

    /// A person's prompt is never a notice, including one that TALKS about
    /// notifications. Substring matching is on the literal banner, not the word.
    #[test]
    fn a_real_request_is_never_mistaken_for_one() {
        for real in [
            "muda o campo status para enum",
            "troca o gate para warn",
            "why did the task notification arrive twice?",
            "add a system notification banner to the UI",
            "",
        ] {
            assert!(!is_harness_notice(real), "misread as a notice: {real:?}");
        }
    }
}
