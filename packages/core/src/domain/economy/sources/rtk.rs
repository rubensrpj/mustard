//! `rtk gain` adapter — translates the local `rtk` binary's savings ledger into
//! [`SavingsRecord`]s.
//!
//! This adapter accepts **two output shapes** because the rtk CLI changed flag
//! conventions across versions and we want to ingest from either:
//!
//! ### Legacy shape — array of per-rewrite entries
//!
//! Older rtk builds (and a hypothetical future one that re-exposes per-rewrite
//! detail) returned a JSON array, one entry per shell rewrite:
//!
//! ```json
//! [
//!   { "command": "git status", "before_tokens": 2000, "after_tokens": 200,
//!     "saved_tokens": 1800, "model": "claude-3-5-sonnet" },
//!   ...
//! ]
//! ```
//!
//! ### Current shape — summary + daily breakdowns
//!
//! Current rtk versions (validated against the installed binary on 2026-05-23)
//! return a JSON object with a `summary` and time-bucketed breakdowns. The
//! adapter consumes `daily[]`, emitting one record per day:
//!
//! ```json
//! {
//!   "summary": { "total_commands": 22377, "total_saved": 409008460, ... },
//!   "daily":   [{ "date": "2026-03-29", "saved_tokens": 102367, ... }, ...],
//!   "weekly":  [...],
//!   "monthly": [...]
//! }
//! ```
//!
//! The CLI invocation switched from `rtk gain --json` (which exits 2 on
//! current rtk) to `rtk gain --all --format json` (the supported syntax).
//!
//! This adapter shells out to the binary via [`std::process::Command`], parses
//! the JSON, and returns one [`SavingsRecord`] per row — *without* writing to
//! `SQLite`. The caller is responsible for persisting via
//! [`crate::domain::economy::writer::record_savings`].
//!
//! ## Testability
//!
//! Calling a real subprocess in unit tests is flaky (the binary may not be on
//! PATH on CI machines, the version may shift between runs). The adapter is
//! built around the [`RtkCommand`] trait, so tests can inject a fake that
//! returns a canned JSON string. Production uses [`RealRtkCommand`] which
//! actually invokes the binary.
//!
//! ## Fail-open
//!
//! If the `rtk` binary is not found (the common case on a fresh machine
//! before the operator installs it), `ingest` returns `Ok(vec![])` with a
//! single `eprintln!` warning. Telemetry is never load-bearing — the absence
//! of `rtk` must not break any caller.

use std::process::Command;

use serde_json::Value;

use crate::domain::economy::model::{SavingsRecord, SavingsSource};
use crate::domain::economy::scope::ProjectPath;
use crate::platform::error::{Error, Result};

use super::IngestContext;
use crate::platform::time::now_iso8601;

/// Pluggable runner for `rtk gain` so tests can inject a fake without
/// spawning a process. The runner is responsible for fetching the raw
/// stdout; the parser in [`ingest_with`] decides how to interpret it.
///
/// The single method returns the raw stdout as bytes (the same shape as
/// `std::process::Output::stdout`) so the adapter logic that decodes the JSON
/// stays in one place — both real and faked paths share it.
pub trait RtkCommand {
    /// Run the rtk subcommand and return its stdout, or an [`Err`] if the
    /// command could not be spawned / exited non-zero.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] when the binary is missing or the spawn fails,
    /// and [`Error::CheckFailed`] when the binary exits non-zero. The adapter
    /// treats both as a fail-open "no savings to ingest" condition.
    fn run(&self) -> Result<Vec<u8>>;
}

/// Production [`RtkCommand`] — invokes the real binary on PATH (or the path
/// pointed at by `MUSTARD_RTK_BIN`). Calls `rtk gain --all --format json` to
/// match the current rtk CLI; the older `--json` shorthand exits 2 on
/// current builds.
#[derive(Debug, Default)]
pub struct RealRtkCommand;

impl RtkCommand for RealRtkCommand {
    fn run(&self) -> Result<Vec<u8>> {
        let bin = std::env::var("MUSTARD_RTK_BIN").unwrap_or_else(|_| "rtk".to_string());
        let output = Command::new(&bin)
            .args(["gain", "--all", "--format", "json"])
            .output()
            .map_err(Error::from)?;
        if !output.status.success() {
            return Err(Error::check_failed(format!(
                "rtk gain --all --format json exited {}",
                output.status
            )));
        }
        Ok(output.stdout)
    }
}

/// Translate `rtk gain --json` output into [`SavingsRecord`]s.
///
/// This is the default entry point — internally instantiates [`RealRtkCommand`].
/// For tests, prefer [`ingest_with`] and pass a fake runner.
///
/// # Errors
///
/// Always returns `Ok` in practice: a missing binary or spawn failure
/// collapses to `Ok(vec![])` with an `eprintln!` warning (fail-open).
pub fn ingest(ctx: &IngestContext) -> Result<Vec<SavingsRecord>> {
    ingest_with(ctx, &RealRtkCommand)
}

/// Variant of [`ingest`] that takes an explicit [`RtkCommand`] runner.
///
/// Exists for unit tests; production code can keep calling [`ingest`].
///
/// # Errors
///
/// Returns `Ok(vec![])` (never `Err`) on the binary-missing / spawn-fail
/// path. Returns `Ok` with parsed records on success. The only `Err` shape
/// is reserved for future variants that need to escalate.
pub fn ingest_with(ctx: &IngestContext, runner: &dyn RtkCommand) -> Result<Vec<SavingsRecord>> {
    let stdout = match runner.run() {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "rtk::ingest: `rtk gain --all --format json` unavailable ({e}); returning 0 savings"
            );
            return Ok(Vec::new());
        }
    };
    let parsed: Value = match serde_json::from_slice(&stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("rtk::ingest: stdout was not JSON ({e}); returning 0 savings");
            return Ok(Vec::new());
        }
    };

    let project = ProjectPath::new(&ctx.project_path);

    // ── Shape 1: legacy array of per-rewrite entries ─────────────────────
    // Kept for compatibility with older rtk builds (or a future one that
    // re-exposes per-rewrite detail). Each entry gets its own
    // `SavingsRecord` stamped with `now_iso8601()` because the legacy shape
    // carries no per-row timestamp.
    if let Some(entries) = parsed.as_array() {
        return Ok(map_legacy_array(entries.clone(), &project));
    }

    // ── Shape 2: current `{summary, daily, weekly, monthly}` object ──────
    // The current rtk CLI returns daily aggregates instead of per-rewrite
    // rows. We emit one record per `daily[]` entry — keeps the temporal
    // breakdown the dashboard wants without inventing fake per-rewrite rows.
    // Weekly/monthly are ignored (would double-count over `daily`).
    if let Some(daily) = parsed.get("daily").and_then(Value::as_array) {
        return Ok(map_daily_array(daily.clone(), &project));
    }

    eprintln!("rtk::ingest: stdout shape not recognised (no array, no `daily`); returning 0 savings");
    Ok(Vec::new())
}

/// Map a legacy `rtk gain --json` array (per-rewrite shape) to records.
///
/// One record per entry whose `saved_tokens > 0`. Timestamp is `now_iso8601`
/// because the legacy shape carries no per-row date. The full RTK entry
/// survives in `extra` so the dashboard can render command/before/after.
fn map_legacy_array(
    entries: Vec<Value>,
    project: &ProjectPath,
) -> Vec<SavingsRecord> {
    let ts = now_iso8601();
    let mut out: Vec<SavingsRecord> = Vec::new();
    for entry in entries {
        let saved = entry
            .get("saved_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if saved <= 0 {
            continue;
        }
        let model_target = entry
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let extra = match entry {
            Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        out.push(SavingsRecord {
            ts: ts.clone(),
            source: SavingsSource::RtkRewrite,
            tokens_saved: saved,
            model_target,
            project_path: project.clone(),
            spec_id: None,
            wave_id: None,
            agent_id: None,
            extra,
        });
    }
    out
}

/// Map current `rtk gain --all --format json` daily aggregates to records.
///
/// One record per day with `saved_tokens > 0`. Each record's `ts` is the
/// day's date at noon UTC so it sorts cleanly inside its bucket but doesn't
/// collide with a midnight timestamp from a different source. `model_target`
/// stays `None` because the daily roll-up doesn't track per-day model split.
fn map_daily_array(daily: Vec<Value>, project: &ProjectPath) -> Vec<SavingsRecord> {
    let mut out: Vec<SavingsRecord> = Vec::new();
    for entry in daily {
        let saved = entry
            .get("saved_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if saved <= 0 {
            continue;
        }
        // Use the rtk-reported `date` field. Stamp at noon UTC so a re-ingest
        // doesn't perfectly collide with another source's midnight write
        // (helps eyeball uniqueness in `savings_records.ts`).
        let ts = entry
            .get("date")
            .and_then(Value::as_str)
            .map_or_else(now_iso8601, |d| format!("{d}T12:00:00Z"));
        let extra = match entry {
            Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        out.push(SavingsRecord {
            ts,
            source: SavingsSource::RtkRewrite,
            tokens_saved: saved,
            model_target: None,
            project_path: project.clone(),
            spec_id: None,
            wave_id: None,
            agent_id: None,
            extra,
        });
    }
    out
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Fake runner that returns a canned stdout. Lets the test exercise the
    /// JSON-parsing path without spawning a subprocess.
    struct FakeRtk(Result<Vec<u8>>);

    impl RtkCommand for FakeRtk {
        fn run(&self) -> Result<Vec<u8>> {
            match &self.0 {
                Ok(v) => Ok(v.clone()),
                Err(e) => Err(match e {
                    Error::Io(_) => Error::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "fake",
                    )),
                    other => Error::check_failed(other.to_string()),
                }),
            }
        }
    }

    fn ctx() -> IngestContext {
        IngestContext {
            project_path: "/tmp/p".into(),
            session_id: None,
        }
    }

    #[test]
    fn ingest_with_maps_each_positive_entry_to_one_record() {
        let stdout = br#"[
            {"command":"git status","before_tokens":2000,"after_tokens":200,"saved_tokens":1800,"model":"claude-3-5-sonnet"},
            {"command":"ls","before_tokens":50,"after_tokens":50,"saved_tokens":0,"model":"claude-3-5-haiku"},
            {"command":"git log","before_tokens":5000,"after_tokens":500,"saved_tokens":4500}
        ]"#;
        let runner = FakeRtk(Ok(stdout.to_vec()));
        let out = ingest_with(&ctx(), &runner).unwrap();
        assert_eq!(out.len(), 2, "zero-saving entry must be filtered out");
        assert_eq!(out[0].tokens_saved, 1800);
        assert_eq!(out[0].source, SavingsSource::RtkRewrite);
        assert_eq!(out[0].model_target.as_deref(), Some("claude-3-5-sonnet"));
        // The original RTK entry survives in `extra` for the dashboard.
        assert_eq!(
            out[0].extra.get("command").and_then(Value::as_str),
            Some("git status")
        );
        assert_eq!(out[1].tokens_saved, 4500);
        assert!(out[1].model_target.is_none());
    }

    #[test]
    fn ingest_with_returns_empty_when_runner_fails() {
        let runner = FakeRtk(Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "rtk not found",
        ))));
        let out = ingest_with(&ctx(), &runner).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn ingest_with_returns_empty_when_stdout_is_not_json_array() {
        let runner = FakeRtk(Ok(b"not-json".to_vec()));
        let out = ingest_with(&ctx(), &runner).unwrap();
        assert!(out.is_empty());

        let runner_obj = FakeRtk(Ok(b"{\"not\":\"an array\"}".to_vec()));
        let out = ingest_with(&ctx(), &runner_obj).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn ingest_with_parses_summary_plus_daily_shape() {
        // Current rtk CLI output: a JSON object with `summary` and
        // time-bucketed breakdowns. Adapter should iterate `daily[]` and
        // emit one record per non-zero day, keyed by the rtk-reported date.
        let stdout = br#"{
            "summary": {"total_commands": 200, "total_saved": 5000},
            "daily": [
                {"date":"2026-05-21","commands":10,"saved_tokens":1500,"savings_pct":75.0},
                {"date":"2026-05-22","commands":5,"saved_tokens":0,"savings_pct":0.0},
                {"date":"2026-05-23","commands":20,"saved_tokens":3500,"savings_pct":80.0}
            ],
            "weekly":  [{"week":"2026-W21","saved_tokens":5000}],
            "monthly": [{"month":"2026-05","saved_tokens":5000}]
        }"#;
        let runner = FakeRtk(Ok(stdout.to_vec()));
        let out = ingest_with(&ctx(), &runner).unwrap();
        assert_eq!(out.len(), 2, "zero-saving day must be filtered out");
        assert_eq!(out[0].tokens_saved, 1500);
        assert_eq!(out[0].ts, "2026-05-21T12:00:00Z");
        assert_eq!(out[0].source, SavingsSource::RtkRewrite);
        assert!(out[0].model_target.is_none());
        assert_eq!(out[1].tokens_saved, 3500);
        assert_eq!(out[1].ts, "2026-05-23T12:00:00Z");
        // Weekly/monthly are intentionally ignored (would double-count).
    }

    #[test]
    fn ingest_with_returns_empty_for_unknown_object_shape() {
        // Object that is neither legacy array nor `{daily: [...]}`. Defensive
        // fail-open — no records, no panic.
        let stdout = br#"{"summary": {"total_commands": 1}, "totally_different": []}"#;
        let runner = FakeRtk(Ok(stdout.to_vec()));
        let out = ingest_with(&ctx(), &runner).unwrap();
        assert!(out.is_empty());
    }

    /// Live-process test — gated `#[ignore]` so CI does not require `rtk` on
    /// PATH. Run locally with `cargo test -p mustard-core -- --ignored`.
    #[test]
    #[ignore = "requires local `rtk` binary on PATH"]
    fn ingest_via_real_command_does_not_panic() {
        let _ = ingest(&ctx());
    }
}
