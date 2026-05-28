//! `mustard-rt run metrics wave-status` — per-wave status + telemetry roll-up
//! for a parent (epic) spec.
//!
//! Part of the wave-network spec (`2026-05-20-mustard-wave-network-standard`).
//! Aggregates the harness event log into one JSON document per wave, so the
//! dashboard can render the parent → waves hierarchy without summing across
//! unrelated specs (the wave-network spec § "Métricas funcionais agrupadas
//! por parent").
//!
//! Per-wave shape:
//!
//! ```json
//! {
//!   "name": "wave-1-rt-infra",
//!   "status": "completed",
//!   "tokens_saved": 1234,
//!   "duration_ms": 56789,
//!   "retries": 0,
//!   "cross_wave_memory_bytes": 0,
//!   "model": "opus"
//! }
//! ```
//!
//! Wave detection: first read `<parent>/wave-plan.md` and parse the `Tabela de
//! Waves` table (Spec column + Modelo column); if no plan exists, fall back to
//! globbing every `wave-*-*/` directory under
//! `.claude/spec/<parent>/` (flat layout) and treating the folder name as the wave
//! name.
//!
//! Fail-open: a missing parent dir, missing DB, or missing wave-plan all
//! degrade to an empty `waves` array — never a non-zero exit.

use crate::run::spec::complete_spec::parse_iso_millis;
use crate::shared::context::project_dir;
use crate::run::knowledge::memory_cross_wave;
use mustard_core::fs;
use mustard_core::ClaudePaths;
use mustard_core::EventReader;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One wave's aggregated row.
#[derive(Debug, Serialize)]
struct WaveStatus {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    tokens_saved: i64,
    duration_ms: i64,
    retries: i64,
    cross_wave_memory_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
}

/// Parse the wave-plan table into ordered `(name, model)` pairs. The `Modelo`
/// column is detected by header position when present.
fn parse_plan_rows(wave_plan_text: &str) -> Vec<(String, Option<String>)> {
    let mut header_cells: Vec<String> = Vec::new();
    let mut model_col: Option<usize> = None;
    let mut data_rows: Vec<Vec<String>> = Vec::new();

    for raw_line in wave_plan_text.split('\n') {
        let line = raw_line.trim_end_matches('\r');
        let Some(rest) = line.strip_prefix('|') else {
            continue;
        };
        let cells: Vec<String> = rest
            .split('|')
            .map(|c| c.trim().to_string())
            .collect();
        if cells.is_empty() {
            continue;
        }
        // Separator row: every non-empty cell consists of `-` / `:` only.
        if cells.iter().all(|c| {
            c.is_empty() || c.chars().all(|ch| ch == '-' || ch == ':')
        }) {
            continue;
        }
        // First non-separator row: header. Subsequent rows: data.
        if header_cells.is_empty() {
            header_cells = cells;
            model_col = header_cells
                .iter()
                .position(|c| c.to_lowercase().starts_with("modelo") || c.eq_ignore_ascii_case("model"));
            continue;
        }
        // Skip rows whose label (first cell) is not a wave number.
        let label = cells[0].to_lowercase();
        let label_body: &str = label
            .strip_prefix('w')
            .map_or(&label, str::trim_start);
        let label_body: &str = label_body
            .strip_prefix("ave")
            .map_or(label_body, str::trim_start);
        if label_body.is_empty() || !label_body.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        data_rows.push(cells);
    }

    let mut out: Vec<(String, Option<String>)> = Vec::new();
    for row in data_rows {
        // Spec column is at index 1 in our standard layout; tolerate shorter
        // rows by skipping.
        let Some(spec_cell) = row.get(1) else { continue };
        let Some(name) = strip_wikilink(spec_cell) else {
            continue;
        };
        let model = model_col
            .and_then(|i| row.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "—" && s != "-");
        out.push((name, model));
    }
    out
}

/// Strip `[[…]]` from a wikilink cell.
fn strip_wikilink(raw: &str) -> Option<String> {
    let t = raw.trim();
    let inner = t.strip_prefix("[[").and_then(|s| s.strip_suffix("]]"))?;
    let inner = inner.trim();
    if inner.is_empty() {
        None
    } else {
        Some(inner.to_string())
    }
}

/// Wave number embedded in a `wave-N-…` name, defaulting to `0` for sort.
fn wave_number(name: &str) -> u32 {
    let after = name.strip_prefix("wave-").unwrap_or(name);
    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().unwrap_or(0)
}

/// Glob fallback when wave-plan is absent: list every `wave-*-*` directory.
fn fallback_wave_dirs(parent_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(parent_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .into_iter()
        .filter(|e| e.is_dir)
        .map(|e| e.file_name)
        .filter(|n| {
            let lc = n.to_lowercase();
            lc.starts_with("wave-")
                && lc[5..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit())
        })
        .collect();
    names.sort_by_key(|n| wave_number(n));
    names
}

/// Aggregate events for a single wave name via the per-spec NDJSON log.
///
/// Reads the wave's events dir at
/// `<project>/.claude/spec/<wave_name>/.events/` and derives:
/// - `status` — last `pipeline.status` event's `to` field.
/// - `tokens_saved` / `retries` / `duration_ms` — folded from `token.saved`
///   and `retry.attempt` events attributed to this wave pipeline.
///
/// Fail-open: a missing events dir or unreadable file yields zeroed counters.
fn aggregate_wave(
    project: &Path,
    wave_name: &str,
    model: Option<String>,
    cross_wave_bytes: usize,
) -> WaveStatus {
    let events_dir = ClaudePaths::for_project(project)
        .ok()
        .and_then(|p| p.for_spec(wave_name).ok())
        .map(|sp| sp.events_dir())
        .unwrap_or_default();

    // Collect all NDJSON events in the wave's events dir.
    let ndjson_events: Vec<_> = collect_ndjson_events(&events_dir);

    // status: last `pipeline.status` event's `to` field (matches
    // chronological sort via ISO-8601 ts comparison).
    let status: Option<String> = {
        let mut last_status: Option<String> = None;
        let mut last_ts: Option<String> = None;
        for ev in &ndjson_events {
            if ev.kind != "pipeline.status" {
                continue;
            }
            let ts = ev.raw.get("ts").and_then(|v| v.as_str()).map(str::to_string);
            let is_later = match (last_ts.as_deref(), ts.as_deref()) {
                (None, _) => true,
                (Some(a), Some(b)) => b > a,
                _ => false,
            };
            if is_later {
                last_ts = ts;
                last_status = ev
                    .payload
                    .get("to")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
            }
        }
        last_status
    };

    // tokens_saved / retries / duration from NDJSON events attributed to
    // this wave pipeline (`payload.pipeline == wave_name`).
    let mut tokens_saved: i64 = 0;
    let mut retries: i64 = 0;
    let mut min_ts: Option<String> = None;
    let mut max_ts: Option<String> = None;

    for ev in &ndjson_events {
        let pipeline = ev
            .payload
            .get("pipeline")
            .and_then(Value::as_str)
            .unwrap_or("");
        if pipeline != wave_name {
            continue;
        }
        let ts_str = ev
            .raw
            .get("ts")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if let Some(ref ts) = ts_str {
            if min_ts.as_deref().is_none_or(|t| ts.as_str() < t) {
                min_ts = Some(ts.clone());
            }
            if max_ts.as_deref().is_none_or(|t| ts.as_str() > t) {
                max_ts = Some(ts.clone());
            }
        }
        match ev.kind.as_str() {
            "token.saved" => {
                if let Some(saved) = ev.payload.get("saved").and_then(Value::as_i64) {
                    tokens_saved += saved;
                }
            }
            "retry.attempt" => retries += 1,
            _ => {}
        }
    }

    let duration_ms = match (min_ts.as_deref(), max_ts.as_deref()) {
        (Some(a), Some(b)) => match (parse_iso_millis(a), parse_iso_millis(b)) {
            (Some(sa), Some(eb)) => (eb - sa).max(0),
            _ => 0,
        },
        _ => 0,
    };

    WaveStatus {
        name: wave_name.to_string(),
        status,
        tokens_saved,
        duration_ms,
        retries,
        cross_wave_memory_bytes: cross_wave_bytes,
        model,
    }
}

/// Collect all [`mustard_core::Event`]s from every `.ndjson` file in `dir`.
///
/// Returns an empty `Vec` when the dir is absent or unreadable (fail-open).
fn collect_ndjson_events(events_dir: &Path) -> Vec<mustard_core::Event> {
    let Ok(rd) = std::fs::read_dir(events_dir) else {
        return Vec::new();
    };
    let mut out: Vec<mustard_core::Event> = Vec::new();
    for entry in rd.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("ndjson") {
            continue;
        }
        out.extend(EventReader::stream(&p));
    }
    out
}

/// Compute the byte length of the cross-wave memory markdown that would land
/// in wave N's agent prompt — exactly what `memory cross-wave --wave N` would
/// emit. Returns 0 for wave 1 (no prior waves) or when nothing is in memory.
fn cross_wave_bytes_for(
    project: &Path,
    all_wave_names: &[String],
    n: u32,
    spec: &str,
) -> usize {
    if n <= 1 {
        return 0;
    }
    let n_prior = (n as usize).saturating_sub(1).min(all_wave_names.len());
    let prior: Vec<String> = all_wave_names.iter().take(n_prior).cloned().collect();
    memory_cross_wave::render(&prior, project, spec).len()
}

/// Build the full result JSON for `--spec <parent>`.
fn build_result(project: &Path, parent: &str) -> Value {
    let Ok(cp) = ClaudePaths::for_project(project) else {
        return json!({"error": "invalid_project_root"});
    };
    let Ok(sp) = cp.for_spec(parent) else {
        return json!({"error": "invalid_spec_name"});
    };
    let parent_dir = sp.dir().to_path_buf();

    // Detect waves: wave-plan first, fallback to dir glob.
    let plan_text = fs::read_to_string(parent_dir.join("wave-plan.md")).unwrap_or_default();
    let plan_rows = parse_plan_rows(&plan_text);
    let (wave_names, models): (Vec<String>, BTreeMap<String, Option<String>>) =
        if plan_rows.is_empty() {
            let names = fallback_wave_dirs(&parent_dir);
            let mut map = BTreeMap::new();
            for n in &names {
                map.insert(n.clone(), None);
            }
            (names, map)
        } else {
            let names: Vec<String> = plan_rows.iter().map(|(n, _)| n.clone()).collect();
            let mut map = BTreeMap::new();
            for (n, m) in plan_rows {
                map.insert(n, m);
            }
            (names, map)
        };

    let waves: Vec<Value> = wave_names
        .iter()
        .enumerate()
        .map(|(idx, name)| {
            let n = wave_number(name);
            let n = if n == 0 { (idx + 1) as u32 } else { n };
            let model = models.get(name).cloned().flatten();
            let bytes = cross_wave_bytes_for(project, &wave_names, n, parent);
            serde_json::to_value(aggregate_wave(project, name, model, bytes))
                .unwrap_or(Value::Null)
        })
        .collect();

    json!({ "parent": parent, "waves": waves })
}

/// Run the `wave-status` metrics subcommand. Args follow the `metrics`
/// dispatcher contract: a trailing `--spec <parent>` flag.
pub fn run(args: &[String]) {
    // Tolerate `--help` so the AC grep matches.
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: metrics wave-status --spec <parent>");
        println!("  --spec <parent>   parent spec name (under .claude/spec/, flat layout)");
        return;
    }

    let mut spec: Option<String> = None;
    let mut i = 0usize;
    while i < args.len() {
        if args[i] == "--spec" {
            spec = args.get(i + 1).cloned();
            i += 2;
            continue;
        }
        i += 1;
    }

    let Some(parent) = spec else {
        eprintln!("Usage: metrics wave-status --spec <parent>");
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "parent": null, "waves": [] }))
                .unwrap_or_else(|_| "{}".to_string())
        );
        return;
    };

    let project = PathBuf::from(project_dir());
    let result = build_result(&project, &parent);
    println!(
        "{}",
        serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string())
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::io::Write as _;

    /// Write one NDJSON line into `<events_dir>/test.ndjson`.
    fn write_ndjson_event(events_dir: &Path, kind: &str, ts: &str, payload: Value) {
        std::fs::create_dir_all(events_dir).unwrap();
        let line = format!(
            "{}\n",
            json!({ "kind": kind, "ts": ts, "payload": payload })
        );
        let path = events_dir.join("test.ndjson");
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        f.write_all(line.as_bytes()).unwrap();
    }

    /// Create a `ClaudePaths`-compatible events dir for `wave_name` under
    /// `<project>/.claude/spec/<wave_name>/.events/`.
    fn events_dir_for(project: &Path, wave_name: &str) -> PathBuf {
        // ClaudePaths::for_project requires mustard.json at root.
        std::fs::write(project.join("mustard.json"), "{}").unwrap_or(());
        project
            .join(".claude")
            .join("spec")
            .join(wave_name)
            .join(".events")
    }

    #[test]
    fn parse_plan_rows_reads_model_column() {
        let plan = "\
| Wave | Spec                  | Role    | Modelo | Status |
|------|-----------------------|---------|--------|--------|
| 1    | [[wave-1-rt-infra]]   | general | opus   | draft  |
| 2    | [[wave-2-skill-tpl]]  | general | sonnet | queued |
";
        let rows = parse_plan_rows(plan);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "wave-1-rt-infra");
        assert_eq!(rows[0].1.as_deref(), Some("opus"));
        assert_eq!(rows[1].0, "wave-2-skill-tpl");
        assert_eq!(rows[1].1.as_deref(), Some("sonnet"));
    }

    #[test]
    fn wave_number_strips_prefix() {
        assert_eq!(wave_number("wave-1-rt-infra"), 1);
        assert_eq!(wave_number("wave-12-x"), 12);
        assert_eq!(wave_number("misc"), 0);
    }

    #[test]
    fn aggregates_per_wave_from_ndjson() {
        let dir = tempdir().unwrap();
        let project = dir.path();

        // Seed NDJSON events for wave-1-rt-infra.
        let ev1_dir = events_dir_for(project, "wave-1-rt-infra");
        write_ndjson_event(&ev1_dir, "pipeline.status", "2026-05-20T10:00:00.000Z",
            json!({ "pipeline": "wave-1-rt-infra", "to": "completed" }));
        write_ndjson_event(&ev1_dir, "token.saved", "2026-05-20T10:00:05.000Z",
            json!({ "pipeline": "wave-1-rt-infra", "saved": 100 }));
        write_ndjson_event(&ev1_dir, "token.saved", "2026-05-20T10:00:10.000Z",
            json!({ "pipeline": "wave-1-rt-infra", "saved": 200 }));
        write_ndjson_event(&ev1_dir, "retry.attempt", "2026-05-20T10:01:00.000Z",
            json!({ "pipeline": "wave-1-rt-infra" }));

        // Seed NDJSON events for wave-2-skill-template.
        let ev2_dir = events_dir_for(project, "wave-2-skill-template");
        write_ndjson_event(&ev2_dir, "pipeline.status", "2026-05-20T11:00:00.000Z",
            json!({ "pipeline": "wave-2-skill-template", "to": "draft" }));
        write_ndjson_event(&ev2_dir, "token.saved", "2026-05-20T11:00:05.000Z",
            json!({ "pipeline": "wave-2-skill-template", "saved": 50 }));

        let w1 = aggregate_wave(project, "wave-1-rt-infra", Some("opus".into()), 0);
        assert_eq!(w1.status.as_deref(), Some("completed"));
        assert_eq!(w1.tokens_saved, 300);
        assert_eq!(w1.retries, 1);
        assert_eq!(w1.model.as_deref(), Some("opus"));
        // Duration spans the min/max of the token/retry events (10:00:05 → 10:01:00).
        assert!(w1.duration_ms >= 55_000);

        let w2 = aggregate_wave(project, "wave-2-skill-template", Some("opus".into()), 0);
        assert_eq!(w2.status.as_deref(), Some("draft"));
        assert_eq!(w2.tokens_saved, 50);
        assert_eq!(w2.retries, 0);
    }
}
