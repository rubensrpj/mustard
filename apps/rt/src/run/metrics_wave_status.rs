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

use crate::run::complete_spec::parse_iso_millis;
use crate::run::env::project_dir;
use crate::run::memory_cross_wave;
use mustard_core::fs;
use mustard_core::projection::read_harness_events_from_ndjson_dir;
use mustard_core::store::sqlite_store::SqliteEventStore;
use rusqlite::{Connection, params};
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

/// Aggregate events for a single wave name.
///
/// W5 split: `pipeline.status` lives in `pipeline_events` (SQLite), while
/// `token.saved` / `retry.attempt` live in the per-spec NDJSON sink. The wave
/// pipeline is identified by `payload.pipeline = wave_name`; spec attribution
/// on NDJSON events is by reading the wave-name's own per-spec dir (the wave
/// "spec" slug is the wave name itself).
fn aggregate_wave(
    conn: &Connection,
    project: &Path,
    wave_name: &str,
    model: Option<String>,
    cross_wave_bytes: usize,
) -> WaveStatus {
    // status: last `pipeline.status` event for this wave's pipeline. Now in
    // `pipeline_events` (W5 lifecycle index).
    let status: Option<String> = conn
        .query_row(
            "SELECT json_extract(payload, '$.to') FROM pipeline_events \
             WHERE kind = 'pipeline.status' \
               AND json_extract(payload, '$.pipeline') = ?1 \
             ORDER BY id DESC LIMIT 1",
            params![wave_name],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten();

    // tokens_saved / retries / duration: walk the per-spec NDJSON dir for the
    // wave name. Wave events are written under `<project>/.claude/spec/<wave_name>/events/`.
    let dir = project
        .join(".claude")
        .join("spec")
        .join(wave_name)
        .join("events");
    let events = read_harness_events_from_ndjson_dir(&dir);
    let mut tokens_saved: i64 = 0;
    let mut retries: i64 = 0;
    let mut min_ts: Option<String> = None;
    let mut max_ts: Option<String> = None;
    for ev in events {
        let pipeline = ev
            .payload
            .get("pipeline")
            .and_then(Value::as_str)
            .unwrap_or("");
        if pipeline != wave_name {
            continue;
        }
        // Track min/max ts for duration.
        if min_ts.as_deref().is_none_or(|t| ev.ts.as_str() < t) {
            min_ts = Some(ev.ts.clone());
        }
        if max_ts.as_deref().is_none_or(|t| ev.ts.as_str() > t) {
            max_ts = Some(ev.ts.clone());
        }
        if ev.event == "token.saved" {
            if let Some(saved) = ev.payload.get("saved").and_then(Value::as_i64) {
                tokens_saved += saved;
            }
        } else if ev.event == "retry.attempt" {
            retries += 1;
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

/// Open a fresh rusqlite connection to the project store. `None` when the
/// store cannot be opened.
fn open_conn(project: &Path) -> Option<Connection> {
    let store = SqliteEventStore::for_project(project).ok()?;
    let db_path = store.path().to_path_buf();
    let conn = Connection::open(&db_path).ok()?;
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    Some(conn)
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
    let parent_dir = project
        .join(".claude")
        .join("spec")
        .join(parent);

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

    let waves: Vec<Value> = if let Some(conn) = open_conn(project) {
        wave_names
            .iter()
            .enumerate()
            .map(|(idx, name)| {
                let n = wave_number(name);
                let n = if n == 0 { (idx + 1) as u32 } else { n };
                let model = models.get(name).cloned().flatten();
                let bytes = cross_wave_bytes_for(project, &wave_names, n, parent);
                serde_json::to_value(aggregate_wave(&conn, project, name, model, bytes))
                    .unwrap_or(Value::Null)
            })
            .collect()
    } else {
        wave_names
            .iter()
            .map(|name| {
                let model = models.get(name).cloned().flatten();
                serde_json::to_value(WaveStatus {
                    name: name.clone(),
                    status: None,
                    tokens_saved: 0,
                    duration_ms: 0,
                    retries: 0,
                    cross_wave_memory_bytes: 0,
                    model,
                })
                .unwrap_or(Value::Null)
            })
            .collect()
    };

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
    use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use tempfile::tempdir;

    fn ev(event: &str, ts: &str, pipeline: &str, extra: Value) -> HarnessEvent {
        let mut payload = json!({ "pipeline": pipeline });
        if let Some(map) = extra.as_object() {
            for (k, v) in map {
                payload[k] = v.clone();
            }
        }
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.to_string(),
            session_id: "s".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Hook,
                id: None,
                actor_type: None,
            },
            event: event.to_string(),
            payload,
            spec: None,
        }
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
    fn aggregates_per_wave() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        // Place mustard.db where `SqliteEventStore::for_project` would resolve it,
        // even though we open it explicitly via `for_project` below.
        let store = SqliteEventStore::for_project(project).unwrap();

        // Pipeline-status events land in SQLite via `append_pipeline_event` (W5).
        store
            .append_pipeline_event(
                "2026-05-20T10:00:00.000Z",
                Some("s-test"),
                Some("wave-1-rt-infra"),
                None,
                "pipeline.status",
                None,
                Some(r#"{"pipeline":"wave-1-rt-infra","to":"completed"}"#),
            )
            .unwrap();
        store
            .append_pipeline_event(
                "2026-05-20T11:00:00.000Z",
                Some("s-test"),
                Some("wave-2-skill-template"),
                None,
                "pipeline.status",
                None,
                Some(r#"{"pipeline":"wave-2-skill-template","to":"draft"}"#),
            )
            .unwrap();

        // Token-savings and retry events land in the per-spec NDJSON sink (W5).
        let _ = ev; // suppress dead-code warning on the legacy helper.
        let route = |event: &str, ts: &str, pipeline: &str, extra: Value| {
            let mut payload = json!({ "pipeline": pipeline });
            if let Some(map) = extra.as_object() {
                for (k, v) in map {
                    payload[k] = v.clone();
                }
            }
            let evt = HarnessEvent {
                v: SCHEMA_VERSION,
                ts: ts.to_string(),
                session_id: "s-test".to_string(),
                wave: 0,
                actor: Actor {
                    kind: ActorKind::Hook,
                    id: None,
                    actor_type: None,
                },
                event: event.to_string(),
                payload,
                spec: Some(pipeline.to_string()),
            };
            crate::run::event_route::emit(project.to_str().unwrap(), &evt);
        };
        route("token.saved", "2026-05-20T10:00:05.000Z", "wave-1-rt-infra", json!({ "saved": 100 }));
        route("token.saved", "2026-05-20T10:00:10.000Z", "wave-1-rt-infra", json!({ "saved": 200 }));
        route("retry.attempt", "2026-05-20T10:01:00.000Z", "wave-1-rt-infra", json!({}));
        route("token.saved", "2026-05-20T11:00:05.000Z", "wave-2-skill-template", json!({ "saved": 50 }));

        let conn = Connection::open(store.path()).unwrap();
        let w1 = aggregate_wave(&conn, project, "wave-1-rt-infra", Some("opus".into()), 0);
        assert_eq!(w1.status.as_deref(), Some("completed"));
        assert_eq!(w1.tokens_saved, 300);
        assert_eq!(w1.retries, 1);
        assert_eq!(w1.model.as_deref(), Some("opus"));
        // Duration spans the min/max of the NDJSON events (10:00:05 → 10:01:00).
        assert!(w1.duration_ms >= 55_000);

        let w2 = aggregate_wave(&conn, project, "wave-2-skill-template", Some("opus".into()), 0);
        assert_eq!(w2.status.as_deref(), Some("draft"));
        assert_eq!(w2.tokens_saved, 50);
        assert_eq!(w2.retries, 0);
    }
}
