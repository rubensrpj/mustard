// PRD Lapidator — Wave 2 (spec 2026-05-20-dashboard-prd-ai-lapidator)
//
// Shells out to the Claude Code CLI to "lapidate" a raw user intent into a
// fully-structured PRD JSON payload, and exposes a probe to check whether the
// `claude` CLI is in PATH at all.
//
// SERIALIZATION CONVENTION (exception to crate-wide snake_case):
//   The PRD types in this module use `#[serde(rename_all = "camelCase")]`
//   end-to-end because the upstream contract (the `/mustard:prd` slash
//   command output and the frontend `interface PrdForm` in
//   `src/pages/Prd.tsx`) is camelCase. Keeping camelCase on both the
//   deserialization (from claude stdout) and the re-serialization (to the
//   frontend) avoids a pointless re-mapping layer.
//
// WINDOWS-INVISIBLE INVOCATION:
//   Every `std::process::Command` constructed here goes through
//   `no_window_command`, which sets `CREATE_NO_WINDOW` (0x08000000) on
//   Windows so the user never sees a console flash when the dashboard
//   probes or invokes the Claude CLI.

use std::path::Path;

use serde::{Deserialize, Serialize};

// Bump this when Anthropic releases a newer Sonnet model that's good at structured JSON output.
const CLAUDE_MODEL: &str = "claude-sonnet-4-6";

// ── Domain types ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PrdData {
    #[serde(rename = "type")]
    pub type_: String,
    pub slug: String,
    pub title: String,
    pub scope: String, // "light" | "full"
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    pub layers: PrdLayers,
    pub boundaries: Vec<String>,
    pub checklist: Vec<String>,
    pub acceptance_criteria: Vec<PrdAc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decisions_not_obvious: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub non_goals: Option<Vec<String>>,
    #[serde(rename = "_confront")]
    pub confront: PrdConfront,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PrdLayers {
    pub backend: bool,
    pub frontend: bool,
    pub database: bool,
    pub design: bool,
    pub docs: bool,
    pub testes: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PrdAc {
    pub title: String,
    pub command: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PrdConfront {
    pub entities_found: Vec<String>,
    pub entities_missing: Vec<String>,
    pub paths_exist: Vec<String>,
    pub paths_missing: Vec<String>,
}

// ── Errors ───────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum PrdError {
    ClaudeNotFound,
    ClaudeError(String),
    InvalidJson(String),
    EmptyIntent,
}

impl std::fmt::Display for PrdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrdError::ClaudeNotFound => write!(
                f,
                "Claude CLI not found in PATH. Install it and try again."
            ),
            PrdError::ClaudeError(msg) => write!(f, "Claude CLI failed: {msg}"),
            PrdError::InvalidJson(msg) => write!(f, "Could not parse Claude output as PRD JSON: {msg}"),
            PrdError::EmptyIntent => write!(f, "Intent is empty."),
        }
    }
}

impl std::error::Error for PrdError {}

impl From<PrdError> for String {
    fn from(err: PrdError) -> String {
        err.to_string()
    }
}

// ── Provider abstraction ─────────────────────────────────────────────────────
//
// `PrdProvider` exists so a future spec can drop in an `OpenRouterProvider`
// (or other transport) without touching the Tauri command surface. Today the
// only implementation is `ClaudeCliProvider`.

pub trait PrdProvider {
    fn lapidate(&self, intent: &str, project_path: &Path) -> Result<PrdData, PrdError>;
}

pub struct ClaudeCliProvider;

impl ClaudeCliProvider {
    pub fn new() -> Self {
        ClaudeCliProvider
    }
}

impl Default for ClaudeCliProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl PrdProvider for ClaudeCliProvider {
    fn lapidate(&self, intent: &str, project_path: &Path) -> Result<PrdData, PrdError> {
        let trimmed = intent.trim();
        if trimmed.is_empty() {
            return Err(PrdError::EmptyIntent);
        }

        let prompt = format!("/mustard:prd {trimmed}");

        let output = crate::process_util::no_window_command("claude")
            .arg("-p")
            .arg(&prompt)
            // Headless one-shot: do not persist this run as a session. Without
            // it, every lapidate spawns a `<hostname>-<codename>` session that
            // lingers in the desktop "Recents" (and the cloud) with no activity.
            // `--no-session-persistence` only works in `--print`/`-p` mode.
            .arg("--no-session-persistence")
            .arg("--output-format")
            .arg("json")
            .arg("--model")
            .arg(CLAUDE_MODEL)
            .current_dir(project_path)
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    PrdError::ClaudeNotFound
                } else {
                    PrdError::ClaudeError(e.to_string())
                }
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(PrdError::ClaudeError(stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let trimmed_out = stdout.trim();

        // Claude `--output-format json` wraps the model's reply in an envelope:
        //   { "type": "result", "result": "<stringified or inlined PRD>", ... }
        // The PRD itself may arrive either as a JSON string inside `result` or
        // as raw stdout (older CLI builds). Try the envelope first, then fall
        // back to parsing stdout directly.
        if let Ok(envelope) = serde_json::from_str::<serde_json::Value>(trimmed_out) {
            if let Some(result_field) = envelope.get("result") {
                let parsed = match result_field {
                    serde_json::Value::String(s) => serde_json::from_str::<PrdData>(s.trim()),
                    other => serde_json::from_value::<PrdData>(other.clone()),
                };
                if let Ok(prd) = parsed {
                    return Ok(prd);
                }
            }
            if let Ok(prd) = serde_json::from_value::<PrdData>(envelope) {
                return Ok(prd);
            }
        }

        serde_json::from_str::<PrdData>(trimmed_out)
            .map_err(|e| PrdError::InvalidJson(e.to_string()))
    }
}

// ── Tauri commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn check_claude_available() -> bool {
    crate::process_util::no_window_command("claude")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

#[tauri::command]
pub fn lapidate_prd(intent: String, project_path: String) -> Result<PrdData, String> {
    let provider = ClaudeCliProvider::new();
    provider
        .lapidate(&intent, Path::new(&project_path))
        .map_err(String::from)
}
