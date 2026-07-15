//! `mustard-rt run economy capture-baseline` — record a baseline measurement.
//!
//! Writes a baseline row into `<root>/.claude/spec/{spec}/economy-baselines.json`
//! (per the W2 path catalog — the legacy `<root>/.claude/.economy-baselines.json`
//! is retired) keyed by `(operation, wave)`. The dashboard `/economia` page
//! reads this file to compare future invocations against the original cost;
//! reconcile and report consume the same file.
//!
//! When `--from-history` is set, the baseline is derived from the most recent
//! `pipeline.economy.operation.invoked` event for the same operation (read
//! from the harness SQLite store). Otherwise the baseline is set to a zero
//! value — useful before the operation has ever run, to mark expected
//! coverage.

use serde_json::json;
use mustard_core::domain::model::event::ActorKind;
use crate::shared::context;
use crate::shared::events::economy;
use crate::shared::context::current_spec;
use mustard_core::time::now_iso8601;
use mustard_core::domain::economy::reader as economy_reader;
use mustard_core::io::fs::{read_to_string, write_atomic};
use mustard_core::ClaudePaths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Options for `mustard-rt run economy capture-baseline`.
#[derive(Debug, Clone)]
pub struct CaptureBaselineOpts {
    pub operation: String,
    pub wave: u32,
    pub from_history: bool,
    /// Per-spec baseline scope (the W2 catalog stores baselines at
    /// `<root>/.claude/spec/{spec}/economy-baselines.json`). When `None`,
    /// fall back to [`current_spec`] from the runtime env.
    pub spec: Option<String>,
}

/// Baseline entry.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BaselineEntry {
    pub operation: String,
    pub wave: u32,
    pub captured_at: String,
    pub duration_ms: i64,
    pub from_history: bool,
}

/// On-disk baseline file.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct BaselineFile {
    pub entries: BTreeMap<String, BaselineEntry>,
}

/// JSON report.
#[derive(Debug, Serialize)]
pub struct CaptureReport {
    pub operation: String,
    pub wave: u32,
    pub entry: BaselineEntry,
}

fn key(operation: &str, wave: u32) -> String {
    format!("{operation}/{wave}")
}

/// Resolve `<root>/.claude/spec/{spec}/economy-baselines.json` for the given
/// spec name, falling back to the legacy `<root>/.claude/.economy-baselines.json`
/// when the spec name is missing or the path catalog rejects it. The fallback
/// is intentionally side-by-side with the canonical path so historical
/// baselines remain readable while pipelines learn to pass `--spec`.
pub(crate) fn file_path_for(cwd: &Path, spec: Option<&str>) -> PathBuf {
    if let Some(name) = spec.filter(|s| !s.is_empty()) {
        if let Ok(sp) = ClaudePaths::for_project(cwd).and_then(|p| p.for_spec(name)) {
            return sp.economy_baselines_path();
        }
    }
    // Legacy fallback for callers without a spec: nest under the per-root
    // `.cache/` directory rather than minting another root-level dot file.
    ClaudePaths::for_project(cwd)
        .map(|p| p.cache_dir().join("economy-baselines.json"))
        .unwrap_or_else(|_| cwd.to_path_buf().join("economy-baselines.json"))
}

pub(crate) fn load(cwd: &Path, spec: Option<&str>) -> BaselineFile {
    read_to_string(file_path_for(cwd, spec))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub(crate) fn save(cwd: &Path, spec: Option<&str>, file: &BaselineFile) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(file).unwrap_or_else(|_| "{}".to_string());
    write_atomic(file_path_for(cwd, spec), format!("{text}\n").as_bytes())
        .map_err(|e| std::io::Error::other(e.to_string()))
}

/// Look up the latest historical duration_ms for `operation`.
///
/// Delegates the NDJSON walk to the canonical
/// [`economy_reader::operation_invoked_samples`] (the single owner of the
/// operation-invocation walk across every event sink) and picks the sample
/// whose `ts` is most recent. Returns `None` when no sample carries a
/// duration.
fn historical_duration_ms(cwd: &Path, operation: &str) -> Option<i64> {
    economy_reader::operation_invoked_samples(cwd, operation)
        .into_iter()
        .max_by(|a, b| a.ts.cmp(&b.ts))
        .map(|s| s.duration_ms)
}

/// CLI entry.
pub fn run(opts: CaptureBaselineOpts) {
    let started = std::time::Instant::now();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let dur = if opts.from_history {
        historical_duration_ms(&cwd, &opts.operation).unwrap_or(0)
    } else {
        0
    };
    let entry = BaselineEntry {
        operation: opts.operation.clone(),
        wave: opts.wave,
        captured_at: now_iso8601(),
        duration_ms: dur,
        from_history: opts.from_history,
    };
    // Resolve the spec for the per-spec baseline file (W2 catalog). When the
    // caller did not pass `--spec`, fall back to the runtime active spec so
    // existing pipelines keep working without flag churn.
    let resolved_spec = opts
        .spec
        .clone()
        .or_else(|| current_spec(cwd.to_string_lossy().as_ref()));
    let mut file = load(&cwd, resolved_spec.as_deref());
    file.entries.insert(key(&opts.operation, opts.wave), entry.clone());
    if let Err(e) = save(&cwd, resolved_spec.as_deref(), &file) {
        eprintln!("[economy capture-baseline] WARN: write failed: {e}");
    }
    let report = CaptureReport {
        operation: opts.operation.clone(),
        wave: opts.wave,
        entry,
    };
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    economy::emit_operation(&context::cwd(), ActorKind::Orchestrator, "economy-capture-baseline", started.elapsed().as_millis() as u64, None, json!({}));
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn key_uses_operation_and_wave() {
        assert_eq!(key("verify", 3), "verify/3");
    }

    #[test]
    fn capture_persists_entry() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        // Manually invoke the load/save pair (we don't shell to a real DB).
        let mut file = load(cwd, None);
        let entry = BaselineEntry {
            operation: "verify".to_string(),
            wave: 1,
            captured_at: "T".to_string(),
            duration_ms: 0,
            from_history: false,
        };
        file.entries.insert(key("verify", 1), entry.clone());
        save(cwd, None, &file).unwrap();
        let reloaded = load(cwd, None);
        let got = reloaded.entries.get("verify/1").unwrap();
        assert_eq!(got.operation, "verify");
    }

    #[test]
    fn missing_file_returns_empty_baseline() {
        let dir = tempdir().unwrap();
        let f = load(dir.path(), None);
        assert!(f.entries.is_empty());
    }

    #[test]
    fn per_spec_file_path_uses_spec_catalog() {
        let dir = tempdir().unwrap();
        // Plant the workspace anchor so `for_project` accepts the path.
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::write(dir.path().join("mustard.json"), b"{}").unwrap();
        let path = file_path_for(dir.path(), Some("my-spec"));
        assert!(
            path.ends_with("economy-baselines.json"),
            "unexpected path: {path:?}"
        );
        assert!(path.to_string_lossy().contains("my-spec"));
    }

    #[test]
    fn report_serializes_to_required_fields() {
        let entry = BaselineEntry {
            operation: "x".to_string(),
            wave: 1,
            captured_at: "T".to_string(),
            duration_ms: 5,
            from_history: false,
        };
        let r = CaptureReport {
            operation: "x".to_string(),
            wave: 1,
            entry,
        };
        let v = serde_json::to_value(r).unwrap();
        assert!(v.get("operation").is_some());
        assert!(v.get("wave").is_some());
        assert!(v.get("entry").is_some());
    }
}
