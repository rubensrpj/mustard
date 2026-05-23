//! `mustard-rt run backfill-run-usage-spec` — retroactive spec attribution.
//!
//! Companion to `backfill_run_usage_cost`. That subcommand prices rows that
//! came in without a model; this one restores the `spec` / `wave_id` /
//! `agent_id` stamp on rows that were carried over from the legacy
//! `mustard.db` migration (which copies rows but does not stamp them).
//!
//! Stdout shape:
//!
//! ```json
//! {
//!   "rows_scanned": 297,
//!   "rows_updated_primary": 0,
//!   "rows_updated_fallback": 280,
//!   "rows_unmatched": 17,
//!   "db_path": "..."
//! }
//! ```
//!
//! Fail-open on store open; exit 1 on UPDATE failure.

use mustard_core::telemetry::{writer, TelemetryStore};
use serde_json::json;

use crate::run::env::project_dir;

/// Run the spec backfill on the project's telemetry.db. Idempotent.
pub fn run() {
    let cwd = project_dir();

    let store = match TelemetryStore::for_project(&cwd) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("backfill_run_usage_spec: open telemetry store failed ({e}); skipping");
            println!(
                "{}",
                json!({
                    "rows_scanned": 0,
                    "rows_updated_primary": 0,
                    "rows_updated_fallback": 0,
                    "rows_unmatched": 0,
                    "db_path": cwd.clone(),
                    "error": e.to_string(),
                })
            );
            return;
        }
    };

    match writer::backfill_null_spec(store.conn()) {
        Ok(report) => {
            println!(
                "{}",
                json!({
                    "rows_scanned": report.scanned,
                    "rows_updated_primary": report.updated_primary,
                    "rows_updated_fallback": report.updated_fallback,
                    "rows_unmatched": report.unmatched,
                    "db_path": cwd,
                })
            );
        }
        Err(e) => {
            eprintln!("backfill_run_usage_spec: UPDATE failed: {e}");
            std::process::exit(1);
        }
    }
}
