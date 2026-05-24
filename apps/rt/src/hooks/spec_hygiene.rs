//! `spec_hygiene` — SessionStart spec-lifecycle hygiene + gated auto-close.
//!
//! ## Scope (spec-lifecycle-unification Wave 5)
//!
//! Runs on `SessionStart`, **before** the [`session_start`](crate::hooks::session_start)
//! memory injection (registration order in `registry.rs`). For each *active*
//! spec (header `Outcome: Active`, or a legacy `### Status:` that is not a
//! terminal one) it classifies the spec and either:
//!
//! - emits an advisory `hygiene.detected` event (stale / abandoned-suspect), or
//! - runs the close-gate and, **only if every check is green**, auto-closes the
//!   spec — emitting `hygiene.autoclose` + `pipeline.outcome: completed` and
//!   rewriting the spec header to `Outcome: Completed`.
//!
//! ## Safety (inviolable)
//!
//! - Auto-close **never** runs without a passing close-gate. A failing
//!   `verify-pipeline` (build/lint/test) or `qa-run` produces a
//!   `hygiene.skipped` event with the blocker — never `pipeline.outcome`.
//! - **Idempotent.** A spec whose header is already terminal is not active and
//!   is skipped silently — running the hook twice on a closed spec emits
//!   nothing (Scenario 5).
//! - **Fail-open.** Any error (unreadable dir, malformed spec, store failure)
//!   degrades to a no-op. The hook never returns `Err` and never crashes a
//!   session.
//!
//! ## Configuration
//!
//! Env `MUSTARD_HYGIENE_MODE`:
//! - `off`   — the hook is disabled (no events at all).
//! - `detect`— only `hygiene.detected` is emitted (every category, including
//!   `candidate`); auto-close never runs.
//! - `auto`  — default; the full behavior described above.

use mustard_core::error::Error;
use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::spec;
use mustard_core::{Flags, Outcome as SpecOutcome, SpecState, Stage};
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::util::{now_iso8601, now_millis};

/// Recency window: a commit within this window (in ms) counts as "recent".
const RECENT_COMMIT_MS: u128 = 72 * 60 * 60 * 1000;
/// A candidate spec must have been quiet for at least this long (6 h).
const CANDIDATE_QUIET_MS: u128 = 6 * 60 * 60 * 1000;
/// A stale spec's last event is at least this old (72 h).
const STALE_MS: u128 = 72 * 60 * 60 * 1000;
/// An abandoned-suspect spec's last event is at least this old (30 days).
const ABANDONED_MS: u128 = 30 * 24 * 60 * 60 * 1000;

/// The SessionStart spec-hygiene module.
pub struct SpecHygiene;

// ---------------------------------------------------------------------------
// Mode
// ---------------------------------------------------------------------------

/// The three hygiene modes resolved from `MUSTARD_HYGIENE_MODE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HygieneMode {
    /// Disabled — no events.
    Off,
    /// Detect-only — emit `hygiene.detected`, never auto-close.
    Detect,
    /// Full behavior (default).
    Auto,
}

impl HygieneMode {
    /// Resolve from `MUSTARD_HYGIENE_MODE`, defaulting to `auto`. An
    /// unrecognised value also falls back to `auto`.
    fn from_env() -> Self {
        match std::env::var("MUSTARD_HYGIENE_MODE")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "off" => Self::Off,
            "detect" => Self::Detect,
            _ => Self::Auto,
        }
    }
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// The hygiene category of a single active spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    /// All AC `[x]`, a recent commit touches the spec, and it has been quiet
    /// for ≥6 h — eligible for gated auto-close.
    Candidate,
    /// All AC `[x]` but no recent commit and the last event is ≥72 h old.
    Stale,
    /// Partial AC and the last event is ≥30 days old.
    AbandonedSuspect,
    /// Anything else — no action.
    Healthy,
}

impl Category {
    /// The `reason` token surfaced in a `hygiene.detected` payload.
    fn reason(self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::Stale => "stale",
            Self::AbandonedSuspect => "abandoned_suspect",
            Self::Healthy => "healthy",
        }
    }
}

/// The evidence gathered for one spec, used both to classify and to populate
/// the `hygiene.detected` payload.
struct Evidence {
    /// Fraction of AC marked `[x]` (`1.0` when all done, `0.0` when none).
    ac_pct: f64,
    /// `true` when every AC line is `[x]`.
    ac_complete: bool,
    /// `true` when the spec has at least one AC line.
    has_ac: bool,
    /// ISO timestamp of the last event of any kind for the spec.
    last_event_at: Option<String>,
    /// Milliseconds since the last event (None when there is no event).
    last_event_age_ms: Option<u128>,
    /// ISO timestamp of the last git commit that touched the spec dir.
    last_commit_at: Option<String>,
    /// Milliseconds since the last commit (None when unknown).
    last_commit_age_ms: Option<u128>,
}

impl Evidence {
    /// Categorise from the gathered evidence per the Wave-5 algorithm.
    fn categorize(&self) -> Category {
        if !self.ac_complete {
            // Partial AC: only abandoned-suspect is actionable.
            if self.has_ac
                && self
                    .last_event_age_ms
                    .is_some_and(|age| age >= ABANDONED_MS)
            {
                return Category::AbandonedSuspect;
            }
            return Category::Healthy;
        }
        // All AC complete from here on.
        let recent_commit = self
            .last_commit_age_ms
            .is_some_and(|age| age <= RECENT_COMMIT_MS);
        let quiet_enough = self
            .last_event_age_ms
            .is_some_and(|age| age >= CANDIDATE_QUIET_MS);
        if recent_commit && quiet_enough {
            return Category::Candidate;
        }
        if self
            .last_event_age_ms
            .is_some_and(|age| age >= STALE_MS)
            && !recent_commit
        {
            return Category::Stale;
        }
        Category::Healthy
    }

    /// The `evidence` object embedded in a `hygiene.detected` payload.
    fn to_payload(&self) -> Value {
        json!({
            "ac_pct": self.ac_pct,
            "last_event_at": self.last_event_at,
            "last_commit_at": self.last_commit_at,
        })
    }
}

// ---------------------------------------------------------------------------
// Spec discovery + header parsing
// ---------------------------------------------------------------------------

/// `true` when a spec's `spec.md` header marks it as still active — i.e. its
/// canonical [`SpecOutcome`] is `Active` (or no terminal status is declared).
///
/// Delegates to the canonical [`mustard_core::spec`] parser, which is
/// tolerant of the new `### Stage:`/`### Outcome:` header *and* every legacy
/// `### Status:`/`### Phase:` shape. A spec with no recognisable lifecycle
/// header is treated as active (a fresh spec).
fn is_active_spec(spec_md: &str) -> bool {
    match spec::parse_state(spec_md) {
        Some(state) => state.is_active(),
        // No recognisable lifecycle header → treat as active.
        None => true,
    }
}

/// Compute AC evidence from a spec body: `(ac_pct, ac_complete, has_ac)`.
///
/// Counts checkbox lines (`- [ ]` / `- [x]`) inside the `## Acceptance
/// Criteria` (EN) / `## Critérios de Aceitação` (PT) section. Falls back to
/// the whole document when no such section is present (matching the lenient
/// close-gate behavior).
fn ac_evidence(spec_md: &str) -> (f64, bool, bool) {
    let section = ac_section(spec_md).unwrap_or(spec_md);
    let mut total = 0usize;
    let mut checked = 0usize;
    for line in section.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("- [") {
            // `- [x]` or `- [ ]` (single char between the brackets).
            let mut chars = rest.chars();
            let mark = chars.next();
            if chars.next() == Some(']') {
                total += 1;
                if matches!(mark, Some('x' | 'X')) {
                    checked += 1;
                }
            }
        }
    }
    if total == 0 {
        return (0.0, false, false);
    }
    let pct = checked as f64 / total as f64;
    (pct, checked == total, true)
}

/// Extract the `## Acceptance Criteria` / `## Critérios de Aceitação` section
/// body, up to the next `## ` heading. Case-insensitive on the heading.
fn ac_section(spec_md: &str) -> Option<&str> {
    // Build a (byte-offset, line-without-terminator) table. `split_inclusive`
    // keeps each segment's real byte length (including any `\r\n`), so the
    // offset advances by the true terminator width — CRLF-safe, and every
    // `start` is guaranteed to land on a char boundary. We trim the trailing
    // `\r?\n` only for the heading comparison.
    let lines: Vec<(usize, &str)> = spec_md
        .split_inclusive('\n')
        .scan(0usize, |off, seg| {
            let start = *off;
            *off += seg.len();
            Some((start, seg.trim_end_matches(['\n', '\r'])))
        })
        .collect();
    let mut start_idx = None;
    for (i, (_, line)) in lines.iter().enumerate() {
        let lower = line.trim_start().to_ascii_lowercase();
        if lower.starts_with("## ")
            && (lower.contains("acceptance criteria") || lower.contains("critérios de aceitação"))
        {
            start_idx = Some(i + 1);
            break;
        }
    }
    let start_idx = start_idx?;
    let mut end_idx = lines.len();
    for (i, (_, line)) in lines.iter().enumerate().skip(start_idx) {
        if line.starts_with("## ") {
            end_idx = i;
            break;
        }
    }
    let start_off = lines.get(start_idx).map(|(o, _)| *o)?;
    let end_off = lines
        .get(end_idx)
        .map_or(spec_md.len(), |(o, _)| *o)
        .min(spec_md.len());
    // Defense in depth: never index with `&spec_md[a..b]` (panics off a char
    // boundary). `get` returns `None` instead — degrade to "no AC section"
    // rather than crash the session (fail-open contract).
    spec_md.get(start_off..end_off)
}

#[cfg(test)]
mod ac_section_tests {
    use super::ac_section;

    /// CRLF line endings + accented Portuguese must not panic and must still
    /// extract the AC body. Regression for the byte-offset drift bug: `lines()`
    /// strips `\r\n` but the old offset advanced by `len()+1`, so on CRLF files
    /// the cumulative under-count eventually sliced inside a multibyte char.
    #[test]
    fn crlf_with_accents_extracts_and_does_not_panic() {
        // Build explicitly with `\r\n` — a raw literal may be saved as LF.
        let body = [
            "# Mustard 2.0 — Phase 3: MCP Memory Server",
            "### Outcome: Active",
            "",
            "Justificativa: a implementação não está pronta — revisão pendente.",
            "",
            "## Critérios de Aceitação",
            "- [x] AC-1: configuração validada — ó é ção",
            "- [ ] AC-2: ainda em revisão",
            "",
            "## Próxima Seção",
            "irrelevante",
        ]
        .join("\r\n");

        let ac = ac_section(&body).expect("AC section must be found");
        assert!(ac.contains("AC-1"), "AC body should contain AC-1; got:\n{ac}");
        assert!(ac.contains("AC-2"), "AC body should contain AC-2; got:\n{ac}");
        assert!(
            !ac.contains("Próxima Seção"),
            "AC body must stop at the next heading; got:\n{ac}"
        );
    }
}

// ---------------------------------------------------------------------------
// Timestamps + git
// ---------------------------------------------------------------------------

/// Parse an ISO-8601 `YYYY-MM-DDThh:mm:ss(.sss)?Z` string to epoch
/// milliseconds. Returns `None` for any malformed input — the inverse of
/// [`now_iso8601`]; no calendar crate dependency.
fn iso_to_millis(ts: &str) -> Option<u128> {
    let ts = ts.trim();
    let bytes = ts.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let num = |a: usize, b: usize| ts.get(a..b)?.parse::<i64>().ok();
    let year = num(0, 4)?;
    let month = num(5, 7)?;
    let day = num(8, 10)?;
    let hour = num(11, 13)?;
    let min = num(14, 16)?;
    let sec = num(17, 19)?;
    // Optional `.sss` milliseconds.
    let millis: i64 = if ts.len() >= 23 && bytes.get(19) == Some(&b'.') {
        ts.get(20..23).and_then(|s| s.parse().ok()).unwrap_or(0)
    } else {
        0
    };
    // days_from_civil (Howard Hinnant) — inverse of now_iso8601's civil_from_days.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let total_secs = days * 86_400 + hour * 3_600 + min * 60 + sec;
    if total_secs < 0 {
        return None;
    }
    u128::try_from(total_secs * 1_000 + millis).ok()
}

/// The ISO timestamp of the last git commit that touched `spec_dir`, via
/// `git log --pretty=%cI -1 -- <spec_dir>`. Returns `None` on any failure
/// (no git, no commit, detached worktree) — the candidate path then simply
/// never fires, which is safe (no auto-close on incomplete evidence).
fn last_commit_iso(cwd: &Path, spec_dir: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["log", "--pretty=%cI", "-1", "--"])
        .arg(spec_dir)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let line = text.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    // Normalise `%cI` (`2026-05-21T12:00:00+00:00`) to a `Z` form iso_to_millis
    // can parse — it only reads the leading 19 chars + optional `.sss`, so the
    // offset suffix is ignored. Hand the raw string through.
    Some(line.to_string())
}

// ---------------------------------------------------------------------------
// Event emission
// ---------------------------------------------------------------------------

/// Append a `hygiene.*` event to the project store. Best-effort: a store
/// failure is swallowed (telemetry is never load-bearing).
fn emit(store: &SqliteEventStore, kind: &str, spec: &str, payload: Value) {
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: "spec-hygiene".to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("spec-hygiene".to_string()),
            actor_type: None,
        },
        event: kind.to_string(),
        payload,
        spec: Some(spec.to_string()),
    };
    let _ = store.append(&event);
}

// ---------------------------------------------------------------------------
// Close-gate (shells out to the run face)
// ---------------------------------------------------------------------------

/// The blocker that stopped an auto-close, or `None` when the gate is green.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Blocker {
    /// `verify-pipeline` (build/lint/test) returned non-zero.
    BuildRed,
    /// `qa-run` reported a failing acceptance criterion.
    AcFailing,
}

impl Blocker {
    fn token(self) -> &'static str {
        match self {
            Self::BuildRed => "build_red",
            Self::AcFailing => "ac_failing",
        }
    }
}

/// Run the close-gate for `spec`: `verify-pipeline` then `qa-run --spec`.
///
/// Returns `Ok(())` when both are green, `Err(blocker)` otherwise. Each step
/// shells to the binary's own `run` face via `current_exe()`. A *spawn* error
/// (env bug, no exe) is treated as green for that step — fail-open: an
/// environment problem must never *force* a close, and the other step still
/// gates. `qa-run` exit `0` covers both "pass" and "skip" (no testable AC),
/// matching the close-gate's advisory QA semantics.
///
/// Under `cfg(test)` the shell is skipped (returns green) so unit tests can
/// drive the classifier without spawning the libtest binary recursively.
fn run_close_gate(cwd: &Path, spec: &str) -> Result<(), Blocker> {
    if cfg!(test) {
        return Ok(());
    }
    let Ok(exe) = std::env::current_exe() else {
        // No exe to shell to → cannot verify → do not auto-close (treat as
        // build_red so we emit hygiene.skipped rather than silently close).
        return Err(Blocker::BuildRed);
    };

    // 1. verify-pipeline (build/lint/test). Exit 1 ⇒ build red.
    let verify = Command::new(&exe)
        .args(["run", "verify-pipeline"])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match verify {
        Ok(s) if s.success() => {}
        Ok(_) => return Err(Blocker::BuildRed),
        // Spawn failure → env bug, not a real red. Fall through to QA.
        Err(_) => {}
    }

    // 2. qa-run --spec NAME (idempotent). Exit 1 ⇒ an AC failed.
    let qa = Command::new(&exe)
        .args(["run", "qa-run", "--spec", spec])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match qa {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err(Blocker::AcFailing),
        Err(_) => Ok(()),
    }
}

/// Rewrite the spec header so it reads the terminal `Close` + `Completed`
/// canonical state, emitting the NEW three-line header format regardless of the
/// legacy shape it started in. Delegates the atomic, byte-stable rewrite to the
/// canonical [`mustard_core::spec`] writer. Fail-open: a missing file or a
/// write error is a silent no-op (the `pipeline.outcome` event the caller also
/// emits remains the durable record).
fn mark_completed(spec_md_path: &Path) {
    // `SpecState::new(Close, Completed, default)` is always legal; the
    // unreachable Err arm degrades to a no-op via the `Ok(_)` guard.
    let Ok(state) = SpecState::new(Stage::Close, SpecOutcome::Completed, Flags::default()) else {
        return;
    };
    let _ = spec::write_state(spec_md_path, &state);
}

// ---------------------------------------------------------------------------
// The hook body
// ---------------------------------------------------------------------------

/// Resolve the project dir for an invocation: the harness `cwd`, else `.`.
fn project_dir(input: &HookInput, ctx: &Ctx) -> String {
    if !ctx.project_dir.is_empty() {
        return ctx.project_dir.clone();
    }
    match input.cwd.as_deref() {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => ".".to_string(),
    }
}

/// Run hygiene over every spec in `.claude/spec/`. Pure side effect — every
/// error path is swallowed (fail-open).
fn run_hygiene(cwd: &str) {
    let mode = HygieneMode::from_env();
    if mode == HygieneMode::Off {
        return;
    }
    let spec_root = Path::new(cwd).join(".claude").join("spec");
    let Ok(entries) = std::fs::read_dir(&spec_root) else {
        return;
    };
    let Ok(store) = SqliteEventStore::for_project(cwd) else {
        return;
    };

    for entry in entries.filter_map(std::result::Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(spec_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let spec_md_path = path.join("spec.md");
        let Ok(spec_md) = std::fs::read_to_string(&spec_md_path) else {
            continue;
        };
        // Idempotence: a terminal spec is not active → skip silently.
        if !is_active_spec(&spec_md) {
            continue;
        }
        process_spec(&store, cwd, spec_name, &path, &spec_md_path, &spec_md, mode);
    }
}

/// Classify one active spec and act per [`HygieneMode`].
fn process_spec(
    store: &SqliteEventStore,
    cwd: &str,
    spec_name: &str,
    spec_dir: &Path,
    spec_md_path: &Path,
    spec_md: &str,
    mode: HygieneMode,
) {
    let now = now_millis();
    let (ac_pct, ac_complete, has_ac) = ac_evidence(spec_md);

    // Last event of any kind for this spec.
    let events = store.query(Some(spec_name)).unwrap_or_default();
    let last_event_at = events.last().map(|e| e.ts.clone());
    let last_event_age_ms = last_event_at
        .as_deref()
        .and_then(iso_to_millis)
        .map(|t| now.saturating_sub(t));

    // Last commit that touched the spec dir.
    let last_commit_at = last_commit_iso(Path::new(cwd), spec_dir);
    let last_commit_age_ms = last_commit_at
        .as_deref()
        .and_then(iso_to_millis)
        .map(|t| now.saturating_sub(t));

    let evidence = Evidence {
        ac_pct,
        ac_complete,
        has_ac,
        last_event_at,
        last_event_age_ms,
        last_commit_at,
        last_commit_age_ms,
    };

    let category = evidence.categorize();

    match category {
        Category::Healthy => {} // no action
        Category::Stale | Category::AbandonedSuspect => {
            emit(
                store,
                "hygiene.detected",
                spec_name,
                json!({
                    "spec": spec_name,
                    "reason": category.reason(),
                    "evidence": evidence.to_payload(),
                }),
            );
        }
        Category::Candidate => {
            if mode == HygieneMode::Detect {
                // detect mode never auto-closes — surface as detected.
                emit(
                    store,
                    "hygiene.detected",
                    spec_name,
                    json!({
                        "spec": spec_name,
                        "reason": category.reason(),
                        "evidence": evidence.to_payload(),
                    }),
                );
                return;
            }
            // auto mode — run the close-gate, then act on the verdict.
            match run_close_gate(Path::new(cwd), spec_name) {
                Ok(()) => {
                    emit(
                        store,
                        "hygiene.autoclose",
                        spec_name,
                        json!({
                            "spec": spec_name,
                            "gate_result": { "build": "pass", "qa": "pass" },
                            "emitted_at": now_iso8601(),
                        }),
                    );
                    // Record the canonical terminal outcome + rewrite header.
                    emit(
                        store,
                        "pipeline.outcome",
                        spec_name,
                        json!({ "outcome": "completed" }),
                    );
                    mark_completed(spec_md_path);
                }
                Err(blocker) => {
                    emit(
                        store,
                        "hygiene.skipped",
                        spec_name,
                        json!({
                            "spec": spec_name,
                            "blocker": blocker.token(),
                            "details": "close-gate not green; spec left active",
                        }),
                    );
                }
            }
        }
    }
}

impl Check for SpecHygiene {
    /// On `SessionStart`, run spec hygiene as a pure side effect, then self-
    /// allow. Any non-`SessionStart` trigger self-allows immediately. The hook
    /// never produces a verdict (its output is the `hygiene.*` event stream),
    /// so it always returns [`Verdict::Allow`].
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::SessionStart) {
            return Ok(Verdict::Allow);
        }
        let cwd = project_dir(input, ctx);
        run_hygiene(&cwd);
        Ok(Verdict::Allow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(
        ac_pct: f64,
        ac_complete: bool,
        has_ac: bool,
        event_age: Option<u128>,
        commit_age: Option<u128>,
    ) -> Evidence {
        Evidence {
            ac_pct,
            ac_complete,
            has_ac,
            last_event_at: event_age.map(|_| "2026-01-01T00:00:00.000Z".to_string()),
            last_event_age_ms: event_age,
            last_commit_at: commit_age.map(|_| "2026-01-01T00:00:00.000Z".to_string()),
            last_commit_age_ms: commit_age,
        }
    }

    const HOUR: u128 = 60 * 60 * 1000;
    const DAY: u128 = 24 * HOUR;

    #[test]
    fn candidate_when_complete_recent_commit_and_quiet() {
        // all AC done, commit 4h ago, last event 8h ago.
        let e = ev(1.0, true, true, Some(8 * HOUR), Some(4 * HOUR));
        assert_eq!(e.categorize(), Category::Candidate);
    }

    #[test]
    fn stale_when_complete_old_event_no_recent_commit() {
        // all AC done, last event 100h ago, commit 100h ago (not recent).
        let e = ev(1.0, true, true, Some(100 * HOUR), Some(100 * HOUR));
        assert_eq!(e.categorize(), Category::Stale);
    }

    #[test]
    fn abandoned_suspect_when_partial_and_very_old() {
        let e = ev(0.5, false, true, Some(60 * DAY), None);
        assert_eq!(e.categorize(), Category::AbandonedSuspect);
    }

    #[test]
    fn healthy_when_complete_but_too_fresh() {
        // all AC done, but last event only 2h ago — not quiet enough.
        let e = ev(1.0, true, true, Some(2 * HOUR), Some(4 * HOUR));
        assert_eq!(e.categorize(), Category::Healthy);
    }

    #[test]
    fn partial_recent_is_healthy() {
        let e = ev(0.5, false, true, Some(HOUR), Some(HOUR));
        assert_eq!(e.categorize(), Category::Healthy);
    }

    #[test]
    fn is_active_reads_outcome_header() {
        assert!(is_active_spec("# X\n### Outcome: Active\n"));
        assert!(!is_active_spec("# X\n### Outcome: Completed\n"));
        assert!(!is_active_spec("# X\n### Outcome: cancelled\n"));
    }

    #[test]
    fn is_active_reads_legacy_status_header() {
        assert!(is_active_spec("# X\n### Status: implementing\n"));
        assert!(!is_active_spec("# X\n### Status: completed\n"));
        assert!(!is_active_spec("# X\n### Status: superseded\n"));
        // No header → active.
        assert!(is_active_spec("# X\nno header\n"));
    }

    #[test]
    fn ac_evidence_counts_acceptance_section() {
        let md = "# X\n## Acceptance Criteria\n- [x] AC-1\n- [x] AC-2\n";
        let (pct, complete, has) = ac_evidence(md);
        assert!((pct - 1.0).abs() < f64::EPSILON);
        assert!(complete);
        assert!(has);
    }

    #[test]
    fn ac_evidence_partial() {
        let md = "# X\n## Acceptance Criteria\n- [x] AC-1\n- [ ] AC-2\n";
        let (pct, complete, has) = ac_evidence(md);
        assert!((pct - 0.5).abs() < f64::EPSILON);
        assert!(!complete);
        assert!(has);
    }

    #[test]
    fn iso_round_trips_through_now() {
        let now = now_millis();
        let iso = now_iso8601();
        let parsed = iso_to_millis(&iso).expect("parse");
        // Within one second of now (sub-ms rounding aside).
        assert!(now.abs_diff(parsed) < 2_000, "now={now} parsed={parsed}");
    }

    #[test]
    fn iso_parses_offset_form() {
        // git %cI form with a +00:00 offset — leading 19 chars are read.
        assert!(iso_to_millis("2026-05-21T12:00:00+00:00").is_some());
    }
}
