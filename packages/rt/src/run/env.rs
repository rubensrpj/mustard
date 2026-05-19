//! Environment resolution for the `run` face.
//!
//! Unlike the enforcement faces, a `run` subcommand never receives a
//! `HookInput` — it resolves the project directory and session id from the
//! process environment, mirroring how the JS scripts did (`CLAUDE_PROJECT_DIR`,
//! `MUSTARD_SESSION_ID` / `CLAUDE_SESSION_ID`).

/// Resolve the project directory: `CLAUDE_PROJECT_DIR` when set, else the
/// process current working directory, else `"."`.
#[must_use]
pub fn project_dir() -> String {
    if let Ok(dir) = std::env::var("CLAUDE_PROJECT_DIR") {
        if !dir.is_empty() {
            return dir;
        }
    }
    std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string())
}

/// Resolve the current session id from the environment, defaulting to
/// `"unknown"` — matching the JS scripts' `MUSTARD_SESSION_ID` /
/// `CLAUDE_SESSION_ID` lookup.
#[must_use]
pub fn session_id() -> String {
    std::env::var("MUSTARD_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("CLAUDE_SESSION_ID").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "unknown".to_string())
}
