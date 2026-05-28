//! `rtk gain` normalisation — a port of `scripts/_rtk-gain.js`.
//!
//! The JS `_rtk-gain.js` was a shared helper, not a standalone script: it
//! shells `rtk gain --all --format json` and normalises the result across rtk
//! versions. This module keeps it as a helper consumed by `run metrics` (and
//! `run statusline`) and also exposes a thin `mustard-rt run rtk-gain` face so
//! the JS entrypoint has a one-to-one Rust subcommand.
//!
//! Fail-open: `rtk` missing, a timeout, or unparseable JSON yields `None`,
//! exactly like the JS helper returning `null`.
//!
//! ## Wave 3 — economia-moat-unification
//!
//! The CLI face (`mustard-rt run rtk-gain`) now *additionally* persists each
//! rewrite into the W1 `savings_records` table before printing the legacy JSON
//! summary to stdout. Translation is delegated to
//! [`mustard_core::economy::sources::rtk::ingest`]; the printed JSON is
//! unchanged so existing consumers (`run metrics`, `run statusline`, the
//! dashboard's RTK panel) keep parsing the same shape they did before. The
//! [`get_rtk_gain`] helper used by `run metrics` keeps its ad-hoc spawn — it
//! has a different output shape (`--all --format json`) and is read-only.

use mustard_core::economy::{self, sources::rtk as rtk_source, sources::IngestContext};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::{json, Value};
use std::process::{Command, Stdio};

use crate::shared::context::{current_spec, project_dir, session_id};
use crate::shared::events::route;
use crate::util::now_iso8601;

/// Normalised `rtk gain` summary — the fields `metrics.js` consumed.
#[derive(Debug, Clone)]
pub struct RtkGain {
    /// Total tokens saved by RTK rewrites.
    pub saved: i64,
    /// Total original (pre-filter) token count.
    pub original_total: i64,
    /// Average savings percentage.
    pub pct: f64,
    /// Number of commands rewritten.
    pub commands: i64,
    /// Per-command breakdown (`data.by_command`), passed through verbatim.
    pub by_command: Option<Value>,
}

impl RtkGain {
    /// Serialise to the JSON shape `_rtk-gain.js` returned.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "saved": self.saved,
            "originalTotal": self.original_total,
            "pct": self.pct,
            "commands": self.commands,
            "byCommand": self.by_command.clone().unwrap_or(Value::Null),
        })
    }
}

/// Read a numeric field from a `serde_json` object, tolerating string numbers
/// and the alternate key spellings `_rtk-gain.js` accepted.
fn num(obj: &Value, keys: &[&str]) -> f64 {
    for key in keys {
        if let Some(v) = obj.get(*key) {
            if let Some(n) = v.as_f64() {
                return n;
            }
            if let Some(s) = v.as_str() {
                if let Ok(n) = s.parse::<f64>() {
                    return n;
                }
            }
        }
    }
    0.0
}

/// Shell `rtk gain --all --format json` and normalise the result.
///
/// Returns `None` on any failure (rtk absent, non-zero exit, bad JSON), or
/// when both `saved` and `commands` are non-positive — the JS guard
/// `if (saved <= 0 && commands <= 0) return null`.
#[must_use]
pub fn get_rtk_gain() -> Option<RtkGain> {
    let output = Command::new("rtk")
        .args(["gain", "--all", "--format", "json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8(output.stdout).ok()?;
    let data: Value = serde_json::from_str(&raw).ok()?;
    // The JS reads `data.summary` when present, else `data` itself.
    let summary = data.get("summary").unwrap_or(&data);

    let saved = num(summary, &["total_saved", "saved_tokens", "savedTokens"]) as i64;
    let original = num(summary, &["total_input", "total_original"]) as i64;
    let pct = num(summary, &["avg_savings_pct", "savings_pct", "savingsPct"]);
    let commands = num(summary, &["total_commands", "commands"]) as i64;

    if saved <= 0 && commands <= 0 {
        return None;
    }
    Some(RtkGain {
        saved,
        original_total: original,
        pct,
        commands,
        by_command: data.get("by_command").cloned(),
    })
}

/// Dispatch `mustard-rt run rtk-gain` — persist per-rewrite savings into the
/// economy `savings_records` table, then print the normalised summary JSON.
///
/// The printed JSON shape is unchanged from the JS port (same `saved` /
/// `originalTotal` / `pct` / `commands` / `byCommand` keys) so existing
/// consumers (`run metrics`, the dashboard's RTK panel) keep parsing it.
/// Persistence is best-effort: every failure path is `eprintln!` + continue,
/// the stdout JSON is still emitted.
pub fn run() {
    // Wave 3 (economia-moat-unification): persist savings before printing the
    // legacy summary so downstream telemetry sees the same rewrites the JSON
    // summary represents. Translation is delegated to
    // `mustard_core::economy::sources::rtk::ingest` — see the rtk adapter for
    // the fail-open contract on a missing `rtk` binary.
    persist_savings();

    match get_rtk_gain() {
        Some(gain) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&gain.to_json()).unwrap_or_else(|_| "null".into())
            );
        }
        None => println!("null"),
    }
}

/// Pull every `rtk gain --json` rewrite into the W1 `savings_records` table.
///
/// Fail-open: a missing `rtk` binary, an empty record set, a connection
/// failure, or a row insert error all degrade to an `eprintln!` and continue.
/// The caller (`run`) is responsible for emitting the legacy stdout JSON
/// regardless of this function's outcome.
fn persist_savings() {
    let cwd = project_dir();
    let session = session_id();
    let session_opt = if session == "unknown" || session.is_empty() {
        None
    } else {
        Some(session.clone())
    };
    let _ = current_spec(&cwd); // reserved for future per-spec attribution
    let ctx = IngestContext {
        project_path: cwd.clone(),
        session_id: session_opt,
    };

    let records = match rtk_source::ingest(&ctx) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("rtk_gain: sources::rtk::ingest failed ({e}); skipping persist");
            return;
        }
    };
    if records.is_empty() {
        return;
    }

    // W7B: each record becomes one `pipeline.economy.savings.rtk-rewrite`
    // NDJSON event via the shared payload builder. Fail-open per record.
    for rec in records {
        let (event_name, payload) = economy::writer::savings_event(&rec);
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: now_iso8601(),
            session_id: session_id(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Orchestrator,
                id: Some("rtk-gain".to_string()),
                actor_type: None,
            },
            event: event_name,
            payload,
            spec: current_spec(&cwd),
        };
        let _ = route::emit(&cwd, &event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn num_reads_string_and_numeric() {
        let v = json!({ "a": "12.5", "b": 7, "c": "nope" });
        assert!((num(&v, &["a"]) - 12.5).abs() < f64::EPSILON);
        assert!((num(&v, &["b"]) - 7.0).abs() < f64::EPSILON);
        assert_eq!(num(&v, &["c"]), 0.0);
        assert_eq!(num(&v, &["missing"]), 0.0);
    }

    #[test]
    fn to_json_has_expected_keys() {
        let g = RtkGain {
            saved: 100,
            original_total: 500,
            pct: 80.0,
            commands: 5,
            by_command: None,
        };
        let v = g.to_json();
        assert_eq!(v["saved"], json!(100));
        assert_eq!(v["originalTotal"], json!(500));
        assert_eq!(v["byCommand"], json!(null));
    }
}
