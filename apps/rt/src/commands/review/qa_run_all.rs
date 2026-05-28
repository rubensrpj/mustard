//! `mustard-rt run qa-run-all` — run QA for every active spec and aggregate
//! the results into a [`QaBatchReport`].
//!
//! Iterates specs via a filesystem walk of `.claude/spec/*/spec.md`, filters
//! those whose `### Stage:` header is not `Close` and `### Outcome:` is not
//! `Completed`/`Cancelled`/`Superseded`, and calls
//! [`super::qa_run::run_for_spec`] on each. Fail-open per spec: a single
//! failure goes into `errors[]`, not propagated.
//!
//! Output: two-space pretty JSON on stdout (`QaBatchReport`).

use crate::shared::context::project_dir;
use mustard_core::ClaudePaths;
use mustard_core::io::fs as mfs;
use serde_json::json;
use std::path::PathBuf;

/// Return `true` when a spec header indicates an active (non-terminal) spec.
///
/// A spec is active when its `### Stage:` is not `Close` and its
/// `### Outcome:` is not `Completed`, `Cancelled`, or `Superseded`.
fn is_active_spec(spec_md: &std::path::Path) -> bool {
    let text = match mfs::read_to_string(spec_md) {
        Ok(t) => t,
        Err(_) => return false,
    };
    // Only scan the first 30 lines for the lifecycle header block.
    let head: String = text.lines().take(30).collect::<Vec<_>>().join("\n");
    let stage = header_value(&head, "stage").unwrap_or_default();
    let outcome = header_value(&head, "outcome").unwrap_or_default();

    // Terminal stages / outcomes → not active.
    if stage.eq_ignore_ascii_case("close") {
        return false;
    }
    let terminal_outcomes = ["completed", "cancelled", "superseded"];
    if terminal_outcomes
        .iter()
        .any(|o| outcome.eq_ignore_ascii_case(o))
    {
        return false;
    }
    true
}

/// Parse `### Key: value` from a header block (case-insensitive key match).
fn header_value(head: &str, key_lower: &str) -> Option<String> {
    for line in head.lines() {
        let t = line.trim_start();
        let Some(rest) = t.strip_prefix("### ") else {
            continue;
        };
        let Some(colon) = rest.find(':') else { continue };
        if rest[..colon].trim().eq_ignore_ascii_case(key_lower) {
            let v = rest[colon + 1..].trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Walk `.claude/spec/` and return every active spec slug.
fn list_active_specs(cwd: &std::path::Path) -> Vec<String> {
    let spec_root = match ClaudePaths::for_project(cwd) {
        Ok(p) => p.spec_dir(),
        Err(_) => return Vec::new(),
    };
    let entries = match std::fs::read_dir(&spec_root) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut names: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let spec_md = path.join("spec.md");
        if !spec_md.exists() {
            continue;
        }
        if is_active_spec(&spec_md) {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    names
}

/// Dispatch `mustard-rt run qa-run-all`.
pub fn run() {
    let cwd = std::env::current_dir()
        .ok()
        .or_else(|| Some(PathBuf::from(project_dir())))
        .unwrap_or_else(|| PathBuf::from("."));

    let specs = list_active_specs(&cwd);

    let (mut ran, mut failed, mut skipped) = (0u32, 0u32, 0u32);
    let errors: Vec<String> = Vec::new();

    for spec in &specs {
        let outcome = super::qa_run::run_for_spec(spec);
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
    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string())
    );
}
