//! `mustard-rt run qa-run-all` — run QA for every active spec and aggregate
//! the results into a [`QaBatchReport`].
//!
//! Iterates specs via [`mustard_core::SqliteSpecReader`], filters to those
//! whose status `is_active()`, and calls [`super::qa_run::run_for_spec`] on
//! each. Fail-open per spec: a single failure goes into `errors[]`, not
//! propagated.
//!
//! Output: two-space pretty JSON on stdout (`QaBatchReport`).

use crate::run::env::project_dir;
use mustard_core::{SpecFilter, SpecStatusFilter, SqliteSpecReader, SpecReader};
use serde_json::json;

/// Dispatch `mustard-rt run qa-run-all`.
pub fn run() {
    let cwd = std::env::current_dir()
        .ok()
        .or_else(|| Some(std::path::PathBuf::from(project_dir())))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let reader = match SqliteSpecReader::for_project(&cwd) {
        Ok(r) => r,
        Err(e) => {
            let report = json!({
                "ran": 0,
                "failed": 0,
                "skipped": 0,
                "errors": [format!("could not open spec reader: {e}")]
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string()));
            return;
        }
    };

    let filter = SpecFilter {
        status: Some(SpecStatusFilter::Active),
        window: mustard_core::TimeWindow::All,
        search: None,
    };

    let specs = match reader.list_specs(&filter) {
        Ok(s) => s,
        Err(e) => {
            let report = json!({
                "ran": 0,
                "failed": 0,
                "skipped": 0,
                "errors": [format!("list_specs failed: {e}")]
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string()));
            return;
        }
    };

    let (mut ran, mut failed, mut skipped) = (0u32, 0u32, 0u32);
    let errors: Vec<String> = Vec::new();

    for summary in &specs {
        let outcome = super::qa_run::run_for_spec(&summary.spec);
        ran += 1;
        match outcome.overall.as_str() {
            "fail" => failed += 1,
            "skip" => skipped += 1,
            _ => {}
        }
        eprintln!(
            "[qa-run-all] spec={} overall={} passed={}/{} failed={} skipped={}",
            outcome.spec, outcome.overall, outcome.passed, outcome.total,
            outcome.failed, outcome.skipped,
        );
    }

    let report = json!({
        "ran": ran,
        "failed": failed,
        "skipped": skipped,
        "errors": errors
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string()));
}
