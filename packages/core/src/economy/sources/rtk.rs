//! `rtk gain` adapter — translates the local `rtk` binary's savings ledger into
//! [`SavingsRecord`]s.
//!
//! `rtk gain --json` prints a JSON array where every entry is one rewrite or
//! filter operation that saved tokens, in this shape (subject to RTK
//! versioning):
//!
//! ```json
//! [
//!   { "command": "git status", "before_tokens": 2000, "after_tokens": 200,
//!     "saved_tokens": 1800, "model": "claude-3-5-sonnet" },
//!   ...
//! ]
//! ```
//!
//! This adapter shells out to the binary via [`std::process::Command`], parses
//! the JSON, and returns one [`SavingsRecord`] per non-zero entry — *without*
//! writing to SQLite. The caller is responsible for persisting via
//! [`crate::economy::writer::record_savings`].
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

use crate::economy::model::{SavingsRecord, SavingsSource};
use crate::economy::scope::ProjectPath;
use crate::error::{Error, Result};

use super::IngestContext;

/// Pluggable runner for `rtk gain --json` so tests can inject a fake
/// without spawning a process.
///
/// The single method returns the raw stdout as bytes (the same shape as
/// `std::process::Output::stdout`) so the adapter logic that decodes the JSON
/// stays in one place — both real and faked paths share it.
pub trait RtkCommand {
    /// Run `rtk gain --json` and return its stdout, or an [`Err`] if the
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
/// pointed at by `MUSTARD_RTK_BIN`).
#[derive(Debug, Default)]
pub struct RealRtkCommand;

impl RtkCommand for RealRtkCommand {
    fn run(&self) -> Result<Vec<u8>> {
        let bin = std::env::var("MUSTARD_RTK_BIN").unwrap_or_else(|_| "rtk".to_string());
        let output = Command::new(&bin)
            .args(["gain", "--json"])
            .output()
            .map_err(Error::from)?;
        if !output.status.success() {
            return Err(Error::check_failed(format!(
                "rtk gain --json exited {}",
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
                "rtk::ingest: `rtk gain --json` unavailable ({e}); returning 0 savings"
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
    let entries = match parsed.as_array() {
        Some(a) => a.clone(),
        None => {
            eprintln!("rtk::ingest: stdout was not a JSON array; returning 0 savings");
            return Ok(Vec::new());
        }
    };

    let ts = now_iso();
    let project = ProjectPath::new(&ctx.project_path);

    let mut out: Vec<SavingsRecord> = Vec::new();
    for entry in entries {
        let saved = entry
            .get("saved_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if saved <= 0 {
            // Zero / negative savings are not interesting and risk polluting
            // the per-source averages the dashboard renders.
            continue;
        }
        let model_target = entry
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned);

        // Preserve the full RTK entry under `extra` so the dashboard can
        // surface the rewriter's `command`/`before_tokens`/`after_tokens`
        // fields without forcing the core schema to grow per-source columns.
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
    Ok(out)
}

/// Now, formatted ISO-8601 to second precision (UTC).
fn now_iso() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, mo, d, h, mi, s) = epoch_secs_to_ymdhms(now);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

#[allow(clippy::cast_possible_truncation)]
fn epoch_secs_to_ymdhms(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
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

    /// Live-process test — gated `#[ignore]` so CI does not require `rtk` on
    /// PATH. Run locally with `cargo test -p mustard-core -- --ignored`.
    #[test]
    #[ignore = "requires local `rtk` binary on PATH"]
    fn ingest_via_real_command_does_not_panic() {
        let _ = ingest(&ctx());
    }
}
