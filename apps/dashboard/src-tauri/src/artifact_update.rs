//! Artifact-drift surface for the sidebar (B6 Wave 3).
//!
//! The Mustard repo ships `apps/cli/templates/.artifacts.json` — a manifest of
//! the artifacts vendored under `templates/` (skills, refs, commands,
//! hooks, tools). `mustard-rt run artifact-update --check` walks that manifest,
//! probes each upstream, and emits a JSON report with one entry per record
//! (status: `up-to-date` | `stale` | `unknown` | `tracked`).
//!
//! This module exposes two Tauri commands consumed by the sidebar:
//!
//! - [`artifact_update_check`] — invokes the `--check` probe for a given
//!   project path and reduces the report to a `{ total, stale, items }` shape
//!   that's cheap to render. `total` counts every record; `stale` filters to
//!   only those whose vendored version diverged from upstream.
//! - [`artifact_update_apply`] — invokes `--apply` to refresh the vendored
//!   trees / bump pinned versions. Only meaningful when the target project IS
//!   the Mustard repo itself (its `templates/` is the authoritative payload).
//! - [`is_mustard_repo`] — cheap presence check used to gate the menu entry:
//!   `apps/cli/templates/.artifacts.json` only exists in the canonical repo.
//!
//! `mustard-rt` is invoked as an external process (the same workflow the user
//! relies on at the shell — `cargo install --path apps/rt` keeps `$PATH` in
//! sync). When the binary is missing, callers see a clear error rather than a
//! cryptic spawn failure. All probes are read-only against the project tree
//! and never persisted by the dashboard — the React layer caches with TanStack
//! Query's `staleTime` (60s) instead.

use std::path::Path;
use crate::process_util::no_window_command;

use serde::Serialize;

/// One row of the `--check` report, mirroring the shape emitted by
/// `apps/rt/src/run/artifact_update.rs::CheckResult`. Field renames keep the
/// JSON wire format snake_case'd by Tauri while letting the frontend consume
/// camelCase-ish keys.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ArtifactDrift {
    /// Stable identifier — e.g. `skill:react-best-practices`.
    pub artifact_id: String,
    /// `skill` | `ref` | `command` | `hook` | `tool`.
    pub category: String,
    /// `up-to-date` | `stale` | `unknown` | `tracked`.
    pub status: String,
    /// `first-party` | `git` | `skills-directory` | `cargo` | `manual`.
    pub source_kind: String,
    /// Vendored version stamped in the manifest (may be `None` for tracked / manual).
    pub local_version: Option<String>,
    /// Upstream version resolved by the probe (may be `None` for tracked / errors).
    pub upstream_version: Option<String>,
}

/// Aggregated report consumed by the sidebar. `stale` is the headline count
/// that drives the badge; `items` carries the full table for tooltips / drawers.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ArtifactDriftReport {
    pub total: usize,
    pub stale: usize,
    pub items: Vec<ArtifactDrift>,
}

/// Outcome of an `--apply` run. The Rust-side payload mirrors the JSON
/// emitted by `apps/rt/src/run/artifact_update.rs::apply_updates` so the
/// frontend can show a meaningful toast (count of updates + the manifest-write
/// flag).
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ArtifactUpdateOutcome {
    pub applied: usize,
    pub manifest_written: bool,
}

/// Inspect the artifact manifest for `project_path` and reduce the report to
/// a `{ total, stale, items }` shape. Spawns `mustard-rt run artifact-update
/// --check` with `current_dir = project_path` so the subprocess resolves the
/// manifest relative to the project root (matching the CLI's default).
///
/// Errors are surfaced as strings — `Err` semantics are reserved for "we
/// couldn't ask the question" (binary missing, spawn failure, non-zero exit,
/// malformed JSON). A project with zero stale artifacts succeeds with
/// `stale: 0` so the badge can hide cleanly.
#[tauri::command]
pub async fn artifact_update_check(
    project_path: String,
) -> Result<ArtifactDriftReport, String> {
    let output = no_window_command("mustard-rt")
        .args(["run", "artifact-update", "--check"])
        .current_dir(&project_path)
        .output()
        .map_err(|e| format!("failed to spawn mustard-rt: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "mustard-rt exited with {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| format!("failed to parse artifact-update JSON: {e}"))?;

    // `--check` may legitimately surface an `error` object when the manifest
    // is missing — propagate it so the UI can show "no manifest" without
    // mis-classifying it as a stale-count of zero.
    if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
        return Err(err.to_string());
    }

    let results = parsed
        .get("results")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "artifact-update output missing 'results' array".to_string())?;

    let mut items: Vec<ArtifactDrift> = Vec::with_capacity(results.len());
    for raw in results {
        let id = raw.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let category = raw
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let status = raw
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let source_kind = raw
            .get("sourceKind")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let local_version = raw
            .get("local")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let upstream_version = raw
            .get("upstream")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        items.push(ArtifactDrift {
            artifact_id: id,
            category,
            status,
            source_kind,
            local_version,
            upstream_version,
        });
    }

    let stale = items.iter().filter(|i| i.status == "stale").count();
    let total = items.len();

    Ok(ArtifactDriftReport { total, stale, items })
}

/// Refresh the vendored payload for `project_path` via `mustard-rt run
/// artifact-update --apply`. Returns the count of records the run touched
/// plus whether the manifest was rewritten — both fields come from the JSON
/// summary emitted by the binary.
///
/// Only meaningful when `project_path` IS the canonical Mustard repo (the
/// frontend gates this behind [`is_mustard_repo`]). Calling on any other
/// folder will report `applied: 0` because the manifest is missing — the
/// binary fail-opens with an `error` field which we propagate.
#[tauri::command]
pub async fn artifact_update_apply(
    project_path: String,
) -> Result<ArtifactUpdateOutcome, String> {
    let output = no_window_command("mustard-rt")
        .args(["run", "artifact-update", "--apply"])
        .current_dir(&project_path)
        .output()
        .map_err(|e| format!("failed to spawn mustard-rt: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "mustard-rt exited with {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| format!("failed to parse artifact-update JSON: {e}"))?;

    if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
        return Err(err.to_string());
    }

    let applied = parsed
        .get("applied")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(0);
    let manifest_written = parsed
        .get("manifestWritten")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Ok(ArtifactUpdateOutcome {
        applied,
        manifest_written,
    })
}

/// `true` iff `project_path` contains the canonical artifact manifest
/// (`apps/cli/templates/.artifacts.json`). Used by the sidebar to gate the
/// "Update artifacts" menu entry — only the Mustard repo itself owns the
/// manifest, so showing the action on a consumer project would be misleading.
#[tauri::command]
pub fn is_mustard_repo(project_path: String) -> bool {
    Path::new(&project_path)
        .join("apps")
        .join("cli")
        .join("templates")
        .join(".artifacts.json")
        .exists()
}
