//! Tauri command `economy_summary` for the `/economia` page.
//!
//! Wave 6A of [[2026-05-26-no-sqlite-git-source-of-truth]] retired the
//! SQLite-backed `economy_savings` and `economy_baselines` tables. Per-wave
//! savings are now derived from NDJSON `pipeline.economy.savings.wave`
//! events emitted by the hook layer (W3A wave-13-rt); baselines remain
//! sourced from `mustard-rt run economy report --format json` (which does
//! not itself touch SQLite).
//!
//! Fail-open at every step: a missing `.events/` directory, a missing
//! binary on `PATH`, malformed JSON — each degrade to a default field
//! rather than an error.

use mustard_core::events::reader::EventReader;
use mustard_core::ClaudePaths;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Per-wave token savings row sourced from
/// `pipeline.economy.savings.wave` NDJSON events.
#[derive(Serialize, Default, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub struct WaveSavings {
    /// Wave label as recorded by the writer (e.g. `W0`, `W1`, ..., `W12`).
    pub wave_id: String,
    /// Total `savings_tokens` summed across every operation in this wave.
    pub savings_tokens: i64,
    /// Distinct operations contributing to this wave's savings.
    pub operations: i64,
}

/// One baseline entry as returned by `economy_report::EconomyReport`. We keep
/// the parse lenient — only the fields the dashboard consumes are typed; the
/// rest pass through opaquely so a schema bump on the rt side doesn't break
/// this command.
#[derive(Serialize, Default, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub struct BaselineEntry {
    pub operation: String,
    pub wave: u32,
    pub captured_at: String,
    pub duration_ms: i64,
    pub from_history: bool,
}

/// Aggregated payload returned by `economy_summary`.
#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "snake_case")]
pub struct EconomySummary {
    /// Sum of `savings_tokens` across every wave.
    pub total_savings_tokens: i64,
    /// Per-wave breakdown, sorted ascending by `wave_id` for the table + the
    /// sparkline. An empty vec is the empty-state signal for the UI.
    pub per_wave: Vec<WaveSavings>,
    /// Operational baselines captured via `mustard-rt run economy report`.
    pub baselines: Vec<BaselineEntry>,
    /// Total number of baseline entries (mirrors the report.total field).
    pub baseline_total: usize,
    /// Best-effort diagnostic — non-empty when the rt CLI couldn't be reached
    /// or the NDJSON walk surfaced an issue. The frontend can surface this in
    /// a subtle subtitle without failing the page.
    pub notes: Vec<String>,
}

/// Walk every `.claude/spec/*/.events/*.ndjson` file under `repo`, filtering
/// for `pipeline.economy.savings.wave` events (emitted by `tracker.rs` in
/// W3A wave-13-rt). Aggregates by `payload.wave_id`. Returns an empty vec +
/// optional note when the spec directory is absent.
fn per_wave_from_events(repo: &Path) -> (Vec<WaveSavings>, Option<String>) {
    let Ok(paths) = ClaudePaths::for_project(repo) else {
        return (
            Vec::new(),
            Some(format!("claude_paths resolution failed for {}", repo.display())),
        );
    };
    let spec_root = paths.spec_dir();
    let Ok(entries) = std::fs::read_dir(&spec_root) else {
        return (
            Vec::new(),
            Some(format!("no spec dir at {}", spec_root.display())),
        );
    };

    // Aggregate (wave_id, distinct ops, sum tokens). Streams every
    // `.events/*.ndjson` file under each spec dir rather than loading them
    // up-front — keeps memory bounded even on long-lived repos.
    let mut by_wave: BTreeMap<String, (i64, std::collections::BTreeSet<String>)> = BTreeMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let events_dir = path.join(".events");
        let Ok(files) = std::fs::read_dir(&events_dir) else {
            continue;
        };
        for file in files.flatten() {
            let fp = file.path();
            if fp.extension().and_then(|s| s.to_str()) != Some("ndjson") {
                continue;
            }
            for event in EventReader::stream(&fp) {
                if event.kind != "pipeline.economy.savings.wave" {
                    continue;
                }
                let payload = &event.payload;
                let wave_id = match payload.get("wave_id").and_then(Value::as_str) {
                    Some(w) => w.to_string(),
                    None => continue,
                };
                let saved = payload
                    .get("savings_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let operation = payload
                    .get("operation")
                    .and_then(Value::as_str)
                    .unwrap_or("default")
                    .to_string();

                let bucket = by_wave.entry(wave_id).or_default();
                bucket.0 += saved;
                bucket.1.insert(operation);
            }
        }
    }

    let rows: Vec<WaveSavings> = by_wave
        .into_iter()
        .map(|(wave_id, (savings_tokens, ops))| WaveSavings {
            wave_id,
            savings_tokens,
            operations: i64::try_from(ops.len()).unwrap_or(0),
        })
        .collect();

    (rows, None)
}

/// Shell to `mustard-rt run economy report --format json` and parse stdout.
/// Returns `(entries, total)`. Fail-open per the spawn / parse layer.
fn baselines_from_rt(repo: &Path) -> (Vec<BaselineEntry>, usize, Option<String>) {
    let args: &[&str] = &["run", "economy", "report", "--format", "json"];
    let mut cmd = mustard_rt_command(args);
    cmd.current_dir(repo);
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return (Vec::new(), 0, Some(format!("spawn mustard-rt: {e}"))),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = slice_json(&stdout);
    let parsed: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return (Vec::new(), 0, Some("malformed economy report json".to_string())),
    };
    let total = parsed.get("total").and_then(Value::as_u64).unwrap_or(0) as usize;
    let entries: Vec<BaselineEntry> = parsed
        .get("entries")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    Some(BaselineEntry {
                        operation: e.get("operation").and_then(Value::as_str)?.to_string(),
                        wave: e.get("wave").and_then(Value::as_u64).unwrap_or(0) as u32,
                        captured_at: e
                            .get("captured_at")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        duration_ms: e.get("duration_ms").and_then(Value::as_i64).unwrap_or(0),
                        from_history: e
                            .get("from_history")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    (entries, total, None)
}

/// `economy_summary` — Tauri command. Returns the merged payload.
///
/// Fail-open: the function never returns `Err` for missing data; it surfaces
/// degradation through `EconomySummary::notes` so the dashboard can render a
/// subtle hint without breaking the page.
#[tauri::command]
pub fn economy_summary(repo_path: String) -> Result<EconomySummary, String> {
    let repo = PathBuf::from(&repo_path);
    let mut notes: Vec<String> = Vec::new();

    let (per_wave, note_db) = per_wave_from_events(&repo);
    if let Some(n) = note_db {
        notes.push(n);
    }
    let total_savings_tokens: i64 = per_wave.iter().map(|w| w.savings_tokens).sum();

    let (baselines, baseline_total, note_rt) = baselines_from_rt(&repo);
    if let Some(n) = note_rt {
        notes.push(n);
    }

    Ok(EconomySummary {
        total_savings_tokens,
        per_wave,
        baselines,
        baseline_total,
        notes,
    })
}

// ── helpers shared with the rest of the dashboard ────────────────────────────

/// Build a `Command` that invokes `mustard-rt`. Windows uses `cmd /C` so PATH
/// resolution matches every other dashboard caller (see `spec_views.rs`).
fn mustard_rt_command(args: &[&str]) -> std::process::Command {
    #[cfg(target_os = "windows")]
    {
        let mut c = crate::process_util::no_window_command("cmd");
        let mut full: Vec<&str> = vec!["/C", "mustard-rt"];
        full.extend_from_slice(args);
        c.args(&full);
        c
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut c = crate::process_util::no_window_command("mustard-rt");
        c.args(args);
        c
    }
}

/// Trim leading RTK / log noise so `serde_json::from_str` sees a clean JSON
/// document starting at the first `{`.
fn slice_json(stdout: &str) -> &str {
    match stdout.find('{') {
        Some(i) => &stdout[i..],
        None => stdout,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_spec_dir_returns_empty_per_wave_with_note() {
        let dir = tempdir().unwrap();
        let (rows, note) = per_wave_from_events(dir.path());
        assert!(rows.is_empty());
        assert!(
            note.is_some(),
            "expected a note when no spec dir exists"
        );
    }

    #[test]
    fn per_wave_aggregates_savings_from_ndjson() {
        let dir = tempdir().unwrap();
        let spec_a = dir
            .path()
            .join(".claude")
            .join("spec")
            .join("alpha")
            .join(".events");
        std::fs::create_dir_all(&spec_a).unwrap();
        // Two wave-W0 entries with two distinct operations + one wave-W1
        // entry. Reader should collapse by wave_id and count distinct ops.
        let lines = vec![
            r#"{"kind":"pipeline.economy.savings.wave","payload":{"wave_id":"W0","savings_tokens":1000,"operation":"scan-rust-first"}}"#,
            r#"{"kind":"pipeline.economy.savings.wave","payload":{"wave_id":"W0","savings_tokens":500,"operation":"templates-md-moat"}}"#,
            r#"{"kind":"pipeline.economy.savings.wave","payload":{"wave_id":"W1","savings_tokens":2000,"operation":"sub-spec-link"}}"#,
        ];
        std::fs::write(spec_a.join("seed.ndjson"), lines.join("\n")).unwrap();

        let (rows, note) = per_wave_from_events(dir.path());
        assert!(note.is_none(), "no note expected on a healthy spec tree");
        assert_eq!(rows.len(), 2);
        let w0 = rows.iter().find(|r| r.wave_id == "W0").unwrap();
        assert_eq!(w0.savings_tokens, 1500);
        assert_eq!(w0.operations, 2);
        let w1 = rows.iter().find(|r| r.wave_id == "W1").unwrap();
        assert_eq!(w1.savings_tokens, 2000);
        assert_eq!(w1.operations, 1);
    }

    #[test]
    fn summary_total_is_sum_of_per_wave() {
        let rows = vec![
            WaveSavings {
                wave_id: "W0".to_string(),
                savings_tokens: 100,
                operations: 1,
            },
            WaveSavings {
                wave_id: "W1".to_string(),
                savings_tokens: 250,
                operations: 2,
            },
        ];
        let total: i64 = rows.iter().map(|r| r.savings_tokens).sum();
        assert_eq!(total, 350);
    }

    #[test]
    fn baseline_entry_serializes_required_fields() {
        let e = BaselineEntry {
            operation: "verify".to_string(),
            wave: 1,
            captured_at: "T".to_string(),
            duration_ms: 42,
            from_history: true,
        };
        let v = serde_json::to_value(e).unwrap();
        for f in ["operation", "wave", "captured_at", "duration_ms", "from_history"] {
            assert!(v.get(f).is_some(), "missing field {f}");
        }
    }
}
