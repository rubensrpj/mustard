//! `mustard-rt run economy reconcile` — re-derive baselines from recent events.
//!
//! For each baseline declared in `<root>/.claude/spec/{spec}/economy-baselines.json`
//! (per the W2 path catalog) matching `--wave N`, walks the most recent
//! `pipeline.economy.operation.invoked` events and updates the baseline
//! `duration_ms` to the median of the last three measurements (smooths
//! transient spikes). Idempotent — running twice with the same event store
//! yields the same baselines.

use crate::shared::context;
use crate::shared::events::economy;
use crate::commands::economy::economy_capture_baseline::{load, save, BaselineEntry};
use crate::shared::context::current_spec;
use mustard_core::time::now_iso8601;
use mustard_core::domain::economy::reader as economy_reader;
use mustard_core::domain::model::event::ActorKind;
use serde::Serialize;
use serde_json::json;
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

/// Median of up to N samples for `operation` from the NDJSON event log.
///
/// Delegates the walk to the canonical
/// [`economy_reader::operation_invoked_samples`] (the single owner of the
/// operation-invocation walk across every event sink), orders the samples by
/// `ts` descending (most recent first), takes the last `take`, and returns
/// `(median, sample_count)`.
fn median_duration_ms(cwd: &Path, operation: &str, take: usize) -> (i64, usize) {
    let mut samples = economy_reader::operation_invoked_samples(cwd, operation);
    if samples.is_empty() {
        return (0, 0);
    }
    // Sort by ts desc — most recent first — then take N samples.
    samples.sort_by(|a, b| b.ts.cmp(&a.ts));
    let mut durations: Vec<i64> = samples.into_iter().take(take).map(|s| s.duration_ms).collect();
    durations.sort_unstable();
    let mid = durations.len() / 2;
    (durations[mid], durations.len())
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
    let report = ReconcileReport {
        wave: opts.wave,
        records,
    };
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    economy::emit_operation(&context::cwd(), ActorKind::Orchestrator, "economy-reconcile", started.elapsed().as_millis() as u64, None, json!({}));
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
