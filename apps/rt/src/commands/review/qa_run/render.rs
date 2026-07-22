//! qa-run report rendering: JSON sidecar, Markdown, and standalone HTML
//! artifacts for a QA run. Split out of `qa_run` (F3 PERF-D).

use crate::report::{table, Report};
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use serde_json::Value;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use super::AcResult;

/// Write the JSON sidecar at `<root>/.claude/spec/{spec}/qa-report.json` (the
/// per-spec aggregate, per the W2 path catalog).
pub(super) fn write_sidecar(cwd: &Path, spec: &str, payload: &Value) {
    let Some(sp) = ClaudePaths::for_project(cwd)
        .ok()
        .and_then(|p| p.for_spec(spec).ok())
    else {
        return;
    };
    let target = sp.qa_report_json_path();
    if let Some(parent) = target.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    if let Ok(text) = serde_json::to_string_pretty(payload) {
        let _ = fs::write_atomic(&target, text.as_bytes());
    }
}

/// Write the consolidated Markdown report at `.claude/spec/{spec}/qa/report.md`
/// (D4). The QA phase materialises its verdict by code so the result is durable
/// and visible in the dashboard, instead of depending on an agent remembering to
/// fill in a template. Atomic via [`fs::write_atomic`]. Fail-open: a missing
/// project root or write error is a silent no-op (the `qa.result` event is the
/// load-bearing record; this file is the human-readable mirror).
pub(super) fn write_qa_report_md(cwd: &Path, spec: &str, overall: &str, criteria: &[AcResult]) {
    let Some(sp) = ClaudePaths::for_project(cwd)
        .ok()
        .and_then(|p| p.for_spec(spec).ok())
    else {
        return;
    };
    let qa_dir = sp.dir().join("qa");
    if fs::create_dir_all(&qa_dir).is_err() {
        return;
    }

    let mut body = String::new();
    body.push_str("# QA Report\n\n");
    let _ = writeln!(body, "- Spec: `{spec}`");
    let _ = writeln!(body, "- Overall: **{}**", overall.to_uppercase());
    let _ = writeln!(body, "- Criteria: {}\n", criteria.len());
    body.push_str("## Acceptance Criteria\n\n");
    body.push_str("| ID | Status | Exit | Duration | Detail |\n");
    body.push_str("|----|--------|------|----------|--------|\n");
    for c in criteria {
        let exit = c.exit.map_or_else(|| "n/a".to_string(), |e| e.to_string());
        let duration = format!("{:.1}s", c.duration_ms as f64 / 1000.0);
        // Keep the detail cell on one line and pipe-safe so the table stays valid.
        let detail: String = c
            .stderr_excerpt
            .replace('|', "\\|")
            .replace('\n', " ")
            .chars()
            .take(80)
            .collect();
        let _ = writeln!(
            body,
            "| {} | {} | {} | {} | {} |",
            c.id,
            c.status.to_uppercase(),
            exit,
            duration,
            detail,
        );
    }
    body.push('\n');

    let _ = fs::write_atomic(qa_dir.join("report.md"), body.as_bytes());
}

/// Write the standalone HTML report at `<root>/.claude/spec/{spec}/qa-report.html`.
pub(super) fn write_html_report(cwd: &Path, spec: &str, overall: &str, criteria: &[AcResult]) -> Option<PathBuf> {
    let sp = ClaudePaths::for_project(cwd).ok()?.for_spec(spec).ok()?;
    let path = sp.qa_report_html_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok()?;
    }
    let mut report = Report::new(
        format!("QA Report — {spec}"),
        format!("overall: {overall} · {} criteria", criteria.len()),
    );
    let rows: Vec<Vec<String>> = criteria
        .iter()
        .map(|c| {
            vec![
                c.id.clone(),
                c.status.to_uppercase(),
                c.exit.map_or_else(|| "n/a".to_string(), |e| e.to_string()),
                format!("{:.1}s", c.duration_ms as f64 / 1000.0),
                c.stderr_excerpt.chars().take(80).collect(),
            ]
        })
        .collect();
    report.section(
        "Acceptance Criteria",
        &table(&["ID", "Status", "Exit", "Duration", "stderr"], &rows),
    );
    fs::write_atomic(&path, report.render().as_bytes()).ok()?;
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// D4: `write_qa_report_md` materialises `.claude/spec/{spec}/qa/report.md`
    /// with the overall verdict + a per-AC table.
    #[test]
    fn qa_report_md_is_materialized() {
        let dir = tempdir().unwrap();
        let criteria = vec![
            AcResult {
                id: "AC-1".into(),
                status: "pass".into(),
                exit: Some(0),
                duration_ms: 120,
                stderr_excerpt: String::new(),
            },
            AcResult {
                id: "AC-2".into(),
                status: "fail".into(),
                exit: Some(1),
                duration_ms: 50,
                stderr_excerpt: "boom | pipe".into(),
            },
        ];
        write_qa_report_md(dir.path(), "demo", "fail", &criteria);

        let report_path = ClaudePaths::for_project(dir.path())
            .unwrap()
            .for_spec("demo")
            .unwrap()
            .dir()
            .join("qa")
            .join("report.md");
        let md = std::fs::read_to_string(&report_path).unwrap();
        assert!(md.starts_with("# QA Report"));
        assert!(md.contains("Overall: **FAIL**"));
        assert!(md.contains("AC-1"));
        assert!(md.contains("AC-2"));
        // Pipe in the detail cell is escaped so the table stays valid.
        assert!(md.contains("boom \\| pipe"));
    }

    /// The `timeout` class is a first-class verdict in the human-readable
    /// report: both the overall header and the criterion's row name it, so a
    /// run killed by its deadline can never be read as a green or skipped one.
    #[test]
    fn qa_report_md_renders_the_timeout_class() {
        let dir = tempdir().unwrap();
        let criteria = vec![AcResult {
            id: "AC-1".into(),
            status: "timeout".into(),
            exit: None,
            duration_ms: 600_000,
            stderr_excerpt: "timeout after 600000ms".into(),
        }];
        write_qa_report_md(dir.path(), "slow", "timeout", &criteria);

        let report_path = ClaudePaths::for_project(dir.path())
            .unwrap()
            .for_spec("slow")
            .unwrap()
            .dir()
            .join("qa")
            .join("report.md");
        let md = std::fs::read_to_string(&report_path).unwrap();
        assert!(md.contains("Overall: **TIMEOUT**"), "{md}");
        assert!(md.contains("| AC-1 | TIMEOUT |"), "{md}");
        assert!(md.contains("timeout after 600000ms"), "{md}");
    }

    #[test]
    fn html_report_is_standalone() {
        let dir = tempdir().unwrap();
        let criteria = vec![AcResult {
            id: "AC-1".into(),
            status: "pass".into(),
            exit: Some(0),
            duration_ms: 12,
            stderr_excerpt: String::new(),
        }];
        let path = write_html_report(dir.path(), "demo", "pass", &criteria).unwrap();
        let html = std::fs::read_to_string(path).unwrap();
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<style>"));
        assert!(!html.contains("href=") && !html.contains("src="));
        assert!(html.contains("AC-1"));
    }
}
