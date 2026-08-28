//! Doctor health surface for the sidebar (W10.T10.7).
//!
//! Wraps `mustard-rt run doctor --json` so the dashboard can render a status
//! badge in the sidebar footer without re-implementing the wiring/drift/wave-
//! integrity checks. The subprocess returns the W10.T10.6 shape:
//!
//! ```json
//! { "checks": [{ "name": "...", "status": "ok|warn|fail|skip",
//!                "message": "...", "details": [...] }],
//!   "overall": "ok|warn|fail" }
//! ```
//!
//! Exposed [`doctor_status`] command parses that output, normalises the
//! `status` field to the four-value contract, and returns it to the frontend.
//!
//! Fail-open: a missing `mustard-rt` binary, spawn failure, non-zero exit, or
//! malformed JSON degrades to `{ overall: "fail", checks: [], error: "..." }`
//! so the badge can render red with a useful tooltip rather than crashing.

use serde::Serialize;

use crate::process_util::no_window_command;

/// One check row, mirroring the W10.T10.6 JSON shape.
#[derive(Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct DoctorCheck {
    pub name: String,
    /// `ok` | `warn` | `fail` | `skip`.
    pub status: String,
    pub message: String,
    pub details: Vec<String>,
}

/// Aggregated doctor report consumed by the badge.
#[derive(Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct DoctorStatus {
    pub overall: String,
    pub checks: Vec<DoctorCheck>,
    /// Populated when the subprocess could not produce a parseable report
    /// (binary missing, spawn failure, non-zero exit, malformed JSON).
    pub error: Option<String>,
}

impl DoctorStatus {
    fn failure(reason: String) -> Self {
        Self {
            overall: "fail".to_string(),
            checks: Vec::new(),
            error: Some(reason),
        }
    }
}

/// Invoke `mustard-rt run doctor --json` inside `project_path` and reduce the
/// JSON to the [`DoctorStatus`] shape. Always returns `Ok` — the failure
/// payload is encoded in the `overall` + `error` fields so the frontend can
/// render a meaningful badge.
pub fn doctor_status(project_path: String) -> Result<DoctorStatus, String> {
    // The runtime spawns the doctor binary in the project root so its `.claude/`
    // sniffing aligns with the user's actual workspace.
    let spawn = no_window_command("mustard-rt")
        .args(["run", "doctor", "--json"])
        .current_dir(&project_path)
        .output();

    let output = match spawn {
        Ok(o) => o,
        Err(e) => {
            return Ok(DoctorStatus::failure(format!(
                "failed to spawn mustard-rt: {e}"
            )));
        }
    };

    // The doctor exits `1` when any check is FAIL, but the JSON body is still
    // valid on that path. Only treat a non-stdout failure (segfault, missing
    // binary surfaced as exit-without-stdout) as a hard error.
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(DoctorStatus::failure(format!(
            "mustard-rt exited with {} (no stdout): {}",
            output.status,
            stderr.trim()
        )));
    }

    let parsed: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(e) => {
            return Ok(DoctorStatus::failure(format!(
                "failed to parse doctor JSON: {e}"
            )));
        }
    };

    let overall = parsed
        .get("overall")
        .and_then(|v| v.as_str())
        .unwrap_or("warn")
        .to_string();
    let checks = parsed
        .get("checks")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut rows: Vec<DoctorCheck> = Vec::with_capacity(checks.len());
    for c in checks {
        let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let status = c
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("warn")
            .to_string();
        let message = c
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let details = c
            .get("details")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|d| d.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        rows.push(DoctorCheck { name, status, message, details });
    }

    Ok(DoctorStatus { overall, checks: rows, error: None })
}
