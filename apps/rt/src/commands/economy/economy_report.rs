//! `mustard-rt run economy report` — list current baselines.
//!
//! Reads `<root>/.claude/spec/{spec}/economy-baselines.json` (per the W2 path
//! catalog) and emits the entries either as JSON (default) or a small ASCII
//! table. Pure read — no mutation, no event store touches.

use crate::commands::economy::economy_capture_baseline::{file_path_for, BaselineEntry, BaselineFile};
use crate::shared::context::{current_spec, session_id};
use mustard_core::time::now_iso8601;
use mustard_core::io::fs::read_to_string;
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};

/// Options for `mustard-rt run economy report`.
#[derive(Debug, Clone)]
pub struct ReportOpts {
    pub format: String,
    /// Per-spec baseline scope (W2 path catalog).
    pub spec: Option<String>,
}

/// JSON report.
#[derive(Debug, Serialize)]
pub struct EconomyReport {
    pub total: usize,
    pub entries: Vec<BaselineEntry>,
}

/// Pure loader for the legacy / cwd-only call sites.
#[must_use]
#[cfg(test)]
pub fn collect(cwd: &Path) -> EconomyReport {
    collect_for_spec(cwd, None)
}

/// Pure loader scoped to a specific spec name (W2 path catalog).
#[must_use]
pub fn collect_for_spec(cwd: &Path, spec: Option<&str>) -> EconomyReport {
    let file: BaselineFile = read_to_string(file_path_for(cwd, spec))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let mut entries: Vec<BaselineEntry> = file.entries.into_values().collect();
    entries.sort_by(|a, b| {
        a.operation
            .cmp(&b.operation)
            .then_with(|| a.wave.cmp(&b.wave))
    });
    EconomyReport {
        total: entries.len(),
        entries,
    }
}

/// Render the report as a compact ASCII table.
#[must_use]
pub fn render_table(report: &EconomyReport) -> String {
    use std::fmt::Write;
    let mut out = String::from("operation                       wave  duration_ms  captured_at\n");
    for e in &report.entries {
        let _ = writeln!(
            out,
            "{:<31} {:>4}  {:>11}  {}",
            truncate(&e.operation, 31),
            e.wave,
            e.duration_ms,
            e.captured_at
        );
    }
    if report.entries.is_empty() {
        out.push_str("(no baselines captured yet)\n");
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let kept: String = s.chars().take(max - 1).collect();
        format!("{kept}…")
    }
}

/// CLI entry.
pub fn run(opts: ReportOpts) {
    let started = std::time::Instant::now();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let resolved_spec = opts
        .spec
        .clone()
        .or_else(|| current_spec(cwd.to_string_lossy().as_ref()));
    let report = collect_for_spec(&cwd, resolved_spec.as_deref());
    match opts.format.as_str() {
        "table" => print!("{}", render_table(&report)),
        _ => {
            let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
            println!("{body}");
        }
    }
    emit_economy(started.elapsed().as_millis());
}

fn emit_economy(duration_ms: u128) {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("economy-report".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "economy-report",
            "duration_ms": duration_capped,
            "tokens_used": 0,
            "was_rust_only": true,
        }),
        spec: None,
    };
    let _ = crate::shared::events::route::emit(&cwd, &ev);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn collect_empty_dir_returns_zero_total() {
        let dir = tempdir().unwrap();
        let r = collect(dir.path());
        assert_eq!(r.total, 0);
        assert!(r.entries.is_empty());
    }

    #[test]
    fn render_table_includes_header_and_empty_hint() {
        let r = EconomyReport {
            total: 0,
            entries: Vec::new(),
        };
        let s = render_table(&r);
        assert!(s.contains("operation"));
        assert!(s.contains("no baselines"));
    }

    #[test]
    fn render_table_lists_entries() {
        let r = EconomyReport {
            total: 1,
            entries: vec![BaselineEntry {
                operation: "verify".to_string(),
                wave: 1,
                captured_at: "T".to_string(),
                duration_ms: 42,
                from_history: false,
            }],
        };
        let s = render_table(&r);
        assert!(s.contains("verify"));
        assert!(s.contains("42"));
    }

    #[test]
    fn truncate_keeps_short_strings_intact() {
        assert_eq!(truncate("short", 10), "short");
        assert!(truncate("a very long operation name", 5).ends_with("…"));
    }

    #[test]
    fn json_shape_has_total_and_entries() {
        let r = EconomyReport {
            total: 0,
            entries: Vec::new(),
        };
        let v = serde_json::to_value(r).unwrap();
        assert!(v.get("total").is_some());
        assert!(v.get("entries").unwrap().is_array());
    }
}
