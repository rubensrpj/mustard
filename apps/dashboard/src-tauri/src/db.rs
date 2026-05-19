use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{KnowledgeRow, KnowledgeSummary, MetricsSummary, RecentEvent, SpecRow};

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

fn summary_from_payload(payload: &Option<String>) -> Option<String> {
    let raw = payload.as_deref()?;
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    for key in &["summary", "description", "msg", "text"] {
        if let Some(s) = v.get(*key).and_then(|x| x.as_str()) {
            let trimmed = s.chars().take(80).collect::<String>();
            return Some(trimmed);
        }
    }
    None
}

pub fn recent_events_from_db(conn: &Connection, limit: usize) -> Result<Vec<RecentEvent>, String> {
    let mut stmt = conn
        .prepare("SELECT event, spec, ts, payload FROM events ORDER BY id DESC LIMIT ?1")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([limit as i64], |row| {
            let event_type: String = row.get(0)?;
            let _spec: Option<String> = row.get(1)?;
            let ts: Option<String> = row.get(2)?;
            let payload: Option<String> = row.get(3)?;
            Ok(RecentEvent {
                event_type,
                ts,
                summary: summary_from_payload(&payload),
            })
        })
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

pub fn specs_from_db(conn: &Connection) -> Result<Vec<SpecRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT name, status, phase, started_at, completed_at, affected_files FROM specs \
             ORDER BY COALESCE(completed_at, started_at) DESC",
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
    let mut stmt = conn
        .prepare(
            "SELECT e.event, e.spec, e.ts, e.payload FROM events_fts f \
             JOIN events e ON f.rowid = e.id \
             WHERE events_fts MATCH ?1 ORDER BY rank LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![escaped, limit as i64], |row| {
            let event_type: String = row.get(0)?;
            let _spec: Option<String> = row.get(1)?;
            let ts: Option<String> = row.get(2)?;
            let payload: Option<String> = row.get(3)?;
            Ok(RecentEvent {
                event_type,
                ts,
                summary: summary_from_payload(&payload),
            })
        })
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
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
