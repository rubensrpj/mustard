//! Span-level verdict log for the W4 behavior-regression gate.
//!
//! Spec A v4 / W5 — `_review-spans.md` is an append-only ledger of per-child
//! verdicts emitted as each `SubagentStop` fires. Consolidation of a wave is
//! blocked when any line registers a red verdict (AC-A-7). The file lives next
//! to the wave's `spec.md` under `.claude/spec/<spec>/wave-<n>-<role>/`.
//!
//! ## Format
//!
//! One line per child, structure-only fields (machine-stable):
//!
//! ```text
//! - {verdict} | {child_id} | {iso_ts} | signals={n} | {first_message}
//! ```
//!
//! - `verdict` is the lowercase classifier (`green` / `amber` / `red`) — the
//!   exact strings emitted by [`mustard_core::ast`] / `gate_regression_check`'s
//!   JSON payloads (stable across locales).
//! - `child_id` is a short identifier supplied by the caller (typically the
//!   `subagent_type` from `tool_input` plus a counter).
//! - `iso_ts` is the wall-clock timestamp at append time.
//! - `signals` is the count of regression signals the verdict carried.
//! - `first_message` is the localised body of the first signal (rendered by
//!   `mustard_core::i18n::translate` in the gate module), trimmed and inlined
//!   so the ledger can be scanned by a human without re-running the gate.
//!
//! ## Atomicity
//!
//! The append uses `OpenOptions::append(true).create(true)` — each
//! `write_all` of a single small line is atomic at the OS level (POSIX
//! `O_APPEND`, Windows `FILE_APPEND_DATA`) when the payload fits in one
//! write. We bound the line length to `LINE_MAX_CHARS` to stay under the
//! filesystem's atomic-write threshold on every supported platform.
//!
//! ## Fail-open
//!
//! Every IO step degrades to a no-op. A missing wave directory, a permission
//! error, or a corrupt ledger line never panics — the caller's
//! `SubagentStop` flow continues so the user's session keeps moving.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Verdict labels used by both the gate and this ledger. Kept here as a
/// `&'static str` table so the ledger writer and reader agree without a
/// circular dependency on `gate_regression_check`'s `RegressionVerdict` enum
/// (which carries signal payloads we don't want to serialise into the line).
pub const VERDICT_GREEN: &str = "green";
/// Amber verdict label — the orchestrator confirms with the user.
pub const VERDICT_AMBER: &str = "amber";
/// Red verdict label — consolidation must be blocked.
pub const VERDICT_RED: &str = "red";

/// Cap on the rendered line length. Bounded so the OS-level append stays
/// atomic on every supported platform (POSIX guarantees atomicity up to
/// `PIPE_BUF`; we stay well below it).
pub const LINE_MAX_CHARS: usize = 1024;

/// Name of the on-disk ledger file (resolved relative to the wave directory).
pub const LEDGER_FILE_NAME: &str = "_review-spans.md";

/// One row of the ledger as written.
///
/// Stored as plain owned strings so the writer doesn't borrow caller-owned
/// signal lists; the price is one short allocation per `SubagentStop`, which
/// is dominated by the file write anyway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictEntry {
    /// One of [`VERDICT_GREEN`] / [`VERDICT_AMBER`] / [`VERDICT_RED`].
    pub verdict: String,
    /// Short caller-supplied identifier for the returning child.
    pub child_id: String,
    /// Wall-clock timestamp (ISO-8601) recorded at append time.
    pub iso_ts: String,
    /// Count of regression signals carried by the verdict.
    pub signal_count: usize,
    /// Localised body of the first signal (already translated by the gate).
    pub first_message: String,
}

impl VerdictEntry {
    /// Render the entry into its on-disk line form.
    ///
    /// The output never contains an embedded `\n` (newline characters in
    /// `first_message` are collapsed) and is truncated to
    /// [`LINE_MAX_CHARS`] so the OS-level append stays atomic.
    #[must_use]
    pub fn render_line(&self) -> String {
        // Collapse embedded newlines so the ledger stays one row per line.
        let safe_message: String = self
            .first_message
            .chars()
            .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
            .collect();
        let raw = format!(
            "- {} | {} | {} | signals={} | {}",
            self.verdict, self.child_id, self.iso_ts, self.signal_count, safe_message
        );
        if raw.chars().count() > LINE_MAX_CHARS {
            let truncated: String = raw.chars().take(LINE_MAX_CHARS).collect();
            return truncated;
        }
        raw
    }
}

/// Resolve the ledger path for `wave_dir`. Used by both
/// [`append_verdict`] and [`has_red_verdict`] so a single source of truth
/// drives the location.
#[must_use]
pub fn ledger_path(wave_dir: &Path) -> PathBuf {
    wave_dir.join(LEDGER_FILE_NAME)
}

/// Append `entry` as a new line to `<wave_dir>/_review-spans.md`.
///
/// Creates the file (and the wave directory, if missing) on first write.
/// Returns `Ok(())` on success; any IO error degrades fail-open and the
/// error is surfaced to the caller for telemetry, never propagated as a
/// blocking failure.
///
/// # Errors
///
/// Returns any [`std::io::Error`] raised by the underlying open / write so
/// the caller can record it (the hook layer treats it as a warning, not a
/// hard fail). The on-disk ledger is best-effort by design — the gate's
/// run-level Red verdict is the authoritative block.
pub fn append_verdict(wave_dir: &Path, entry: &VerdictEntry) -> std::io::Result<()> {
    // Best-effort directory create; ignore "already exists" so we don't
    // collapse a real failure into the next step.
    if !wave_dir.exists() {
        let _ = std::fs::create_dir_all(wave_dir);
    }
    let path = ledger_path(wave_dir);
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)?;
    // Render once, write once — a single `write_all` of <= LINE_MAX_CHARS
    // bytes is the atomic-append contract documented in this module.
    let mut line = entry.render_line();
    line.push('\n');
    file.write_all(line.as_bytes())?;
    Ok(())
}

/// Read every line of the ledger at `wave_dir` and return them parsed into
/// [`VerdictEntry`] rows. Malformed lines are skipped (fail-open). Returns
/// an empty vec when the ledger does not exist.
#[must_use]
pub fn read_entries(wave_dir: &Path) -> Vec<VerdictEntry> {
    let path = ledger_path(wave_dir);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    text.lines().filter_map(parse_line).collect()
}

/// Parse one rendered ledger line back into a [`VerdictEntry`]. Returns
/// `None` on malformed input so [`read_entries`] can skip without aborting.
fn parse_line(line: &str) -> Option<VerdictEntry> {
    // Strip the leading `- ` bullet introduced by [`VerdictEntry::render_line`].
    let body = line.trim_start().strip_prefix("- ")?.trim();
    // Split into the 5 declared columns. We split on " | " (with the
    // surrounding spaces) so a `child_id` that happens to contain a bare
    // `|` glyph does not split prematurely.
    let mut parts = body.splitn(5, " | ");
    let verdict = parts.next()?.trim().to_string();
    let child_id = parts.next()?.trim().to_string();
    let iso_ts = parts.next()?.trim().to_string();
    let signals_raw = parts.next()?.trim();
    let first_message = parts.next().unwrap_or("").trim().to_string();
    let signal_count = signals_raw
        .strip_prefix("signals=")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    Some(VerdictEntry {
        verdict,
        child_id,
        iso_ts,
        signal_count,
        first_message,
    })
}

/// Return `true` when the ledger at `wave_dir` records at least one red
/// verdict. The consolidation gate (AC-A-7) uses this as its single
/// boolean: any red line blocks the wave.
#[must_use]
pub fn has_red_verdict(wave_dir: &Path) -> bool {
    read_entries(wave_dir)
        .iter()
        .any(|e| e.verdict == VERDICT_RED)
}

/// Result of a consolidation check — distinguishes "no ledger" (`Allowed`)
/// from "ledger present and clean" (`Allowed`) from "red verdict captured"
/// (`Blocked` with the offending entry). The orchestrator turns `Blocked`
/// into an exit-code-2 close-gate failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsolidationCheck {
    /// Safe to consolidate — no red entries on file.
    Allowed,
    /// At least one red entry exists; consolidation must be blocked. The
    /// first red entry is surfaced so the caller can include it in the
    /// user-facing block message.
    Blocked {
        /// The first red entry the scan found.
        entry: VerdictEntry,
    },
}

/// Scan the ledger at `wave_dir` and decide whether the wave can consolidate.
/// Fail-open: a missing or unreadable ledger returns [`ConsolidationCheck::Allowed`].
#[must_use]
pub fn check_consolidation(wave_dir: &Path) -> ConsolidationCheck {
    for entry in read_entries(wave_dir) {
        if entry.verdict == VERDICT_RED {
            return ConsolidationCheck::Blocked { entry };
        }
    }
    ConsolidationCheck::Allowed
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn entry(verdict: &str, child: &str, msg: &str) -> VerdictEntry {
        VerdictEntry {
            verdict: verdict.to_string(),
            child_id: child.to_string(),
            iso_ts: "2026-05-27T18:00:00Z".to_string(),
            signal_count: 0,
            first_message: msg.to_string(),
        }
    }

    #[test]
    fn render_line_emits_expected_shape() {
        let e = entry(VERDICT_GREEN, "rt-impl", "no signals");
        let line = e.render_line();
        assert!(line.starts_with("- green | rt-impl | 2026-05-27T18:00:00Z | signals=0 | "));
        assert!(line.ends_with("no signals"));
        assert!(!line.contains('\n'));
    }

    #[test]
    fn render_line_collapses_embedded_newlines() {
        let e = entry(VERDICT_AMBER, "ui-impl", "line1\nline2\rline3");
        let line = e.render_line();
        assert!(!line.contains('\n'));
        assert!(!line.contains('\r'));
        assert!(line.contains("line1 line2 line3"));
    }

    #[test]
    fn append_then_read_round_trips_a_single_entry() {
        let dir = tempdir().unwrap();
        let wave = dir.path().join("wave-5-rt");
        let e = entry(VERDICT_GREEN, "rt-impl-1", "no signals");
        append_verdict(&wave, &e).expect("append succeeds");
        let entries = read_entries(&wave);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].verdict, "green");
        assert_eq!(entries[0].child_id, "rt-impl-1");
    }

    #[test]
    fn append_is_append_only_across_calls() {
        let dir = tempdir().unwrap();
        let wave = dir.path().join("wave-5-rt");
        append_verdict(&wave, &entry(VERDICT_GREEN, "a", "")).unwrap();
        append_verdict(&wave, &entry(VERDICT_AMBER, "b", "")).unwrap();
        append_verdict(&wave, &entry(VERDICT_RED, "c", "")).unwrap();
        let entries = read_entries(&wave);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].child_id, "a");
        assert_eq!(entries[1].child_id, "b");
        assert_eq!(entries[2].child_id, "c");
    }

    #[test]
    fn has_red_verdict_is_true_when_any_red_exists() {
        let dir = tempdir().unwrap();
        let wave = dir.path().join("wave-5-rt");
        append_verdict(&wave, &entry(VERDICT_GREEN, "a", "")).unwrap();
        append_verdict(&wave, &entry(VERDICT_RED, "b", "found stub")).unwrap();
        append_verdict(&wave, &entry(VERDICT_GREEN, "c", "")).unwrap();
        assert!(has_red_verdict(&wave));
    }

    #[test]
    fn has_red_verdict_is_false_when_no_red_exists() {
        let dir = tempdir().unwrap();
        let wave = dir.path().join("wave-5-rt");
        append_verdict(&wave, &entry(VERDICT_GREEN, "a", "")).unwrap();
        append_verdict(&wave, &entry(VERDICT_AMBER, "b", "")).unwrap();
        assert!(!has_red_verdict(&wave));
    }

    #[test]
    fn has_red_verdict_is_false_when_ledger_missing() {
        let dir = tempdir().unwrap();
        assert!(!has_red_verdict(&dir.path().join("never-existed")));
    }

    #[test]
    fn check_consolidation_blocks_on_first_red() {
        let dir = tempdir().unwrap();
        let wave = dir.path().join("wave-5-rt");
        append_verdict(&wave, &entry(VERDICT_GREEN, "a", "")).unwrap();
        append_verdict(&wave, &entry(VERDICT_RED, "b", "stub detected")).unwrap();
        append_verdict(&wave, &entry(VERDICT_RED, "c", "another stub")).unwrap();
        match check_consolidation(&wave) {
            ConsolidationCheck::Blocked { entry } => {
                assert_eq!(entry.child_id, "b");
                assert!(entry.first_message.contains("stub"));
            }
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    #[test]
    fn check_consolidation_allows_clean_ledger() {
        let dir = tempdir().unwrap();
        let wave = dir.path().join("wave-5-rt");
        append_verdict(&wave, &entry(VERDICT_GREEN, "a", "")).unwrap();
        append_verdict(&wave, &entry(VERDICT_AMBER, "b", "")).unwrap();
        assert_eq!(check_consolidation(&wave), ConsolidationCheck::Allowed);
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let dir = tempdir().unwrap();
        let wave = dir.path().join("wave-5-rt");
        std::fs::create_dir_all(&wave).unwrap();
        std::fs::write(
            ledger_path(&wave),
            "- green | a | 2026-05-27T00:00:00Z | signals=0 | ok\n\
             garbage line without bullet\n\
             - red | b | 2026-05-27T00:01:00Z | signals=2 | found stub\n",
        )
        .unwrap();
        let entries = read_entries(&wave);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].verdict, "green");
        assert_eq!(entries[1].verdict, "red");
        assert!(has_red_verdict(&wave));
    }

    #[test]
    fn render_line_truncates_to_line_max_chars() {
        let big = "x".repeat(LINE_MAX_CHARS * 2);
        let e = entry(VERDICT_GREEN, "huge", &big);
        let line = e.render_line();
        assert!(line.chars().count() <= LINE_MAX_CHARS);
    }
}
