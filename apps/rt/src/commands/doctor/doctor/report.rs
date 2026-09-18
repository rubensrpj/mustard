//! Como o resultado das conferências sai: o relatório compacto em texto, uma
//! linha OK/WARN/FAIL/SKIP por conferência, ou o mesmo conteúdo em JSON.

use serde_json::json;

use super::{CheckResult, Status};

/// Print the compact OK/WARN/FAIL/SKIP report to stdout.
pub(super) fn render_report(results: &[CheckResult]) {
    let timestamp = mustard_core::time::now_iso8601();
    println!("mustard doctor — {timestamp}");
    println!("{}", "─".repeat(40));
    for r in results {
        let label = r.status.label();
        println!("{label:4}  {}", r.name);
        for detail in &r.details {
            println!("      · {detail}");
        }
    }
    println!("{}", "─".repeat(40));
    let any_fail = results.iter().any(|r| r.status == Status::Fail);
    let any_warn = results.iter().any(|r| r.status == Status::Warn);
    if any_fail {
        println!("status  FAIL — fix issues above before continuing");
    } else if any_warn {
        println!("status  WARN — review warnings above");
    } else {
        println!("status  OK — installation looks healthy");
    }
}

/// Serialize the report as JSON, in this shape:
///
/// ```json
/// {
///   "checks": [{ "name": "...", "status": "ok|warn|fail|skip",
///                "message": "...", "details": ["..."] }],
///   "overall": "ok|warn|fail",
///   "violations": [...]
/// }
/// ```
///
/// `status` is lowercased (the spec's `ok|warn|fail` contract);
/// `message` is the first detail line, joined with `; ` when multiple exist.
/// `details` is preserved for callers that want the full per-check list.
///
/// É o mesmo formato para uma conferência sozinha (`--check`) e para a rodada
/// inteira.
pub(super) fn render_report_json(results: &[CheckResult]) {
    let checks: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            let status_str = r.status.label().to_ascii_lowercase();
            let message = if r.details.is_empty() {
                String::new()
            } else if r.details.len() == 1 {
                r.details[0].clone()
            } else {
                r.details.join("; ")
            };
            json!({
                "name": r.name,
                "status": status_str,
                "message": message,
                "details": r.details,
            })
        })
        .collect();

    // Aggregate overall verdict (FAIL > WARN > OK; SKIP is neutral).
    let any_fail = results.iter().any(|r| r.status == Status::Fail);
    let any_warn = results.iter().any(|r| r.status == Status::Warn);
    let overall = if any_fail {
        "fail"
    } else if any_warn {
        "warn"
    } else {
        "ok"
    };

    let violations: Vec<String> = results
        .iter()
        .filter(|r| r.name == "skill-discovery" && r.status == Status::Warn)
        .flat_map(|r| r.details.iter())
        .cloned()
        .collect();

    let body = json!({
        "checks": checks,
        "overall": overall,
        "violations": violations,
    });
    println!("{}", serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".to_string()));
}
