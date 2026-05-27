//! Amend-window metric readers.
//!
//! Wave 6A of [[2026-05-26-no-sqlite-git-source-of-truth]]: the legacy
//! `pipeline_amend_window` SQLite table and the cross-join against `events`
//! are gone. The amend-window state now lives in
//! `.claude/spec/{spec}/.amend-window.json`, written atomically by
//! `apps/rt/src/hooks/amend_capture.rs` (W3C). Each file holds the
//! latest snapshot for one spec:
//!
//! ```json
//! {
//!   "spec":           "2026-05-26-some-spec",
//!   "session_id":     "abc123",
//!   "status":         "archived" | "closed-amend-drift" | "closed-amend-pending" | "open" | "amending" | ...,
//!   "opened_at":      "2026-05-26T12:00:00.000Z",
//!   "closed_at":      "2026-05-26T12:05:00.000Z",
//!   "last_amend_close_ts": "2026-05-26T12:04:55.000Z"
//! }
//! ```
//!
//! Readers walk `.claude/spec/*/.amend-window.json` cross-spec, compute the
//! aggregate, and return. Every entry-point is fail-open: a missing repo,
//! unreadable directory, or malformed JSON yields the type's zero value.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Snapshot loaded from a single `.amend-window.json` file. Every field is
/// optional so missing keys do not break the walk.
#[derive(Debug, Default, Deserialize)]
struct AmendWindow {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    closed_at: Option<String>,
    #[serde(default)]
    last_amend_close_ts: Option<String>,
}

/// Parse an ISO-8601 UTC timestamp into milliseconds since epoch.
/// Returns `None` on any parse failure. Uses `chrono` for correctness with
/// timezones, leap days and fractional seconds.
fn iso_to_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

/// Walk `.claude/spec/*/.amend-window.json` under `repo_path`. Returns the
/// successfully-deserialized windows; missing or malformed entries are
/// silently skipped.
fn load_windows(repo_path: &Path) -> Vec<AmendWindow> {
    let base = repo_path.join(".claude").join("spec");
    let Ok(entries) = std::fs::read_dir(&base) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path: PathBuf = entry.path().join(".amend-window.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(win) = serde_json::from_str::<AmendWindow>(&text) {
            out.push(win);
        }
    }
    out
}

/// True when the snapshot represents a closed window (not still being
/// negotiated). Mirrors the legacy SQL filter `status NOT IN ('open','amending')`.
fn is_closed(win: &AmendWindow) -> bool {
    match win.status.as_deref() {
        Some("open") | Some("amending") | None => false,
        _ => true,
    }
}

// ── 1. amend_resolution_rate ────────────────────────────────────────────────

fn resolution_rate(windows: &[AmendWindow]) -> f64 {
    let closed: Vec<&AmendWindow> = windows.iter().filter(|w| is_closed(w)).collect();
    let total = closed.len() as f64;
    if total <= 0.0 {
        return 0.0;
    }
    let archived = closed
        .iter()
        .filter(|w| w.status.as_deref() == Some("archived"))
        .count() as f64;
    archived / total
}

// ── 2. amend_drift_rate ─────────────────────────────────────────────────────

fn drift_rate(windows: &[AmendWindow]) -> f64 {
    let closed: Vec<&AmendWindow> = windows.iter().filter(|w| is_closed(w)).collect();
    let total = closed.len() as f64;
    if total <= 0.0 {
        return 0.0;
    }
    let drift = closed
        .iter()
        .filter(|w| w.status.as_deref() == Some("closed-amend-drift"))
        .count() as f64;
    drift / total
}

// ── 3. cross_session_amend_count ─────────────────────────────────────────────

fn carry_over_count(windows: &[AmendWindow]) -> u64 {
    windows
        .iter()
        .filter(|w| w.status.as_deref() == Some("closed-amend-pending"))
        .count() as u64
}

// ── 4. amend_window_duration ────────────────────────────────────────────────

fn window_durations(windows: &[AmendWindow]) -> Vec<i64> {
    let mut out = Vec::new();
    for win in windows.iter().filter(|w| is_closed(w)) {
        let (Some(closed_at), Some(last)) = (win.closed_at.as_deref(), win.last_amend_close_ts.as_deref()) else {
            continue;
        };
        if let (Some(closed_ms), Some(event_ms)) = (iso_to_ms(closed_at), iso_to_ms(last)) {
            out.push((closed_ms - event_ms).abs());
        }
    }
    out
}

// ── Tauri command wrappers ───────────────────────────────────────────────────

#[tauri::command]
pub fn amend_resolution_rate(repo_path: String) -> Result<f64, String> {
    let base = PathBuf::from(&repo_path);
    Ok(resolution_rate(&load_windows(&base)))
}

#[tauri::command]
pub fn amend_drift_rate(repo_path: String) -> Result<f64, String> {
    let base = PathBuf::from(&repo_path);
    Ok(drift_rate(&load_windows(&base)))
}

#[tauri::command]
pub fn cross_session_amend_count(repo_path: String) -> Result<u64, String> {
    let base = PathBuf::from(&repo_path);
    Ok(carry_over_count(&load_windows(&base)))
}

#[tauri::command]
pub fn amend_window_duration(repo_path: String) -> Result<Vec<i64>, String> {
    let base = PathBuf::from(&repo_path);
    Ok(window_durations(&load_windows(&base)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_window(dir: &Path, spec: &str, body: &str) {
        let spec_dir = dir.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join(".amend-window.json"), body).unwrap();
    }

    #[test]
    fn resolution_and_drift_zero_when_empty() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(resolution_rate(&load_windows(tmp.path())), 0.0);
        assert_eq!(drift_rate(&load_windows(tmp.path())), 0.0);
        assert_eq!(carry_over_count(&load_windows(tmp.path())), 0);
        assert!(window_durations(&load_windows(tmp.path())).is_empty());
    }

    #[test]
    fn rates_compute_over_closed_windows() {
        let tmp = TempDir::new().unwrap();
        write_window(tmp.path(), "spec-a", r#"{"status":"archived"}"#);
        write_window(tmp.path(), "spec-b", r#"{"status":"closed-amend-drift"}"#);
        write_window(tmp.path(), "spec-c", r#"{"status":"open"}"#);
        write_window(tmp.path(), "spec-d", r#"{"status":"closed-amend-pending"}"#);
        let windows = load_windows(tmp.path());
        // 3 closed total: 1 archived → 33.3%; 1 drift → 33.3%; 1 pending carries over.
        assert!((resolution_rate(&windows) - 1.0 / 3.0).abs() < 1e-9);
        assert!((drift_rate(&windows) - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(carry_over_count(&windows), 1);
    }

    #[test]
    fn duration_diffs_closed_vs_last_event() {
        let tmp = TempDir::new().unwrap();
        write_window(
            tmp.path(),
            "spec-z",
            r#"{"status":"archived","closed_at":"2026-05-26T12:05:00.000Z","last_amend_close_ts":"2026-05-26T12:04:55.000Z"}"#,
        );
        let windows = load_windows(tmp.path());
        let durations = window_durations(&windows);
        assert_eq!(durations, vec![5_000_i64]);
    }
}
