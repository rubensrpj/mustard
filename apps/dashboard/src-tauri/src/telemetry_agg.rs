//! Wave-7 telemetry aggregation functions.
//!
//! Each function takes a rusqlite `Connection` (read-only) and a `time_range`
//! string, and returns a typed shape. All functions are fail-open: DB errors
//! or missing columns return `Ok(empty)` rather than propagating.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

// ── Shapes ──────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct PhaseSummary {
    pub phase: String,
    pub events_count: i64,
    pub last_event_at: Option<String>,
    /// Event counts per day, last 7 days (oldest first, 7 slots).
    pub sparkline: Vec<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct TimelineEvent {
    pub id: String,
    pub ts: String,
    pub phase: Option<String>,
    pub spec: Option<String>,
    pub agent: Option<String>,
    pub summary: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct HeatmapCell {
    /// 0 = Sunday … 6 = Saturday (SQLite strftime('%w'))
    pub day_of_week: i64,
    /// 0–23
    pub hour: i64,
    pub event_count: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct HistoryEntry {
    pub spec: String,
    pub status: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    /// phase label → cumulative event count for that phase
    pub duration_per_phase: std::collections::HashMap<String, i64>,
    pub ac_passed: i64,
    pub ac_total: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct AcceptanceCriterion {
    pub spec: String,
    pub id: String,
    pub status: String,
    pub last_run_at: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct FileCount {
    pub path: String,
    pub count: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct ToolUseCount {
    pub name: String,
    pub count: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct PhaseEventCount {
    pub phase: String,
    pub duration_ms: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct AgentTypeCount {
    pub agent_type: String,
    pub count: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct EffortBreakdown {
    pub top_files: Vec<FileCount>,
    pub top_tools: Vec<ToolUseCount>,
    pub top_phases: Vec<PhaseEventCount>,
    pub top_agents: Vec<AgentTypeCount>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct AgentDispatch {
    pub subagent_type: String,
    pub count: i64,
    pub error_count: i64,
    pub avg_duration_ms: i64,
    pub last_dispatched_at: Option<String>,
}

// ── time_range helper ────────────────────────────────────────────────────────

/// Convert a `time_range` label into a SQL WHERE fragment (without leading AND).
/// Always returns a safe, injectable literal — no user data reaches SQL.
fn time_filter(time_range: &str) -> &'static str {
    match time_range {
        "7d"  => "ts >= datetime('now', '-7 days')",
        "30d" => "ts >= datetime('now', '-30 days')",
        "all" => "1=1",
        _     => "date(ts) >= date('now')",   // "today" and unknown → today
    }
}

// ── 1. telemetry_phases ──────────────────────────────────────────────────────

pub fn telemetry_phases(conn: &Connection, time_range: &str) -> Result<Vec<PhaseSummary>, String> {
    let tf = time_filter(time_range);
    let sql = format!(
        "SELECT COALESCE(json_extract(payload, '$.phase'), \
                         json_extract(payload, '$.to'), 'unknown') AS phase, \
                COUNT(*) AS cnt, MAX(ts) AS last_ts \
         FROM events \
         WHERE event IN ('pipeline.phase', 'pipeline.status') AND {tf} \
         GROUP BY phase \
         ORDER BY cnt DESC"
    );

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Ok(vec![]),
    };

    let rows = match stmt.query_map([], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?.unwrap_or_default(),
            row.get::<_, i64>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    }) {
        Ok(r) => r,
        Err(_) => return Ok(vec![]),
    };

    let mut out = Vec::new();
    for r in rows.flatten() {
        let (phase, cnt, last_ts) = r;
        let sparkline = phase_sparkline(conn, &phase);
        out.push(PhaseSummary {
            phase,
            events_count: cnt,
            last_event_at: last_ts,
            sparkline,
        });
    }
    Ok(out)
}

/// Build a 7-slot sparkline (oldest day first) for a given phase label.
/// Fail-soft: returns vec![0;7] on any error.
fn phase_sparkline(conn: &Connection, phase: &str) -> Vec<i64> {
    let mut buckets = vec![0i64; 7];
    let sql = "SELECT CAST((julianday('now') - julianday(date(ts))) AS INTEGER) AS days_ago, \
                      COUNT(*) \
               FROM events \
               WHERE event IN ('pipeline.phase', 'pipeline.status') \
                 AND COALESCE(json_extract(payload, '$.phase'), json_extract(payload, '$.to'), '') = ?1 \
                 AND ts >= datetime('now', '-7 days') \
               GROUP BY days_ago";
    if let Ok(mut stmt) = conn.prepare(sql) {
        if let Ok(rows) = stmt.query_map(params![phase], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        }) {
            for r in rows.flatten() {
                let (days_ago, cnt) = r;
                // days_ago 0 = today, 6 = 6 days ago → slot = 6 - days_ago
                if (0..7).contains(&days_ago) {
                    let idx = (6 - days_ago) as usize;
                    buckets[idx] = cnt;
                }
            }
        }
    }
    buckets
}

// ── 2. telemetry_timeline ────────────────────────────────────────────────────

pub fn telemetry_timeline(
    conn: &Connection,
    time_range: &str,
    limit: usize,
) -> Result<Vec<TimelineEvent>, String> {
    let tf = time_filter(time_range);
    let sql = format!(
        "SELECT CAST(id AS TEXT), COALESCE(ts,''), \
                json_extract(payload, '$.phase'), \
                spec, \
                COALESCE(json_extract(payload, '$.subagent_type'), \
                         json_extract(payload, '$.agent_type'), \
                         actor_id), \
                COALESCE(json_extract(payload, '$.summary'), \
                         json_extract(payload, '$.description'), \
                         json_extract(payload, '$.msg'), \
                         event, '') \
         FROM events \
         WHERE {tf} \
         ORDER BY id DESC \
         LIMIT ?1"
    );

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Ok(vec![]),
    };

    let rows = match stmt.query_map(params![limit as i64], |row| {
        Ok(TimelineEvent {
            id:      row.get::<_, Option<String>>(0)?.unwrap_or_default(),
            ts:      row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            phase:   row.get::<_, Option<String>>(2)?,
            spec:    row.get::<_, Option<String>>(3)?,
            agent:   row.get::<_, Option<String>>(4)?,
            summary: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        })
    }) {
        Ok(r) => r,
        Err(_) => return Ok(vec![]),
    };

    Ok(rows.flatten().collect())
}

// ── 3. telemetry_heatmap ─────────────────────────────────────────────────────

pub fn telemetry_heatmap(conn: &Connection, time_range: &str) -> Result<Vec<HeatmapCell>, String> {
    let tf = time_filter(time_range);
    let sql = format!(
        "SELECT CAST(strftime('%w', ts) AS INTEGER) AS dow, \
                CAST(strftime('%H', ts) AS INTEGER) AS hr, \
                COUNT(*) AS cnt \
         FROM events \
         WHERE {tf} AND ts IS NOT NULL \
         GROUP BY dow, hr \
         ORDER BY dow, hr"
    );

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Ok(vec![]),
    };

    let rows = match stmt.query_map([], |row| {
        Ok(HeatmapCell {
            day_of_week: row.get::<_, i64>(0)?,
            hour:        row.get::<_, i64>(1)?,
            event_count: row.get::<_, i64>(2)?,
        })
    }) {
        Ok(r) => r,
        Err(_) => return Ok(vec![]),
    };

    Ok(rows.flatten().collect())
}

// ── 4. telemetry_history ─────────────────────────────────────────────────────

pub fn telemetry_history(conn: &Connection, time_range: &str, limit: usize) -> Result<Vec<HistoryEntry>, String> {
    let tf = time_filter(time_range);
    // Spec-level metadata from the specs table, filtered by time_range.
    let spec_sql = format!(
        "SELECT name, COALESCE(status,'unknown'), \
               COALESCE(started_at,''), completed_at \
        FROM specs \
        WHERE COALESCE(completed_at, started_at) IS NOT NULL \
          AND {tf} \
        ORDER BY COALESCE(completed_at, started_at) DESC \
        LIMIT ?1"
    );
    let spec_sql = spec_sql.as_str();

    let mut stmt = match conn.prepare(spec_sql) {
        Ok(s) => s,
        Err(_) => return Ok(vec![]),
    };

    let spec_rows: Vec<(String, String, String, Option<String>)> =
        match stmt.query_map(params![limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        }) {
            Ok(r) => r.flatten().collect(),
            Err(_) => return Ok(vec![]),
        };

    // Phase event counts per spec.
    let phase_sql = "SELECT spec, \
                            COALESCE(json_extract(payload, '$.phase'), \
                                     json_extract(payload, '$.to'), 'unknown') AS phase, \
                            COUNT(*) AS cnt \
                     FROM events \
                     WHERE event IN ('pipeline.phase','pipeline.status') \
                       AND spec IS NOT NULL \
                     GROUP BY spec, phase";

    // (spec → (phase → count))
    let mut phase_map: std::collections::HashMap<
        String,
        std::collections::HashMap<String, i64>,
    > = std::collections::HashMap::new();

    if let Ok(mut stmt2) = conn.prepare(phase_sql) {
        if let Ok(rows) = stmt2.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, i64>(2)?,
            ))
        }) {
            for r in rows.flatten() {
                let (spec, phase, cnt) = r;
                phase_map.entry(spec).or_default().insert(phase, cnt);
            }
        }
    }

    // QA results per spec.
    let qa_sql = "SELECT spec, \
                         SUM(CASE WHEN json_extract(payload,'$.overall')='pass' THEN 1 ELSE 0 END), \
                         COUNT(*) \
                  FROM events \
                  WHERE event='qa.result' AND spec IS NOT NULL \
                  GROUP BY spec";

    let mut qa_map: std::collections::HashMap<String, (i64, i64)> =
        std::collections::HashMap::new();

    if let Ok(mut stmt3) = conn.prepare(qa_sql) {
        if let Ok(rows) = stmt3.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        }) {
            for r in rows.flatten() {
                let (spec, passed, total) = r;
                qa_map.insert(spec, (passed, total));
            }
        }
    }

    let mut out = Vec::with_capacity(spec_rows.len());
    for (spec, status, started_at, completed_at) in spec_rows {
        let duration_per_phase = phase_map.remove(&spec).unwrap_or_default();
        let (ac_passed, ac_total) = qa_map.get(&spec).copied().unwrap_or((0, 0));
        out.push(HistoryEntry {
            spec,
            status,
            started_at,
            completed_at,
            duration_per_phase,
            ac_passed,
            ac_total,
        });
    }
    Ok(out)
}

// ── 5. telemetry_criteria ────────────────────────────────────────────────────

pub fn telemetry_criteria(
    conn: &Connection,
    time_range: &str,
) -> Result<Vec<AcceptanceCriterion>, String> {
    let tf = time_filter(time_range);
    // Last qa.result per (spec, ac_id). Some qa.result payloads embed criteria
    // as an array; we use json_each to expand them.
    let expanded_sql = format!(
        "SELECT e.spec, \
                json_extract(c.value, '$.id') AS ac_id, \
                COALESCE(json_extract(c.value, '$.result'), 'unknown') AS status, \
                MAX(e.ts) AS last_run \
         FROM events e, json_each(json_extract(e.payload, '$.criteria')) c \
         WHERE e.event = 'qa.result' AND e.spec IS NOT NULL AND {tf} \
         GROUP BY e.spec, ac_id"
    );

    if let Ok(mut stmt) = conn.prepare(&expanded_sql) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok(AcceptanceCriterion {
                spec:        row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                id:          row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                status:      row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                last_run_at: row.get::<_, Option<String>>(3)?,
            })
        }) {
            let out: Vec<AcceptanceCriterion> = rows.flatten()
                .filter(|r| !r.id.is_empty())
                .collect();
            if !out.is_empty() {
                return Ok(out);
            }
        }
    }

    // Fallback: qa.result rows with no embedded criteria array.
    let fallback_sql = format!(
        "SELECT spec, \
                COALESCE(json_extract(payload,'$.spec'), spec, 'unknown') AS ac_id, \
                COALESCE(json_extract(payload,'$.overall'), 'unknown') AS status, \
                MAX(ts) AS last_run \
         FROM events \
         WHERE event='qa.result' AND spec IS NOT NULL AND {tf} \
         GROUP BY spec"
    );

    let mut stmt = match conn.prepare(&fallback_sql) {
        Ok(s) => s,
        Err(_) => return Ok(vec![]),
    };

    let rows = match stmt.query_map([], |row| {
        Ok(AcceptanceCriterion {
            spec:        row.get::<_, Option<String>>(0)?.unwrap_or_default(),
            id:          row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            status:      row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            last_run_at: row.get::<_, Option<String>>(3)?,
        })
    }) {
        Ok(r) => r,
        Err(_) => return Ok(vec![]),
    };

    Ok(rows.flatten().collect())
}

// ── 6. telemetry_effort ──────────────────────────────────────────────────────

pub fn telemetry_effort(conn: &Connection, time_range: &str) -> Result<EffortBreakdown, String> {
    let tf = time_filter(time_range);
    const TOP_N: i64 = 10;

    // Top files: extracted from tool.use payload.target.file or tool_input.file_path
    let files_sql = format!(
        "SELECT COALESCE( \
                    json_extract(payload, '$.target.file'), \
                    json_extract(payload, '$.tool_input.file_path') \
                 ) AS path, COUNT(*) AS cnt \
         FROM events \
         WHERE event='tool.use' AND {tf} AND path IS NOT NULL \
         GROUP BY path ORDER BY cnt DESC LIMIT {TOP_N}"
    );

    let top_files: Vec<FileCount> = match conn.prepare(&files_sql) {
        Ok(mut stmt) => stmt
            .query_map([], |row| {
                Ok(FileCount {
                    path:  row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    count: row.get::<_, i64>(1)?,
                })
            })
            .map(|r| r.flatten().collect())
            .unwrap_or_default(),
        Err(_) => vec![],
    };

    // Top tools
    let tools_sql = format!(
        "SELECT COALESCE( \
                    json_extract(payload, '$.tool'), \
                    json_extract(payload, '$.tool_name') \
                 ) AS tool, COUNT(*) AS cnt \
         FROM events \
         WHERE event='tool.use' AND {tf} AND tool IS NOT NULL \
         GROUP BY tool ORDER BY cnt DESC LIMIT {TOP_N}"
    );

    let top_tools: Vec<ToolUseCount> = match conn.prepare(&tools_sql) {
        Ok(mut stmt) => stmt
            .query_map([], |row| {
                Ok(ToolUseCount {
                    name:  row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    count: row.get::<_, i64>(1)?,
                })
            })
            .map(|r| r.flatten().collect())
            .unwrap_or_default(),
        Err(_) => vec![],
    };

    // Top phases: event count per phase (using duration_ms=count as proxy
    // when spans are absent; callers interpret this as relative effort).
    let phases_sql = format!(
        "SELECT COALESCE( \
                    json_extract(payload, '$.phase'), \
                    json_extract(payload, '$.to'), 'unknown' \
                 ) AS phase, COUNT(*) AS cnt \
         FROM events \
         WHERE event IN ('pipeline.phase','pipeline.status') AND {tf} \
         GROUP BY phase ORDER BY cnt DESC LIMIT {TOP_N}"
    );

    let top_phases: Vec<PhaseEventCount> = match conn.prepare(&phases_sql) {
        Ok(mut stmt) => stmt
            .query_map([], |row| {
                Ok(PhaseEventCount {
                    phase:       row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    duration_ms: row.get::<_, i64>(1)?,
                })
            })
            .map(|r| r.flatten().collect())
            .unwrap_or_default(),
        Err(_) => vec![],
    };

    // Top agents by subagent_type from payload
    let agents_sql = format!(
        "SELECT COALESCE( \
                    json_extract(payload, '$.subagent_type'), \
                    json_extract(payload, '$.agent_type'), \
                    actor_id, 'unknown' \
                 ) AS atype, COUNT(*) AS cnt \
         FROM events \
         WHERE event='agent.start' AND {tf} \
         GROUP BY atype ORDER BY cnt DESC LIMIT {TOP_N}"
    );

    let top_agents: Vec<AgentTypeCount> = match conn.prepare(&agents_sql) {
        Ok(mut stmt) => stmt
            .query_map([], |row| {
                Ok(AgentTypeCount {
                    agent_type: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    count:      row.get::<_, i64>(1)?,
                })
            })
            .map(|r| r.flatten().collect())
            .unwrap_or_default(),
        Err(_) => vec![],
    };

    Ok(EffortBreakdown {
        top_files,
        top_tools,
        top_phases,
        top_agents,
    })
}

// ── 7. telemetry_agents ──────────────────────────────────────────────────────

/// Group by `payload->>'$.subagent_type'` (real agent type), NOT by actor_id.
/// Duration is computed in Rust by matching start→stop pairs on (session_id, actor_id).
pub fn telemetry_agents(
    conn: &Connection,
    time_range: &str,
) -> Result<Vec<AgentDispatch>, String> {
    let tf = time_filter(time_range);

    // Collect start events: (session_id, actor_id, subagent_type, ts)
    let start_sql = format!(
        "SELECT COALESCE(session_id,''), \
                COALESCE(actor_id,''), \
                COALESCE( \
                    json_extract(payload, '$.subagent_type'), \
                    json_extract(payload, '$.agent_type'), \
                    actor_id, 'unknown' \
                ) AS stype, \
                ts \
         FROM events \
         WHERE event='agent.start' AND {tf}"
    );

    struct StartRow {
        session_id: String,
        actor_id: String,
        subagent_type: String,
        ts: Option<String>,
    }

    let starts: Vec<StartRow> = match conn.prepare(&start_sql) {
        Ok(mut stmt) => stmt
            .query_map([], |row| {
                Ok(StartRow {
                    session_id:    row.get::<_, String>(0)?,
                    actor_id:      row.get::<_, String>(1)?,
                    subagent_type: row.get::<_, String>(2)?,
                    ts:            row.get::<_, Option<String>>(3)?,
                })
            })
            .map(|r| r.flatten().collect())
            .unwrap_or_default(),
        Err(_) => vec![],
    };

    // Collect stop events: (session_id, actor_id, ts, is_error)
    let stop_sql = format!(
        "SELECT COALESCE(session_id,''), \
                COALESCE(actor_id,''), \
                ts, \
                COALESCE(json_extract(payload,'$.isError'),0) \
         FROM events \
         WHERE event='agent.stop' AND {tf}"
    );

    struct StopRow {
        session_id: String,
        actor_id: String,
        ts: Option<String>,
        is_error: bool,
    }

    let stops: Vec<StopRow> = match conn.prepare(&stop_sql) {
        Ok(mut stmt) => stmt
            .query_map([], |row| {
                let is_err_raw: i64 = row.get::<_, Option<i64>>(3)?.unwrap_or(0);
                Ok(StopRow {
                    session_id: row.get::<_, String>(0)?,
                    actor_id:   row.get::<_, String>(1)?,
                    ts:         row.get::<_, Option<String>>(2)?,
                    is_error:   is_err_raw != 0,
                })
            })
            .map(|r| r.flatten().collect())
            .unwrap_or_default(),
        Err(_) => vec![],
    };

    // Build pending map: key = "session_id|actor_id" → (subagent_type, ts_ms)
    let mut pending: std::collections::HashMap<String, (String, Option<i64>)> =
        std::collections::HashMap::new();

    struct Acc {
        count: i64,
        error_count: i64,
        durations_ms: Vec<i64>,
        last_ts: Option<String>,
    }
    let mut acc: std::collections::HashMap<String, Acc> = std::collections::HashMap::new();

    for s in &starts {
        let key = format!("{}|{}", s.session_id, s.actor_id);
        let ts_ms = s.ts.as_deref().and_then(parse_iso_to_ms);
        pending.insert(key, (s.subagent_type.clone(), ts_ms));

        let entry = acc.entry(s.subagent_type.clone()).or_insert_with(|| Acc {
            count: 0, error_count: 0, durations_ms: vec![], last_ts: None,
        });
        entry.count += 1;
        if let Some(ref t) = s.ts {
            if entry.last_ts.as_ref().map_or(true, |cur| t > cur) {
                entry.last_ts = Some(t.clone());
            }
        }
    }

    for s in &stops {
        let key = format!("{}|{}", s.session_id, s.actor_id);
        if let Some((subagent_type, start_ms)) = pending.remove(&key) {
            let entry = acc.entry(subagent_type).or_insert_with(|| Acc {
                count: 0, error_count: 0, durations_ms: vec![], last_ts: None,
            });
            if s.is_error {
                entry.error_count += 1;
            }
            if let (Some(t0), Some(t1)) = (start_ms, s.ts.as_deref().and_then(parse_iso_to_ms)) {
                if t1 >= t0 {
                    entry.durations_ms.push(t1 - t0);
                }
            }
            if let Some(ref t) = s.ts {
                if entry.last_ts.as_ref().map_or(true, |cur| t > cur) {
                    entry.last_ts = Some(t.clone());
                }
            }
        }
    }

    let mut out: Vec<AgentDispatch> = acc
        .into_iter()
        .map(|(subagent_type, a)| {
            let avg = if a.durations_ms.is_empty() {
                0
            } else {
                a.durations_ms.iter().sum::<i64>() / a.durations_ms.len() as i64
            };
            AgentDispatch {
                subagent_type,
                count: a.count,
                error_count: a.error_count,
                avg_duration_ms: avg,
                last_dispatched_at: a.last_ts,
            }
        })
        .collect();

    // Stable order: most-dispatched first.
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| b.subagent_type.cmp(&a.subagent_type)));
    Ok(out)
}

// ── internal ISO → ms helper ─────────────────────────────────────────────────

/// Parse ISO-8601 UTC string into milliseconds since epoch.
/// Reuses the same hand-rolled approach as `crate::parse_iso_to_unix_secs`,
/// extended for sub-second precision. Returns `None` on any parse failure.
fn parse_iso_to_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    let s = s.strip_suffix('Z').unwrap_or(s);
    let s = if let Some(pos) = s.rfind('+') {
        if pos > 10 { &s[..pos] } else { s }
    } else {
        s
    };
    let (date_part, time_part) = s.split_once('T')?;
    let mut dp = date_part.splitn(3, '-');
    let year:  i64 = dp.next()?.parse().ok()?;
    let month: i64 = dp.next()?.parse().ok()?;
    let day:   i64 = dp.next()?.parse().ok()?;
    let (time_no_frac, frac) = match time_part.split_once('.') {
        Some((t, f)) => (t, f),
        None => (time_part, ""),
    };
    let mut tp = time_no_frac.splitn(3, ':');
    let hour:   i64 = tp.next()?.parse().ok()?;
    let minute: i64 = tp.next()?.parse().ok()?;
    let second: i64 = tp.next()?.parse().ok()?;
    let ms_frac: i64 = if frac.is_empty() {
        0
    } else {
        let padded = format!("{:0<3}", &frac[..frac.len().min(3)]);
        padded.parse().ok()?
    };
    // Days since epoch (ignores leap seconds, same as existing code).
    let days = days_since_epoch_i64(year, month, day)?;
    let secs = days * 86400 + hour * 3600 + minute * 60 + second;
    Some(secs * 1000 + ms_frac)
}

fn days_since_epoch_i64(year: i64, month: i64, day: i64) -> Option<i64> {
    if year < 1970 { return None; }
    let mut total: i64 = 0;
    for y in 1970..year {
        total += if is_leap_i64(y) { 366 } else { 365 };
    }
    let days_in_month: [i64; 12] = [
        31, if is_leap_i64(year) { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    for m in 1..month {
        total += *days_in_month.get((m - 1) as usize)?;
    }
    total += day - 1;
    Some(total)
}

fn is_leap_i64(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
