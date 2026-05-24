//! `mustard-rt run rebuild-specs` — rematerialise the denormalised
//! `specs` + `metrics_projection` tables from the event stream.
//!
//! Why a dedicated subcommand
//! --------------------------
//!
//! The two tables (`specs.name/status/phase/…` and `metrics_projection.spec/…`)
//! used to be filled by the JS harness. After the `eliminate-bun` migration
//! removed that payload, nothing populated them — every column read by the
//! dashboard fell back to `NULL`, which the readers translated into the literal
//! `"unknown"` badges the 2026-05-20 audit captured. This subcommand closes
//! that gap by reading from the canonical event log and writing the projected
//! rows back in place.
//!
//! Design
//! ------
//!
//! - **Source of truth:** the harness event store (`mustard.db.events`).
//! - **Projection:** [`mustard_core::SqliteSpecReader`] — the same domain
//!   layer the dashboard reads from. Symmetric pipelines, no SQL drift.
//! - **Writes:** `INSERT OR REPLACE` against `specs` and `metrics_projection`
//!   via the new [`SqliteEventStore::upsert_spec`] /
//!   [`SqliteEventStore::upsert_metrics`] helpers. Idempotent — running twice
//!   is a no-op.
//! - **Failure model:** fail-open per spec. A spec whose events somehow refuse
//!   to project is recorded in the JSON output under `errors[]` and skipped;
//!   the rest still materialise.
//!
//! Output (JSON, written to stdout):
//!
//! ```json
//! {
//!   "specs_count": 17,
//!   "duration_ms": 42,
//!   "errors": []
//! }
//! ```
//!
//! Trigger paths
//! -------------
//!
//! 1. Manual: `mustard-rt run rebuild-specs` after any large event-store
//!    surgery (migration, ingest, history import).
//! 2. Automatic: `complete_spec.rs` calls [`rebuild_one`] after a pipeline
//!    closes, so the row for that spec is fresh by the time the dashboard
//!    polls.

use crate::run::env::project_dir;
use mustard_core::error::Result as CoreResult;
use mustard_core::store::sqlite_store::{MetricsRow, SpecRow, SqliteEventStore};
use mustard_core::{SpecFilter, SpecReader, SqliteSpecReader};
use serde_json::json;
use std::time::Instant;

// `rebuild_one` is `pub` because `complete_spec::run` calls it. The crate
// must also expose this module path; `mod rebuild_specs;` in `run/mod.rs`
// does that.

/// Subcommand entry point — full re-materialisation across every spec.
///
/// Always exits `0`: the JSON report carries the count and any per-spec
/// errors so a caller (e.g. an integration test) can read both.
pub fn run() {
    let started = Instant::now();
    let project = project_dir();

    let report = match rematerialize_all(&project) {
        Ok(r) => r,
        Err(err) => {
            // The reader could not even open the store. Emit a JSON error so
            // callers can `mustard-rt run rebuild-specs | jq .errors` —
            // matching the fail-open shape of every other run subcommand.
            print_json(&json!({
                "specs_count": 0,
                "duration_ms": started.elapsed().as_millis() as u64,
                "errors": [format!("open failed: {err}")],
            }));
            return;
        }
    };

    print_json(&json!({
        "specs_count": report.specs_count,
        "duration_ms": started.elapsed().as_millis() as u64,
        "errors": report.errors,
    }));
}

/// Re-materialise a single spec, intended for incremental updates after a
/// `pipeline.complete` or similar terminal event.
///
/// Fail-open: returns `Ok(())` even when the spec has no events at all (it
/// just writes a row with `NoEvents` status — same as the projection).
///
/// # Errors
///
/// Returns [`mustard_core::error::Error`] if the database cannot be opened.
pub fn rebuild_one(project_dir: &str, spec: &str) -> CoreResult<()> {
    if spec.is_empty() {
        return Ok(());
    }
    let store = SqliteEventStore::for_project(project_dir)?;
    let Ok(reader) = SqliteSpecReader::for_project(project_dir) else {
        return Ok(());
    };
    let Ok(Some(view)) = reader.spec_view(spec) else {
        return Ok(());
    };
    let _ = store.upsert_spec(&spec_row_from_view(&view));
    let _ = store.upsert_metrics(&metrics_row_from_view(&view));
    Ok(())
}

/// Result of a full rebuild — the report shape printed to stdout.
struct RebuildReport {
    specs_count: usize,
    errors: Vec<String>,
}

/// Walk every spec known to the event store and write its projected rows.
fn rematerialize_all(project_dir: &str) -> CoreResult<RebuildReport> {
    let store = SqliteEventStore::for_project(project_dir)?;
    let reader = SqliteSpecReader::for_project(project_dir).map_err(|e| {
        mustard_core::error::Error::config(format!("reader: {e}"))
    })?;
    let mut errors = Vec::new();
    let filter = SpecFilter::default();

    let summaries = match reader.list_specs(&filter) {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("list_specs: {e}"));
            return Ok(RebuildReport { specs_count: 0, errors });
        }
    };

    let mut count = 0usize;
    for summary in &summaries {
        let view = match reader.spec_view(&summary.spec) {
            Ok(Some(v)) => v,
            Ok(None) => continue,
            Err(e) => {
                errors.push(format!("{}: spec_view failed: {e}", summary.spec));
                continue;
            }
        };
        if let Err(e) = store.upsert_spec(&spec_row_from_view(&view)) {
            errors.push(format!("{}: upsert_spec failed: {e}", summary.spec));
            continue;
        }
        if let Err(e) = store.upsert_metrics(&metrics_row_from_view(&view)) {
            errors.push(format!("{}: upsert_metrics failed: {e}", summary.spec));
            continue;
        }
        count += 1;
    }

    Ok(RebuildReport { specs_count: count, errors })
}

/// Translate a [`SpecView`] into the legacy [`SpecRow`] shape.
///
/// The persisted `status` string is derived from the canonical
/// [`mustard_core::SpecState`] via [`SpecState::status_kebab`], which is the
/// single source of truth for the kebab-case mapping the dashboard reads.
fn spec_row_from_view(view: &mustard_core::SpecView) -> SpecRow {
    SpecRow {
        name: view.spec.clone(),
        status: Some(view.state.status_kebab().to_string()),
        phase: view.phase.map(|p| phase_string(p).to_string()),
        started_at: view.started_at.clone(),
        completed_at: if view.state.outcome == mustard_core::Outcome::Completed {
            view.last_event_at.clone()
        } else {
            None
        },
        affected_files: None, // legacy column — files now live on metrics_projection.tool_breakdown
    }
}

/// Translate a [`SpecView`] into a [`MetricsRow`]. The tool breakdown is a
/// JSON string the dashboard already knows how to decode.
fn metrics_row_from_view(view: &mustard_core::SpecView) -> MetricsRow {
    // Compact JSON: `{"tools_used": 12, "files_touched": 4, "agents_dispatched": 2}`.
    // The dashboard's existing reader treats `tool_breakdown` as opaque JSON
    // (it's used for the per-tool histogram). We pack the most useful
    // counters here so the read path always has *something*.
    let tool_breakdown = serde_json::to_string(&json!({
        "tools_used": view.tools_used,
        "files_touched": view.files_touched,
        "agents_dispatched": view.agents_dispatched,
    }))
    .ok();
    MetricsRow {
        spec: view.spec.clone(),
        api_calls: None,
        retries: None,
        pass1: if view.ac_total > 0 {
            Some(i64::from(u32::from(view.ac_passed == view.ac_total)))
        } else {
            None
        },
        tool_breakdown,
        dispatch_failures_by_phase: None,
        agent_count: Some(i64::from(view.agents_dispatched)),
        updated_at: view.last_event_at.clone(),
    }
}

/// Lowercase phase tag — matches the spec.md header convention.
const fn phase_string(p: mustard_core::Phase) -> &'static str {
    match p {
        mustard_core::Phase::Analyze => "analyze",
        mustard_core::Phase::Plan => "plan",
        mustard_core::Phase::Execute => "execute",
        mustard_core::Phase::Qa => "qa",
        mustard_core::Phase::Close => "close",
    }
}

/// Pretty-print a JSON value with two-space indentation — the byte-stable
/// shape every other `run` subcommand uses (CLAUDE.md guard).
fn print_json(value: &serde_json::Value) {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    println!("{text}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::store::event_store::EventSink;
    use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use serde_json::json;
    use tempfile::tempdir;

    fn seed(store: &SqliteEventStore, spec: &str, ts: &str, kind: &str, payload: serde_json::Value) {
        store
            .append(&HarnessEvent {
                v: SCHEMA_VERSION,
                ts: ts.into(),
                session_id: "s1".into(),
                wave: 0,
                actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
                event: kind.into(),
                payload,
                spec: Some(spec.into()),
            })
            .unwrap();
    }

    #[test]
    fn rematerialize_all_populates_specs_and_metrics_tables() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let store = SqliteEventStore::for_project(project).unwrap();

        seed(
            &store,
            "auth",
            "2026-05-20T10:00:00Z",
            "pipeline.scope",
            json!({ "scope": "full", "lang": "pt" }),
        );
        seed(&store, "auth", "2026-05-20T10:00:01Z", "tool.use", json!({}));

        let report = rematerialize_all(project).unwrap();
        assert_eq!(report.specs_count, 1);
        assert!(report.errors.is_empty());

        // Re-open to read.
        let store2 = SqliteEventStore::for_project(project).unwrap();
        let specs = store2.specs().unwrap();
        let row = specs.iter().find(|s| s.name == "auth").expect("spec row present");
        assert_eq!(row.status.as_deref(), Some("planning"));

        let metrics = store2.metrics("auth").unwrap().expect("metrics row present");
        let tb: serde_json::Value =
            serde_json::from_str(metrics.tool_breakdown.as_deref().unwrap_or("{}")).unwrap();
        assert_eq!(tb["tools_used"], 1);
    }

    #[test]
    fn rebuild_one_creates_row_for_targeted_spec_only() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let store = SqliteEventStore::for_project(project).unwrap();
        seed(&store, "auth", "2026-05-20T10:00:00Z", "tool.use", json!({}));
        seed(&store, "billing", "2026-05-20T10:00:00Z", "tool.use", json!({}));

        rebuild_one(project, "auth").unwrap();

        let store2 = SqliteEventStore::for_project(project).unwrap();
        let specs = store2.specs().unwrap();
        let names: Vec<_> = specs.iter().map(|s| s.name.clone()).collect();
        assert!(names.contains(&"auth".to_string()));
        assert!(!names.contains(&"billing".to_string()));
    }

    #[test]
    fn rematerialize_is_idempotent() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let store = SqliteEventStore::for_project(project).unwrap();
        seed(&store, "auth", "2026-05-20T10:00:00Z", "tool.use", json!({}));

        let first = rematerialize_all(project).unwrap();
        let second = rematerialize_all(project).unwrap();
        assert_eq!(first.specs_count, second.specs_count);
        // No duplicate rows from the second run.
        let store2 = SqliteEventStore::for_project(project).unwrap();
        let count = store2.specs().unwrap().iter().filter(|s| s.name == "auth").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn empty_store_yields_zero_count() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let _ = SqliteEventStore::for_project(project).unwrap();
        let report = rematerialize_all(project).unwrap();
        assert_eq!(report.specs_count, 0);
        assert!(report.errors.is_empty());
    }
}
