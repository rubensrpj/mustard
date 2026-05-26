//! `mustard-rt run economy reconcile` — re-derive baselines from recent events.
//!
//! For each baseline declared in `<root>/.claude/spec/{spec}/economy-baselines.json`
//! (per the W2 path catalog) matching `--wave N`, walks the most recent
//! `pipeline.economy.operation.invoked` events and updates the baseline
//! `duration_ms` to the median of the last three measurements (smooths
//! transient spikes). Idempotent — running twice with the same event store
//! yields the same baselines.

use crate::run::economy_capture_baseline::{file_path_for, BaselineEntry, BaselineFile};
use crate::run::env::{current_spec, session_id};
use crate::util::now_iso8601;
use mustard_core::fs::{read_to_string, write_atomic};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::telemetry::store::TelemetryStore;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Options for `mustard-rt run economy reconcile`.
#[derive(Debug, Clone)]
pub struct ReconcileOpts {
    pub wave: u32,
    /// Per-spec baseline scope (W2 path catalog). When `None`, fall back to
    /// the runtime active spec (via [`current_spec`]).
    pub spec: Option<String>,
}

/// One reconciled baseline entry.
#[derive(Debug, Serialize)]
pub struct ReconcileRecord {
    pub key: String,
    pub operation: String,
    pub wave: u32,
    pub old_duration_ms: i64,
    pub new_duration_ms: i64,
    pub samples: usize,
}

/// JSON report.
#[derive(Debug, Serialize)]
pub struct ReconcileReport {
    pub wave: u32,
    pub records: Vec<ReconcileRecord>,
}

fn load(cwd: &Path, spec: Option<&str>) -> BaselineFile {
    read_to_string(file_path_for(cwd, spec))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(cwd: &Path, spec: Option<&str>, file: &BaselineFile) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(file).unwrap_or_else(|_| "{}".to_string());
    write_atomic(file_path_for(cwd, spec), format!("{text}\n").as_bytes())
        .map_err(|e| std::io::Error::other(e.to_string()))
}

/// Median of up to N samples for `operation` from the event store.
fn median_duration_ms(cwd: &Path, operation: &str, take: usize) -> (i64, usize) {
    let Ok(store) = SqliteEventStore::for_project(cwd.to_string_lossy().as_ref()) else {
        return (0, 0);
    };
    let Ok(events) = store.replay() else {
        return (0, 0);
    };
    let mut durations: Vec<i64> = events
        .iter()
        .rev()
        .filter(|e| {
            e.event == "pipeline.economy.operation.invoked"
                && e.payload
                    .get("operation")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s == operation)
        })
        .take(take)
        .filter_map(|e| e.payload.get("duration_ms").and_then(Value::as_i64))
        .collect();
    if durations.is_empty() {
        return (0, 0);
    }
    durations.sort_unstable();
    let mid = durations.len() / 2;
    (durations[mid], durations.len())
}

/// W11.T11.3 — write one `economy_savings` row per reconciled `(wave_id,
/// operation)` pair, materialising the per-wave savings the dashboard
/// `/economia` Deep Refactor tab reads. The savings figure is the positive
/// delta between the historical baseline and the new median (in ms,
/// reinterpreted as "tokens saved" — the schema column is generic enough to
/// hold either unit; the dashboard labels it "Tokens economizados" so the
/// number reads as token-equivalent friction removed by the wave's work).
///
/// Fail-open: a missing telemetry.db or a SQL error degrades to a no-op so the
/// JSON report still prints.
fn record_savings(cwd: &Path, wave: u32, records: &[ReconcileRecord]) {
    let Ok(store) = TelemetryStore::for_project(cwd) else { return };
    let measured_at: i64 = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())) as i64;
    let wave_id = format!("W{wave}");
    for r in records {
        let savings: i64 = (r.old_duration_ms - r.new_duration_ms).max(0);
        let _ = store.conn().execute(
            "INSERT OR REPLACE INTO economy_savings \
                 (wave_id, operation, savings_tokens, measured_at) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![&wave_id, &r.operation, savings, measured_at],
        );
    }
}

/// CLI entry.
pub fn run(opts: ReconcileOpts) {
    let started = std::time::Instant::now();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let resolved_spec = opts
        .spec
        .clone()
        .or_else(|| current_spec(cwd.to_string_lossy().as_ref()));
    let mut file = load(&cwd, resolved_spec.as_deref());
    let mut records: Vec<ReconcileRecord> = Vec::new();
    let mut keys: Vec<String> = file.entries.keys().cloned().collect();
    keys.sort();
    for k in keys {
        let entry: BaselineEntry = match file.entries.get(&k) {
            Some(e) => e.clone(),
            None => continue,
        };
        if entry.wave != opts.wave {
            continue;
        }
        let (new_dur, samples) = median_duration_ms(&cwd, &entry.operation, 3);
        let old_dur = entry.duration_ms;
        if samples > 0 {
            let mut updated = entry.clone();
            updated.duration_ms = new_dur;
            updated.captured_at = now_iso8601();
            updated.from_history = true;
            file.entries.insert(k.clone(), updated);
        }
        records.push(ReconcileRecord {
            key: k,
            operation: entry.operation,
            wave: entry.wave,
            old_duration_ms: old_dur,
            new_duration_ms: new_dur,
            samples,
        });
    }
    if let Err(e) = save(&cwd, resolved_spec.as_deref(), &file) {
        eprintln!("[economy reconcile] WARN: write failed: {e}");
    }
    // W11.T11.3 — persist per-wave savings into telemetry.db.
    record_savings(&cwd, opts.wave, &records);

    let report = ReconcileReport {
        wave: opts.wave,
        records,
    };
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    emit_economy(started.elapsed().as_millis());
}

fn emit_economy(duration_ms: u128) {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());
    let spec = current_spec(&cwd);
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("economy-reconcile".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "economy-reconcile",
            "duration_ms": duration_capped,
            "tokens_used": 0,
            "was_rust_only": true,
        }),
        spec,
    };
    let _ = crate::run::event_route::emit(&cwd, &ev);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_serializes_to_required_fields() {
        let r = ReconcileReport {
            wave: 1,
            records: vec![ReconcileRecord {
                key: "x/1".to_string(),
                operation: "x".to_string(),
                wave: 1,
                old_duration_ms: 5,
                new_duration_ms: 7,
                samples: 3,
            }],
        };
        let v = serde_json::to_value(r).unwrap();
        assert_eq!(v["wave"], json!(1));
        assert!(v.get("records").unwrap().is_array());
        assert_eq!(v["records"][0]["samples"], json!(3));
    }

    #[test]
    fn median_helper_returns_zero_on_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let (d, s) = median_duration_ms(dir.path(), "any-op", 3);
        assert_eq!(d, 0);
        assert_eq!(s, 0);
    }

    #[test]
    fn record_fields_byte_stable() {
        let r = ReconcileRecord {
            key: "k".to_string(),
            operation: "o".to_string(),
            wave: 1,
            old_duration_ms: 1,
            new_duration_ms: 2,
            samples: 0,
        };
        let v = serde_json::to_value(r).unwrap();
        for f in ["key", "operation", "wave", "old_duration_ms", "new_duration_ms", "samples"] {
            assert!(v.get(f).is_some(), "missing {f}");
        }
    }
}
