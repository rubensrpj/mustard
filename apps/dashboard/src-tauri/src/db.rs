use rusqlite::{Connection, OpenFlags, params};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    ActivePipeline, ActivityGroup, AgentUsage, ConsumptionSummary, DailyPoint, KnowledgeRow,
    KnowledgeSummary, MetricsSummary, ModelUsage, PhaseTokens, PipelineSummary, QualityMetrics,
    RecentEvent, RoleQuality, SlowestWave, SpecRow, SpecUsage,
};

/// Open a SQLite connection in read-only mode. Returns an error if the file
/// does not exist on disk (rusqlite would otherwise create it with default flags).
pub fn open_readonly(db_path: &Path) -> Result<Connection, String> {
    if !db_path.exists() {
        return Err("db not found".to_string());
    }
    Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())
}

/// Return true if the connection has at least 3 of the 4 expected Phase 1 tables
/// (events, specs, knowledge, spans). Tolerant to partial Phase 1.
pub fn has_phase1_schema(conn: &Connection) -> bool {
    let mut stmt = match conn.prepare(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type='table' AND name IN ('events','specs','knowledge','spans')",
    ) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let count: i64 = match stmt.query_row([], |row| row.get(0)) {
        Ok(n) => n,
        Err(_) => return false,
    };
    count >= 3
}

/// Try to run a closure against the SQLite reader. Returns `None` when the DB
/// is missing, unreadable, or doesn't expose the Phase 1 schema — signalling
/// the caller to fall back to the legacy JSONL/JSON readers.
pub fn with_db<T, F>(repo_path: &Path, f: F) -> Option<Result<T, String>>
where
    F: FnOnce(&Connection) -> Result<T, String>,
{
    let db_path = repo_path.join(".claude").join(".harness").join("mustard.db");
    if !db_path.exists() {
        return None;
    }
    let conn = match open_readonly(&db_path) {
        Ok(c) => c,
        Err(_) => return None,
    };
    if !has_phase1_schema(&conn) {
        return None;
    }
    Some(f(&conn))
}

/// Escape a free-text query for FTS5 MATCH. Returns `None` for empty input
/// (caller should short-circuit to an empty result set). Wraps in double
/// quotes (doubling any internal quotes) whenever the query contains a char
/// that FTS5 would otherwise interpret as syntax.
pub fn fts_escape(q: &str) -> Option<String> {
    let trimmed = q.trim();
    if trimmed.is_empty() {
        return None;
    }
    let needs_quote = trimmed
        .chars()
        .any(|c| matches!(c, '\'' | '"' | '\t' | '*' | '-' | ':' | '(' | ')' | '`' | ' '));
    if needs_quote {
        let escaped = trimmed.replace('"', "\"\"");
        Some(format!("\"{}\"", escaped))
    } else {
        Some(trimmed.to_string())
    }
}

/// Milliseconds since UNIX epoch for today's UTC midnight.
fn utc_midnight_ms_today() -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let day = now / 86_400;
    day * 86_400 * 1000
}

pub fn metrics_from_db(conn: &Connection) -> Result<MetricsSummary, String> {
    let total_events: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;

    let sessions_recent: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT session_id) FROM events WHERE ts >= datetime('now', '-7 days')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let agents_dispatched: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event='agent.start'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let last_event_at: Option<String> = conn
        .query_row("SELECT MAX(ts) FROM events", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;

    let tokens_total: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(COALESCE(input_tokens,0) + COALESCE(output_tokens,0)), 0) FROM spans",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let midnight_ms = utc_midnight_ms_today();
    let tokens_today: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(COALESCE(input_tokens,0) + COALESCE(output_tokens,0)), 0) \
             FROM spans WHERE started_at >= ?1",
            [midnight_ms],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    Ok(MetricsSummary {
        total_events: total_events as usize,
        sessions_recent: sessions_recent as usize,
        agents_dispatched: agents_dispatched as usize,
        last_event_at,
        tokens_total: tokens_total as u64,
        tokens_today: tokens_today as u64,
    })
}

pub fn knowledge_from_db(conn: &Connection) -> Result<KnowledgeSummary, String> {
    let mut stmt = conn
        .prepare("SELECT type, COUNT(*) FROM knowledge GROUP BY type")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let t: Option<String> = row.get(0)?;
            let n: i64 = row.get(1)?;
            Ok((t, n))
        })
        .map_err(|e| e.to_string())?;

    let mut patterns_count = 0usize;
    let mut conventions_count = 0usize;
    for r in rows {
        let (t, n) = r.map_err(|e| e.to_string())?;
        match t.as_deref() {
            Some("pattern") => patterns_count += n as usize,
            Some("convention") => conventions_count += n as usize,
            _ => {}
        }
    }

    let high_confidence_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM knowledge WHERE confidence >= 0.8",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    Ok(KnowledgeSummary {
        patterns_count,
        conventions_count,
        high_confidence_count: high_confidence_count as usize,
    })
}

fn summary_from_payload(payload: &Option<String>, event_type: &str) -> Option<String> {
    let raw = payload.as_deref()?;
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;

    // Event-specific summaries. qa-run.js emits `{ spec, overall, criteria }`
    // with `overall: "pass"|"fail"|"skip"` — none of the generic summary keys
    // match, so without this branch the dashboard sees `summary: null` and
    // parseQaOverall (frontend) can't distinguish pass/fail/skip.
    if event_type == "qa.result" {
        let overall = v.get("overall").and_then(|x| x.as_str());
        if let Some(o) = overall {
            // Include failed AC count when available so the summary stays
            // informative even after the frontend's parseQaOverall extracts
            // the verdict.
            let criteria = v.get("criteria").and_then(|c| c.as_array());
            let fail_count = criteria
                .map(|arr| {
                    arr.iter()
                        .filter(|c| {
                            c.get("result").and_then(|r| r.as_str()) == Some("fail")
                        })
                        .count()
                })
                .unwrap_or(0);
            return Some(if fail_count > 0 {
                format!("overall={} ({} failed)", o, fail_count)
            } else {
                format!("overall={}", o)
            });
        }
    }

    for key in &["summary", "description", "msg", "text"] {
        if let Some(s) = v.get(*key).and_then(|x| x.as_str()) {
            let trimmed = s.chars().take(80).collect::<String>();
            return Some(trimmed);
        }
    }
    None
}

fn extract_event_details(payload: &Option<String>, event_type: &str) -> (Option<String>, Option<String>) {
    let raw = match payload.as_deref() { Some(s) => s, None => return (None, None) };
    let v: serde_json::Value = match serde_json::from_str(raw) { Ok(v) => v, Err(_) => return (None, None) };
    // Mustard hooks emit payload as `{ tool, phase, target: {file|command|pattern|url|...} }`.
    // Some legacy hooks used `tool_name` / `tool_input.*` — keep both for compatibility.
    let tool_name = v.get("tool")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("tool_name").and_then(|x| x.as_str()))
        .map(|s| s.to_string());
    let target = if event_type == "agent.start" {
        v.get("agent_type").and_then(|x| x.as_str())
            .or_else(|| v.get("agentType").and_then(|x| x.as_str()))
            .map(|s| s.to_string())
    } else if event_type == "pipeline.phase" {
        v.get("phase").and_then(|x| x.as_str()).map(|s| s.to_string())
    } else {
        // Try modern shape first: payload.target.{file|command|pattern|url}
        let modern = v.get("target").and_then(|t| {
            t.get("file").and_then(|x| x.as_str())
                .or_else(|| t.get("command").and_then(|x| x.as_str()))
                .or_else(|| t.get("pattern").and_then(|x| x.as_str()))
                .or_else(|| t.get("url").and_then(|x| x.as_str()))
                .or_else(|| t.get("path").and_then(|x| x.as_str()))
        });
        // payload.target may also be a plain string in some events
        let target_str = if modern.is_none() {
            v.get("target").and_then(|x| x.as_str())
        } else { modern };
        // Legacy shape: payload.tool_input.{file_path|command|pattern|url}
        let legacy = v.get("tool_input").and_then(|ti| {
            ti.get("file_path").and_then(|x| x.as_str())
                .or_else(|| ti.get("command").and_then(|x| x.as_str()))
                .or_else(|| ti.get("pattern").and_then(|x| x.as_str()))
                .or_else(|| ti.get("url").and_then(|x| x.as_str()))
        });
        target_str.or(legacy).map(|s| s.to_string())
    };
    (tool_name, target)
}

fn row_to_event(
    event_type: String,
    spec: Option<String>,
    wave: Option<i64>,
    actor_kind: Option<String>,
    actor_id: Option<String>,
    ts: Option<String>,
    payload: Option<String>,
) -> RecentEvent {
    let summary = summary_from_payload(&payload, &event_type);
    let (tool_name, target) = extract_event_details(&payload, &event_type);
    let phase = payload
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v.get("phase").and_then(|x| x.as_str()).map(|s| s.to_ascii_uppercase()));
    RecentEvent {
        event_type,
        ts,
        summary,
        spec,
        wave,
        actor_kind,
        actor_id,
        tool_name,
        target,
        phase,
    }
}

pub fn recent_events_from_db(conn: &Connection, limit: usize) -> Result<Vec<RecentEvent>, String> {
    // Try rich SELECT first; fall back if columns are missing in older schemas.
    let rich_sql = "SELECT event, spec, wave, actor_kind, actor_id, ts, payload FROM events ORDER BY id DESC LIMIT ?1";
    let narrow_sql = "SELECT event, spec, ts, payload FROM events ORDER BY id DESC LIMIT ?1";

    let use_rich = conn.prepare(rich_sql).is_ok();

    let mut out = Vec::new();
    if use_rich {
        let mut stmt = conn.prepare(rich_sql).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        }).map_err(|e| e.to_string())?;
        for r in rows {
            let (et, spec, wave, ak, ai, ts, payload) = r.map_err(|e| e.to_string())?;
            out.push(row_to_event(et, spec, wave, ak, ai, ts, payload));
        }
    } else {
        let mut stmt = conn.prepare(narrow_sql).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        }).map_err(|e| e.to_string())?;
        for r in rows {
            let (et, spec, ts, payload) = r.map_err(|e| e.to_string())?;
            out.push(row_to_event(et, spec, None, None, None, ts, payload));
        }
    }
    Ok(out)
}

pub fn specs_from_db(conn: &Connection) -> Result<Vec<SpecRow>, String> {
    // `phase` is derived from the most-recent `pipeline.phase` event for the
    // spec (`payload.to`), NOT from the `specs.phase` projection column. The
    // event log is the canonical write path — `emit_phase.rs` and `post_edit`
    // both append `pipeline.phase` rows — so the latest event always reflects
    // the spec's true current phase. The correlated subquery mirrors
    // `mustard_core::emit_phase::last_phase_for_spec` (reverse-iterate, take
    // the freshest), using the `idx_events_spec`/`idx_events_event` indexes.
    let mut stmt = conn
        .prepare(
            "SELECT s.name, s.status, \
                    (SELECT json_extract(e.payload, '$.to') FROM events e \
                     WHERE e.event = 'pipeline.phase' AND e.spec = s.name \
                     ORDER BY e.id DESC LIMIT 1) AS phase, \
                    s.started_at, s.completed_at, s.affected_files \
             FROM specs s \
             ORDER BY COALESCE(s.completed_at, s.started_at) DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let name: String = row.get(0)?;
            let status: Option<String> = row.get(1)?;
            let phase: Option<String> = row.get(2)?;
            let started_at: Option<String> = row.get(3)?;
            let completed_at: Option<String> = row.get(4)?;
            let affected_raw: Option<String> = row.get(5)?;
            let affected_files: Vec<String> = affected_raw
                .as_deref()
                .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
                .unwrap_or_default();
            Ok(SpecRow {
                name,
                status,
                phase,
                started_at,
                completed_at,
                affected_files,
                bucket: None,
                parent: None,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

pub fn search_events_from_db(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<RecentEvent>, String> {
    let escaped = match fts_escape(query) {
        Some(q) => q,
        None => return Ok(vec![]),
    };

    let rich_sql = "SELECT e.event, e.spec, e.wave, e.actor_kind, e.actor_id, e.ts, e.payload \
                    FROM events_fts f JOIN events e ON f.rowid = e.id \
                    WHERE events_fts MATCH ?1 ORDER BY rank LIMIT ?2";
    let narrow_sql = "SELECT e.event, e.spec, e.ts, e.payload FROM events_fts f \
                      JOIN events e ON f.rowid = e.id \
                      WHERE events_fts MATCH ?1 ORDER BY rank LIMIT ?2";

    let use_rich = conn.prepare(rich_sql).is_ok();

    let mut out = Vec::new();
    if use_rich {
        let mut stmt = conn.prepare(rich_sql).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![escaped, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        }).map_err(|e| e.to_string())?;
        for r in rows {
            let (et, spec, wave, ak, ai, ts, payload) = r.map_err(|e| e.to_string())?;
            out.push(row_to_event(et, spec, wave, ak, ai, ts, payload));
        }
    } else {
        let mut stmt = conn.prepare(narrow_sql).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![escaped, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        }).map_err(|e| e.to_string())?;
        for r in rows {
            let (et, spec, ts, payload) = r.map_err(|e| e.to_string())?;
            out.push(row_to_event(et, spec, None, None, None, ts, payload));
        }
    }
    Ok(out)
}

pub fn workflow_by_phase_from_db(conn: &Connection) -> Result<crate::telemetry::WorkflowBlock, String> {
    let mut stmt = conn
        .prepare(
            "SELECT json_extract(payload, '$.phase') AS phase, COUNT(*) \
             FROM events WHERE event = 'pipeline.phase' \
             GROUP BY phase ORDER BY 2 DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let phase: Option<String> = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((phase, count))
        })
        .map_err(|e| e.to_string())?;

    let mut by_phase = Vec::new();
    for r in rows {
        let (phase, count) = r.map_err(|e| e.to_string())?;
        if let Some(phase) = phase {
            by_phase.push(crate::telemetry::PhaseCount { phase, count: count as u64 });
        }
    }
    Ok(crate::telemetry::WorkflowBlock { by_phase })
}

pub fn tool_breakdown_from_db(conn: &Connection, limit: usize) -> Result<Vec<crate::telemetry::ToolCount>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT COALESCE(json_extract(payload, '$.tool'), json_extract(payload, '$.tool_name')) AS tool, \
             COUNT(*) FROM events WHERE event = 'tool.use' \
             GROUP BY tool ORDER BY 2 DESC LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([limit as i64], |row| {
            let tool: Option<String> = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((tool, count))
        })
        .map_err(|e| e.to_string())?;

    let mut list = Vec::new();
    for r in rows {
        let (tool, count) = r.map_err(|e| e.to_string())?;
        if let Some(tool_name) = tool {
            list.push(crate::telemetry::ToolCount { tool_name, count: count as u64 });
        }
    }
    Ok(list)
}

pub fn search_knowledge_from_db(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<KnowledgeRow>, String> {
    let escaped = match fts_escape(query) {
        Some(q) => q,
        None => return Ok(vec![]),
    };
    let mut stmt = conn
        .prepare(
            "SELECT k.id, k.type, k.name, k.description, k.confidence, k.source \
             FROM knowledge_fts f JOIN knowledge k ON f.id = k.id \
             WHERE knowledge_fts MATCH ?1 ORDER BY rank LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![escaped, limit as i64], |row| {
            Ok(KnowledgeRow {
                id: row.get(0)?,
                type_: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                name: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                description: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                confidence: row.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
                source: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Aggregate activity events into 5-minute buckets grouped by spec/wave/tool.
/// Joins spans on spec to sum tokens. Fail-soft: returns empty vec on any
/// schema mismatch (missing columns/tables on partial Phase 1 DBs).
pub fn aggregate_activity_from_db(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<ActivityGroup>, String> {
    // 5-minute bucket = 300 seconds. ts is ISO-8601 string, so use strftime('%s', ts)/300.
    let sql = "SELECT e.spec, e.wave, \
                      json_extract(e.payload, '$.tool') AS action_kind, \
                      COUNT(*) AS cnt, \
                      MIN(e.ts) AS min_ts, \
                      MAX(e.ts) AS max_ts, \
                      COALESCE(SUM(COALESCE(s.input_tokens,0) + COALESCE(s.output_tokens,0)), 0) AS tokens_total, \
                      COUNT(DISTINCT json_extract(e.payload, '$.target.file')) AS files_touched \
               FROM events e \
               LEFT JOIN spans s ON s.spec = e.spec \
               GROUP BY e.spec, e.wave, action_kind, CAST(strftime('%s', e.ts) AS INTEGER) / 300 \
               ORDER BY max_ts DESC \
               LIMIT ?1";

    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Ok(vec![]),
    };
    let rows = match stmt.query_map([limit as i64], |row| {
        Ok(ActivityGroup {
            spec: row.get::<_, Option<String>>(0)?,
            wave: row.get::<_, Option<i64>>(1)?,
            action_kind: row.get::<_, Option<String>>(2)?,
            count: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
            min_ts: row.get::<_, Option<String>>(4)?,
            max_ts: row.get::<_, Option<String>>(5)?,
            tokens_total: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
            files_touched: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
        })
    }) {
        Ok(r) => r,
        Err(_) => return Ok(vec![]),
    };

    let mut out = Vec::new();
    for r in rows {
        if let Ok(group) = r {
            out.push(group);
        }
    }
    Ok(out)
}

/// Compute pipeline quality metrics. Every sub-query is independent and
/// fail-soft so partial schemas (e.g. spans without duration_ms) still return
/// a partially-populated `QualityMetrics`. Returns `QualityMetrics::default()`
/// if the connection doesn't satisfy `has_phase1_schema`.
pub fn quality_metrics_from_db(conn: &Connection) -> Result<QualityMetrics, String> {
    if !has_phase1_schema(conn) {
        return Ok(QualityMetrics::default());
    }
    let mut metrics = QualityMetrics::default();

    // pass_at_1: ratio of specs with status='completed'.
    metrics.pass_at_1 = conn
        .query_row(
            "SELECT COALESCE(1.0 * SUM(CASE WHEN status='completed' THEN 1 ELSE 0 END) / NULLIF(COUNT(*), 0), 0.0) FROM specs",
            [],
            |row| row.get::<_, Option<f64>>(0),
        )
        .ok()
        .flatten()
        .unwrap_or(0.0);

    // fix_loop_rate: distinct specs with escalation events / total distinct specs.
    metrics.fix_loop_rate = conn
        .query_row(
            "SELECT COALESCE(1.0 * (SELECT COUNT(DISTINCT spec) FROM events WHERE event='escalation') \
             / NULLIF((SELECT COUNT(DISTINCT spec) FROM events), 0), 0.0)",
            [],
            |row| row.get::<_, Option<f64>>(0),
        )
        .ok()
        .flatten()
        .unwrap_or(0.0);

    // avg_phase_duration_ms: AVG(duration_ms) from spans.
    metrics.avg_phase_duration_ms = conn
        .query_row(
            "SELECT COALESCE(AVG(duration_ms), 0.0) FROM spans",
            [],
            |row| row.get::<_, Option<f64>>(0),
        )
        .ok()
        .flatten()
        .unwrap_or(0.0);

    // by_role: top actor_ids by sample count from spans.
    if let Ok(mut stmt) = conn.prepare(
        "SELECT COALESCE(actor_id, 'unknown') AS role, COUNT(*) AS samples \
         FROM spans GROUP BY role ORDER BY samples DESC LIMIT 10",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok(RoleQuality {
                role: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                pass_at_1: 0.0,
                fix_loops: 0,
                samples: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            })
        }) {
            for r in rows.flatten() {
                metrics.by_role.push(r);
            }
        }
    }

    // slowest_waves: top 5 by duration_ms.
    if let Ok(mut stmt) = conn.prepare(
        "SELECT spec, wave, COALESCE(duration_ms, 0) FROM spans \
         ORDER BY duration_ms DESC LIMIT 5",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok(SlowestWave {
                spec: row.get::<_, Option<String>>(0)?,
                wave: row.get::<_, Option<i64>>(1)?,
                duration_ms: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            })
        }) {
            for r in rows.flatten() {
                metrics.slowest_waves.push(r);
            }
        }
    }

    // tokens_by_phase: average input/output per phase.
    if let Ok(mut stmt) = conn.prepare(
        "SELECT COALESCE(phase, 'unknown') AS phase, \
                COALESCE(AVG(input_tokens), 0.0), \
                COALESCE(AVG(output_tokens), 0.0) \
         FROM spans GROUP BY phase ORDER BY phase",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok(PhaseTokens {
                phase: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                input_avg: row.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
                output_avg: row.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
            })
        }) {
            for r in rows.flatten() {
                metrics.tokens_by_phase.push(r);
            }
        }
    }

    Ok(metrics)
}

// ── Consumption / cost queries (spans table) ────────────────────────────────
//
// All queries fail-soft: missing tables/columns or malformed `attributes` JSON
// degrade to empty/zero rather than propagating. Phase 1 dashboards may show
// a partially-populated summary when spans have not been emitted yet.
//
// Cost is materialised at span emit time by `pricing.ts` and persisted under
// `attributes -> 'mustard.cost_usd'`. We sum it back via json_extract.

const COST_EXTRACT: &str =
    "CAST(json_extract(attributes, '$.\"mustard.cost_usd\"') AS REAL)";
const AGENT_EXTRACT: &str =
    "json_extract(attributes, '$.\"mustard.agent_type\"')";

fn fourteen_days_ago_ms() -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    now - 14 * 86_400 * 1000
}

pub fn consumption_by_model(conn: &Connection) -> Result<Vec<ModelUsage>, String> {
    let sql = format!(
        "SELECT COALESCE(model, 'unknown') AS model, \
                COUNT(*) AS calls, \
                COALESCE(SUM(input_tokens), 0) AS input_tokens, \
                COALESCE(SUM(output_tokens), 0) AS output_tokens, \
                COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0) AS total_tokens, \
                COALESCE(SUM({cost}), 0.0) AS cost_usd \
         FROM spans GROUP BY model ORDER BY total_tokens DESC",
        cost = COST_EXTRACT
    );
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Ok(vec![]),
    };
    let rows = stmt
        .query_map([], |row| {
            Ok(ModelUsage {
                model: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                calls: row.get::<_, Option<i64>>(1)?.unwrap_or(0) as u64,
                input_tokens: row.get::<_, Option<i64>>(2)?.unwrap_or(0) as u64,
                output_tokens: row.get::<_, Option<i64>>(3)?.unwrap_or(0) as u64,
                total_tokens: row.get::<_, Option<i64>>(4)?.unwrap_or(0) as u64,
                cost_usd: row.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
                pct_tokens: 0.0,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut out: Vec<ModelUsage> = rows.flatten().collect();
    let grand_total: u64 = out.iter().map(|r| r.total_tokens).sum();
    if grand_total > 0 {
        for r in &mut out {
            r.pct_tokens = r.total_tokens as f64 / grand_total as f64;
        }
    }
    Ok(out)
}

pub fn consumption_by_agent_type(conn: &Connection) -> Result<Vec<AgentUsage>, String> {
    let sql = format!(
        "SELECT COALESCE({agent}, 'unknown') AS agent_type, \
                COUNT(*) AS calls, \
                COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0) AS total_tokens, \
                COALESCE(SUM({cost}), 0.0) AS cost_usd \
         FROM spans GROUP BY agent_type ORDER BY total_tokens DESC",
        agent = AGENT_EXTRACT,
        cost = COST_EXTRACT
    );
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Ok(vec![]),
    };
    let rows = stmt
        .query_map([], |row| {
            Ok(AgentUsage {
                agent_type: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                calls: row.get::<_, Option<i64>>(1)?.unwrap_or(0) as u64,
                total_tokens: row.get::<_, Option<i64>>(2)?.unwrap_or(0) as u64,
                cost_usd: row.get::<_, Option<f64>>(3)?.unwrap_or(0.0),
                pct_tokens: 0.0,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut out: Vec<AgentUsage> = rows.flatten().collect();
    let grand_total: u64 = out.iter().map(|r| r.total_tokens).sum();
    if grand_total > 0 {
        for r in &mut out {
            r.pct_tokens = r.total_tokens as f64 / grand_total as f64;
        }
    }
    Ok(out)
}

pub fn consumption_top_specs(conn: &Connection, limit: usize) -> Result<Vec<SpecUsage>, String> {
    let sql = format!(
        "SELECT spec, COUNT(*) AS calls, \
                COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0) AS total_tokens, \
                COALESCE(SUM({cost}), 0.0) AS cost_usd \
         FROM spans WHERE spec IS NOT NULL \
         GROUP BY spec ORDER BY total_tokens DESC LIMIT ?1",
        cost = COST_EXTRACT
    );
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Ok(vec![]),
    };
    let rows = stmt
        .query_map([limit as i64], |row| {
            Ok(SpecUsage {
                spec: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                calls: row.get::<_, Option<i64>>(1)?.unwrap_or(0) as u64,
                total_tokens: row.get::<_, Option<i64>>(2)?.unwrap_or(0) as u64,
                cost_usd: row.get::<_, Option<f64>>(3)?.unwrap_or(0.0),
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

pub fn consumption_daily_series(conn: &Connection, days: u32) -> Result<Vec<DailyPoint>, String> {
    let since_ms = fourteen_days_ago_ms().max(0);
    // Allow caller to override window size while keeping the default at 14d.
    let window_ms = (days as i64) * 86_400 * 1000;
    let since = (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
        - window_ms)
        .max(since_ms);

    let sql = format!(
        "SELECT date(started_at/1000, 'unixepoch') AS d, \
                COUNT(*) AS calls, \
                COALESCE(SUM(input_tokens), 0) AS input_tokens, \
                COALESCE(SUM(output_tokens), 0) AS output_tokens, \
                COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0) AS total_tokens, \
                COALESCE(SUM({cost}), 0.0) AS cost_usd \
         FROM spans WHERE started_at >= ?1 \
         GROUP BY d ORDER BY d ASC",
        cost = COST_EXTRACT
    );
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Ok(vec![]),
    };
    let rows = stmt
        .query_map([since], |row| {
            Ok(DailyPoint {
                date: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                calls: row.get::<_, Option<i64>>(1)?.unwrap_or(0) as u64,
                input_tokens: row.get::<_, Option<i64>>(2)?.unwrap_or(0) as u64,
                output_tokens: row.get::<_, Option<i64>>(3)?.unwrap_or(0) as u64,
                total_tokens: row.get::<_, Option<i64>>(4)?.unwrap_or(0) as u64,
                cost_usd: row.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

/// Token + cost totals (lifetime and today). Single-row query; conditional
/// aggregation avoids the cost of two scans.
pub fn cost_summary(conn: &Connection) -> Result<(u64, u64, f64, f64), String> {
    let midnight_ms = {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let day = now / 86_400;
        day * 86_400 * 1000
    };
    let sql = format!(
        "SELECT \
            COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0) AS tokens_total, \
            COALESCE(SUM({cost}), 0.0) AS cost_total, \
            COALESCE(SUM(CASE WHEN started_at >= ?1 \
                              THEN COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0) \
                              ELSE 0 END), 0) AS tokens_today, \
            COALESCE(SUM(CASE WHEN started_at >= ?1 \
                              THEN {cost} \
                              ELSE 0 END), 0.0) AS cost_today \
         FROM spans",
        cost = COST_EXTRACT
    );
    let row = conn
        .query_row(&sql, [midnight_ms], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?.unwrap_or(0) as u64,
                row.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
                row.get::<_, Option<i64>>(2)?.unwrap_or(0) as u64,
                row.get::<_, Option<f64>>(3)?.unwrap_or(0.0),
            ))
        })
        .map_err(|e| e.to_string())?;
    Ok((row.0, row.2, row.1, row.3))
}

/// One-shot consumption summary used by the `dashboard_consumption` Tauri
/// command. Composes the breakdowns above into a single payload.
pub fn consumption_summary_from_db(conn: &Connection) -> Result<ConsumptionSummary, String> {
    let (tokens_total, tokens_today, cost_total_usd, cost_today_usd) = cost_summary(conn)?;
    Ok(ConsumptionSummary {
        tokens_total,
        tokens_today,
        cost_total_usd,
        cost_today_usd,
        by_model: consumption_by_model(conn).unwrap_or_default(),
        by_agent_type: consumption_by_agent_type(conn).unwrap_or_default(),
        top_specs: consumption_top_specs(conn, 10).unwrap_or_default(),
        daily_series: consumption_daily_series(conn, 14).unwrap_or_default(),
    })
}

/// Agent activity — aggregates agent.start / agent.stop pairs from the events
/// table, grouped by actor_id (agent_type). Mirrors the logic of the former
/// NDJSON-based agent activity reader, running against SQLite.
///
/// Duration is computed per matched start→stop pair by collecting starts into an
/// in-memory map keyed by (session_id, actor_id) and matching each stop against
/// it. Tokens are deliberately omitted (they live in spans, not in the events
/// table). Returns an empty block on any schema mismatch.
pub fn agent_activity_from_db(conn: &Connection) -> Result<crate::telemetry::AgentActivityBlock, String> {
    // Collect starts per (session_id, actor_id) → ts.
    let start_sql = "SELECT COALESCE(actor_id, 'unknown') AS aid, \
                            COALESCE(session_id, '') AS sid, ts \
                     FROM events WHERE event = 'agent.start'";
    let stop_sql  = "SELECT COALESCE(actor_id, 'unknown') AS aid, \
                            COALESCE(session_id, '') AS sid, ts, \
                            COALESCE(json_extract(payload, '$.isError'), 0) AS is_err \
                     FROM events WHERE event = 'agent.stop'";

    // Accumulator per agent_type (actor_id).
    struct Acc {
        starts: u64,
        stops: u64,
        errors: u64,
        durations_ms: Vec<u64>,
        last_ts: Option<String>,
    }
    let mut acc: std::collections::HashMap<String, Acc> = std::collections::HashMap::new();
    // pending start timestamps: key = "sid|aid" → ts string
    let mut pending: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    if let Ok(mut stmt) = conn.prepare(start_sql) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        }) {
            for r in rows.flatten() {
                let (aid, sid, ts) = r;
                let entry = acc.entry(aid.clone()).or_insert_with(|| Acc { starts: 0, stops: 0, errors: 0, durations_ms: vec![], last_ts: None });
                entry.starts += 1;
                if let Some(ref t) = ts {
                    if entry.last_ts.as_ref().map_or(true, |cur| t > cur) {
                        entry.last_ts = Some(t.clone());
                    }
                    pending.insert(format!("{}|{}", sid, aid), t.clone());
                }
            }
        }
    }

    if let Ok(mut stmt) = conn.prepare(stop_sql) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                // json_extract returns 0/1/null for boolean fields in SQLite
                row.get::<_, Option<i64>>(3)?,
            ))
        }) {
            for r in rows.flatten() {
                let (aid, sid, ts, is_err_raw) = r;
                let is_error = is_err_raw.unwrap_or(0) != 0;
                let entry = acc.entry(aid.clone()).or_insert_with(|| Acc { starts: 0, stops: 0, errors: 0, durations_ms: vec![], last_ts: None });
                entry.stops += 1;
                if is_error { entry.errors += 1; }
                if let Some(ref t) = ts {
                    if entry.last_ts.as_ref().map_or(true, |cur| t > cur) {
                        entry.last_ts = Some(t.clone());
                    }
                    let key = format!("{}|{}", sid, aid);
                    if let Some(start_ts) = pending.remove(&key) {
                        if let (Some(t0), Some(t1)) = (
                            crate::telemetry::parse_iso_ms_pub(&start_ts),
                            crate::telemetry::parse_iso_ms_pub(t),
                        ) {
                            if t1 >= t0 { entry.durations_ms.push(t1 - t0); }
                        }
                    }
                }
            }
        }
    }

    let mut total_dispatches: u64 = 0;
    let mut total_errors: u64 = 0;
    let mut agents: Vec<crate::telemetry::AgentActivity> = acc
        .into_iter()
        .map(|(agent_type, a)| {
            total_dispatches += a.starts;
            total_errors += a.errors;
            let avg_duration_ms = if a.durations_ms.is_empty() { 0 } else {
                let sum: u64 = a.durations_ms.iter().sum();
                sum / a.durations_ms.len() as u64
            };
            crate::telemetry::AgentActivity { agent_type, starts: a.starts, stops: a.stops, errors: a.errors, avg_duration_ms, last_ts: a.last_ts }
        })
        .collect();
    agents.sort_by(|a, b| b.starts.cmp(&a.starts).then_with(|| b.last_ts.cmp(&a.last_ts)));
    agents.truncate(10);

    Ok(crate::telemetry::AgentActivityBlock { total_dispatches, total_errors, agents })
}

/// Derive the current session start timestamp from the events table.
///
/// Algorithm: find the `session_id` of the most recent event that carries one,
/// then return the earliest `ts` that shares that session_id. This mirrors the
/// former `session_start_ts` which read the NDJSON log (now removed).
pub fn session_start_ts_from_db(conn: &Connection) -> Option<String> {
    // Most recent session_id.
    let last_session: Option<String> = conn
        .query_row(
            "SELECT session_id FROM events WHERE session_id IS NOT NULL AND session_id != '' \
             ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok()
        .flatten();
    let session = last_session?;

    // Earliest ts carrying that session_id (ISO-8601 strings sort correctly).
    conn.query_row(
        "SELECT MIN(ts) FROM events WHERE session_id = ?1 AND ts IS NOT NULL",
        [&session],
        |row| row.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
}

/// Live activity tail — most recent events (up to `limit`) from the events
/// table, plus aggregates matching the `LiveActivity` / `PhaseActivity` shapes.
/// This backs `telemetry::live_activity` (NDJSON-based reader removed).
pub fn live_activity_from_db(conn: &Connection) -> Result<crate::telemetry::LiveActivity, String> {
    use crate::telemetry::{LiveActivity, PhaseActivity, ToolCount, CANONICAL_PHASES};

    // ------------------------------------------------------------------
    // 1. Global aggregates for today, last hour, last 5 minutes.
    //    The events table stores ts as ISO-8601 UTC strings.
    // ------------------------------------------------------------------
    let events_today: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE ts >= date('now')",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )
        .ok()
        .flatten()
        .unwrap_or(0)
        .max(0) as u64;

    let events_last_hour: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE ts >= datetime('now', '-1 hour')",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )
        .ok()
        .flatten()
        .unwrap_or(0)
        .max(0) as u64;

    let events_last_5min: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE ts >= datetime('now', '-5 minutes')",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )
        .ok()
        .flatten()
        .unwrap_or(0)
        .max(0) as u64;

    // ------------------------------------------------------------------
    // 2. Most-recent event metadata.
    // ------------------------------------------------------------------
    let last_event_ts: Option<String> = conn
        .query_row("SELECT MAX(ts) FROM events", [], |row| row.get(0))
        .ok()
        .flatten();

    let (current_phase, current_spec, current_wave): (Option<String>, Option<String>, Option<u32>) = conn
        .query_row(
            "SELECT json_extract(payload, '$.phase'), spec, wave \
             FROM events ORDER BY id DESC LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            },
        )
        .ok()
        .map(|(p, s, w)| {
            let phase = p.map(|ph| ph.to_ascii_uppercase());
            let wave = w.and_then(|n| u32::try_from(n).ok());
            (phase, s, wave)
        })
        .unwrap_or((None, None, None));

    // ------------------------------------------------------------------
    // 3. is_fresh: last event within 2 minutes.
    // ------------------------------------------------------------------
    let is_fresh: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE ts >= datetime('now', '-2 minutes')",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )
        .ok()
        .flatten()
        .unwrap_or(0)
        > 0;

    // ------------------------------------------------------------------
    // 4. Top tools today (all phases).
    // ------------------------------------------------------------------
    let tools_today: Vec<ToolCount> = {
        let sql = "SELECT COALESCE(json_extract(payload, '$.tool'), json_extract(payload, '$.tool_name')) AS t, \
                          COUNT(*) AS cnt \
                   FROM events WHERE event = 'tool.use' AND ts >= date('now') AND t IS NOT NULL \
                   GROUP BY t ORDER BY cnt DESC LIMIT 10";
        let mut stmt = conn.prepare(sql).unwrap_or_else(|_| conn.prepare("SELECT NULL, 0 WHERE 0").unwrap());
        stmt.query_map([], |row| {
            Ok(ToolCount {
                tool_name: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                count: row.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0) as u64,
            })
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    };

    // ------------------------------------------------------------------
    // 5. 60-minute sparkline (one bucket per minute, oldest first).
    // ------------------------------------------------------------------
    let minute_buckets: Vec<u64> = {
        let sql = "SELECT CAST((strftime('%s', 'now') - strftime('%s', ts)) / 60 AS INTEGER) AS mins_ago, \
                          COUNT(*) FROM events \
                   WHERE ts >= datetime('now', '-1 hour') \
                   GROUP BY mins_ago";
        let mut buckets = vec![0u64; 60];
        if let Ok(mut stmt) = conn.prepare(sql) {
            if let Ok(rows) = stmt.query_map([], |row| {
                Ok((row.get::<_, Option<i64>>(0)?.unwrap_or(-1), row.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0) as u64))
            }) {
                for r in rows.flatten() {
                    let (mins_ago, cnt) = r;
                    if (0..60).contains(&mins_ago) {
                        let idx = 59 - mins_ago as usize;
                        if let Some(b) = buckets.get_mut(idx) { *b += cnt; }
                    }
                }
            }
        }
        buckets
    };

    // ------------------------------------------------------------------
    // 6. Per-phase aggregates.
    // ------------------------------------------------------------------
    let by_phase: Vec<PhaseActivity> = CANONICAL_PHASES
        .iter()
        .map(|p| {
            let phase = (*p).to_string();

            let events_today_p: u64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM events \
                     WHERE json_extract(payload, '$.phase') = ?1 AND ts >= date('now')",
                    params![phase],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .ok().flatten().unwrap_or(0).max(0) as u64;

            let events_last_hour_p: u64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM events \
                     WHERE json_extract(payload, '$.phase') = ?1 AND ts >= datetime('now', '-1 hour')",
                    params![phase],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .ok().flatten().unwrap_or(0).max(0) as u64;

            let events_last_5min_p: u64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM events \
                     WHERE json_extract(payload, '$.phase') = ?1 AND ts >= datetime('now', '-5 minutes')",
                    params![phase],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .ok().flatten().unwrap_or(0).max(0) as u64;

            let last_event_ts_p: Option<String> = conn
                .query_row(
                    "SELECT MAX(ts) FROM events WHERE json_extract(payload, '$.phase') = ?1",
                    params![phase],
                    |row| row.get(0),
                )
                .ok().flatten();

            let last_spec_p: Option<String> = conn
                .query_row(
                    "SELECT spec FROM events WHERE json_extract(payload, '$.phase') = ?1 \
                     AND spec IS NOT NULL ORDER BY id DESC LIMIT 1",
                    params![phase],
                    |row| row.get(0),
                )
                .ok().flatten();

            let top_tools_p: Vec<ToolCount> = {
                let sql = "SELECT COALESCE(json_extract(payload, '$.tool'), json_extract(payload, '$.tool_name')) AS t, \
                                  COUNT(*) AS cnt \
                           FROM events WHERE event = 'tool.use' AND json_extract(payload, '$.phase') = ?1 \
                           AND ts >= date('now') AND t IS NOT NULL \
                           GROUP BY t ORDER BY cnt DESC LIMIT 3";
                if let Ok(mut stmt) = conn.prepare(sql) {
                    stmt.query_map(params![phase], |row| {
                        Ok(ToolCount {
                            tool_name: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                            count: row.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0) as u64,
                        })
                    })
                    .map(|rows| rows.flatten().collect())
                    .unwrap_or_default()
                } else { vec![] }
            };

            let minute_buckets_p: Vec<u64> = {
                let sql = "SELECT CAST((strftime('%s', 'now') - strftime('%s', ts)) / 60 AS INTEGER) AS mins_ago, \
                                  COUNT(*) FROM events \
                           WHERE json_extract(payload, '$.phase') = ?1 AND ts >= datetime('now', '-1 hour') \
                           GROUP BY mins_ago";
                let mut buckets = vec![0u64; 60];
                if let Ok(mut stmt) = conn.prepare(sql) {
                    if let Ok(rows) = stmt.query_map(params![phase], |row| {
                        Ok((row.get::<_, Option<i64>>(0)?.unwrap_or(-1), row.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0) as u64))
                    }) {
                        for r in rows.flatten() {
                            let (mins_ago, cnt) = r;
                            if (0..60).contains(&mins_ago) {
                                let idx = 59 - mins_ago as usize;
                                if let Some(b) = buckets.get_mut(idx) { *b += cnt; }
                            }
                        }
                    }
                }
                buckets
            };

            PhaseActivity {
                phase,
                events_today: events_today_p,
                events_last_hour: events_last_hour_p,
                events_last_5min: events_last_5min_p,
                minute_buckets: minute_buckets_p,
                last_event_ts: last_event_ts_p,
                top_tools: top_tools_p,
                last_spec: last_spec_p,
            }
        })
        .collect();

    Ok(LiveActivity {
        last_event_ts,
        events_today,
        events_last_hour,
        events_last_5min,
        tools_today,
        minute_buckets,
        current_spec,
        current_phase,
        current_wave,
        is_fresh,
        by_phase,
    })
}

// ---------------------------------------------------------------------------
// Pipeline aggregations — Wave 3b of 2026-05-19-pipeline-state-from-sqlite
// ---------------------------------------------------------------------------
//
// Both functions fold the pipeline.* event stream in Rust (insertion-order
// ASC) rather than in SQL, mirroring the logic of
// `mustard_rt::run::pipeline_state_for_spec`. mustard-rt is a binary-only
// crate and cannot be imported as a library, so the fold is re-implemented
// inline here.
//
// Precedence comment (mirrors the spec's DB-wins/FS-wins rule):
//   DB wins : status, scope, lang, model, is_wave_plan, total_waves,
//             current_wave, completed_waves, tasks_count, has_dispatch_failure,
//             failure_age_ms, updated_at (last event ts).
//   FS wins : spec title, frontmatter (### Lang: / ### Scope:), narrative.

/// Fold `pipeline.*` events for one spec into a `PipelineSummary`.
///
/// Returns `None` when the spec has no pipeline events at all.
fn fold_pipeline_summary(spec_name: &str, events: &[PipelineEventRow]) -> Option<PipelineSummary> {
    if events.is_empty() {
        return None;
    }
    let mut status: Option<String> = None;
    let mut scope = String::new();
    let mut phase = String::new();
    let mut updated_at: Option<String> = None;

    for ev in events {
        if !ev.ts.is_empty() {
            updated_at = Some(ev.ts.clone());
        }
        match ev.event.as_str() {
            "pipeline.scope" => {
                if let Some(v) = ev.payload.as_ref()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                {
                    if let Some(s) = v.get("scope").and_then(|x| x.as_str()) {
                        scope = s.to_string();
                    }
                }
            }
            "pipeline.status" => {
                if let Some(v) = ev.payload.as_ref()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                {
                    if let Some(to) = v.get("to").and_then(|x| x.as_str()) {
                        status = Some(to.to_string());
                    }
                }
            }
            "pipeline.phase" => {
                if let Some(v) = ev.payload.as_ref()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                {
                    if let Some(to) = v.get("to").and_then(|x| x.as_str()) {
                        phase = to.to_string();
                    }
                }
            }
            _ => {}
        }
    }

    Some(PipelineSummary {
        spec_name: spec_name.to_string(),
        phase,
        scope,
        status: status.unwrap_or_default(),
        updated_at,
    })
}

/// Fold `pipeline.*` events for one spec into an `ActivePipeline`.
///
/// Returns `None` when the spec has no pipeline events.
fn fold_active_pipeline(
    spec_name: &str,
    events: &[PipelineEventRow],
    now_secs: u64,
) -> Option<ActivePipeline> {
    if events.is_empty() {
        return None;
    }

    let mut status = String::from("unknown");
    let mut phase = String::from("UNKNOWN");
    let mut total_waves: Option<u32> = None;
    let mut model: Option<String> = None;
    let mut completed_waves: Vec<u32> = Vec::new();
    let mut tasks_pending: usize = 0;
    let mut tasks_in_progress: usize = 0;
    let mut tasks_completed: usize = 0;
    let mut task_statuses: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut updated_at: Option<String> = None;

    // Dispatch failure tracking.
    let mut failure_reason: Option<String> = None;
    let mut failure_at_secs: Option<u64> = None;

    for ev in events {
        if !ev.ts.is_empty() {
            updated_at = Some(ev.ts.clone());
        }
        match ev.event.as_str() {
            "pipeline.scope" => {
                if let Some(v) = ev.payload.as_ref()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                {
                    if let Some(tw) = v.get("totalWaves")
                        .or_else(|| v.get("total_waves"))
                        .and_then(|x| x.as_u64())
                    {
                        total_waves = Some(tw as u32);
                    }
                    if let Some(m) = v.get("model").and_then(|x| x.as_str()) {
                        model = Some(m.to_string());
                    }
                }
            }
            "pipeline.status" => {
                if let Some(v) = ev.payload.as_ref()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                {
                    if let Some(to) = v.get("to").and_then(|x| x.as_str()) {
                        status = to.to_string();
                    }
                }
            }
            "pipeline.phase" => {
                if let Some(v) = ev.payload.as_ref()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                {
                    if let Some(to) = v.get("to").and_then(|x| x.as_str()) {
                        phase = to.to_string();
                    }
                }
            }
            "pipeline.task.dispatch" => {
                if let Some(v) = ev.payload.as_ref()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                {
                    if let Some(name) = v.get("name").and_then(|x| x.as_str()) {
                        task_statuses.insert(name.to_string(), "pending".to_string());
                    }
                }
            }
            "pipeline.task.complete" => {
                if let Some(v) = ev.payload.as_ref()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                {
                    if let Some(name) = v.get("name").and_then(|x| x.as_str()) {
                        task_statuses.insert(name.to_string(), "completed".to_string());
                    }
                }
            }
            "pipeline.wave.complete" => {
                if let Some(v) = ev.payload.as_ref()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                {
                    if let Some(w) = v.get("wave").and_then(|x| x.as_u64()) {
                        let wn = w as u32;
                        if !completed_waves.contains(&wn) {
                            completed_waves.push(wn);
                        }
                    }
                }
            }
            "pipeline.dispatch_failure" => {
                if let Some(v) = ev.payload.as_ref()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                {
                    failure_reason = v.get("reason").and_then(|x| x.as_str()).map(str::to_string);
                    // `at` field from payload, fall back to event ts.
                    let at_str = v.get("at")
                        .and_then(|x| x.as_str())
                        .map(str::to_string)
                        .or_else(|| if ev.ts.is_empty() { None } else { Some(ev.ts.clone()) });
                    failure_at_secs = at_str.as_deref().and_then(crate::parse_iso_to_unix_secs);
                }
            }
            _ => {}
        }
    }

    // Compute current_wave from completed_waves.
    completed_waves.sort_unstable();
    completed_waves.dedup();
    let current_wave = completed_waves.iter().max().map(|w| w + 1).unwrap_or(1);

    // Task count breakdown.
    for s in task_statuses.values() {
        match s.as_str() {
            "pending" => tasks_pending += 1,
            "in_progress" => tasks_in_progress += 1,
            "completed" => tasks_completed += 1,
            _ => {}
        }
    }

    // Dispatch failure: stale if > 10 min old.
    const FAILURE_TTL_SECS: u64 = 10 * 60;
    let (has_dispatch_failure, failure_age_ms) = match (failure_reason, failure_at_secs) {
        (Some(_), Some(at)) => {
            let age = now_secs.saturating_sub(at);
            if age > FAILURE_TTL_SECS {
                (false, None)
            } else {
                (true, Some(age * 1000))
            }
        }
        (Some(_), None) => (true, None), // no timestamp → keep (fail-open)
        _ => (false, None),
    };

    Some(ActivePipeline {
        spec_name: spec_name.to_string(),
        status,
        phase,
        current_wave: Some(current_wave),
        total_waves,
        model,
        has_dispatch_failure,
        failure_age_ms,
        tasks_pending,
        tasks_in_progress,
        tasks_completed,
        updated_at,
    })
}

/// A raw row read from the events table for pipeline aggregation.
struct PipelineEventRow {
    spec: String,
    event: String,
    ts: String,
    payload: Option<String>,
}

/// Fetch and group all pipeline.* events per spec, then fold each group.
///
/// Single SQL query ordered by `id ASC` so the fold always processes events
/// in insertion order (matches `pipeline_state_for_spec` in mustard-rt).
fn fetch_pipeline_events_by_spec(conn: &Connection) -> Result<std::collections::BTreeMap<String, Vec<PipelineEventRow>>, String> {
    let sql = "SELECT spec, event, COALESCE(ts,''), payload \
               FROM events \
               WHERE event IN ('pipeline.scope','pipeline.status','pipeline.phase',\
                               'pipeline.task.dispatch','pipeline.task.complete',\
                               'pipeline.wave.complete','pipeline.dispatch_failure',\
                               'pipeline.pause','pipeline.resume_mode') \
               AND spec IS NOT NULL \
               ORDER BY id ASC";

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok(PipelineEventRow {
            spec:    row.get::<_, String>(0)?,
            event:   row.get::<_, String>(1)?,
            ts:      row.get::<_, String>(2)?,
            payload: row.get::<_, Option<String>>(3)?,
        })
    }).map_err(|e| e.to_string())?;

    let mut by_spec: std::collections::BTreeMap<String, Vec<PipelineEventRow>> =
        std::collections::BTreeMap::new();
    for r in rows {
        let row = r.map_err(|e| e.to_string())?;
        by_spec.entry(row.spec.clone()).or_default().push(row);
    }
    Ok(by_spec)
}

/// Aggregate pipeline events from the events table into `PipelineSummary` records.
///
/// Replaces the legacy `.claude/.pipeline-states/*.json` walk in
/// `dashboard_pipelines`. Fail-open: returns empty vec on schema mismatch.
pub fn pipelines_from_db(conn: &Connection) -> Vec<PipelineSummary> {
    let by_spec = match fetch_pipeline_events_by_spec(conn) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[pipelines_from_db] query failed: {e}");
            return vec![];
        }
    };
    let mut out = Vec::with_capacity(by_spec.len());
    for (spec_name, events) in &by_spec {
        if let Some(summary) = fold_pipeline_summary(spec_name, events) {
            out.push(summary);
        }
    }
    out
}

/// Aggregate pipeline events into `ActivePipeline` records.
///
/// Replaces the legacy `.claude/.pipeline-states/*.json` walk in
/// `dashboard_active_pipelines`. The caller filters by status when needed.
/// Fail-open: returns empty vec on schema mismatch.
pub fn active_pipelines_from_db(conn: &Connection, now_secs: u64) -> Vec<ActivePipeline> {
    let by_spec = match fetch_pipeline_events_by_spec(conn) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[active_pipelines_from_db] query failed: {e}");
            return vec![];
        }
    };
    let mut out = Vec::with_capacity(by_spec.len());
    for (spec_name, events) in &by_spec {
        if let Some(ap) = fold_active_pipeline(spec_name, events, now_secs) {
            out.push(ap);
        }
    }
    out
}

/// Browse the knowledge base without a query — sorted by type then recency.
/// Tries the rich SELECT (with `last_seen`) first and falls back to ordering
/// by id when the column is absent on older schemas.
pub fn knowledge_browse_from_db(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<KnowledgeRow>, String> {
    let rich_sql = "SELECT id, type, name, description, COALESCE(confidence, 0.0), source \
                    FROM knowledge \
                    ORDER BY type ASC, COALESCE(last_seen, 0) DESC \
                    LIMIT ?1";
    let fallback_sql = "SELECT id, type, name, description, COALESCE(confidence, 0.0), source \
                        FROM knowledge \
                        ORDER BY type ASC, id DESC \
                        LIMIT ?1";

    let use_rich = conn.prepare(rich_sql).is_ok();
    let sql = if use_rich { rich_sql } else { fallback_sql };

    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Ok(vec![]),
    };
    let rows = match stmt.query_map([limit as i64], |row| {
        Ok(KnowledgeRow {
            id: row.get(0)?,
            type_: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            name: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            description: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            confidence: row.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
            source: row.get(5)?,
        })
    }) {
        Ok(r) => r,
        Err(_) => return Ok(vec![]),
    };

    let mut out = Vec::new();
    for r in rows {
        if let Ok(row) = r {
            out.push(row);
        }
    }
    Ok(out)
}
