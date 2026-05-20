//! Amend-window metric queries for Wave 4 of spec 2026-05-20-session-bound-amendments.
//!
//! All functions are fail-open: DB errors return Ok(zero-value) + eprintln.
//! No panics propagate to the Tauri frontend.

use rusqlite::{Connection, params};

use crate::db;

// ── helpers ─────────────────────────────────────────────────────────────────

/// Parse ISO-8601 UTC string to milliseconds since epoch.
/// Returns None on any parse failure.
fn iso_to_ms(s: &str) -> Option<i64> {
    let s = s.trim().strip_suffix('Z').unwrap_or(s.trim());
    let s = if let Some(pos) = s.rfind('+') {
        if pos > 10 { &s[..pos] } else { s }
    } else { s };
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
    let days = days_since_epoch(year, month, day)?;
    Some((days * 86400 + hour * 3600 + minute * 60 + second) * 1000 + ms_frac)
}

fn days_since_epoch(year: i64, month: i64, day: i64) -> Option<i64> {
    if year < 1970 { return None; }
    let mut total: i64 = 0;
    for y in 1970..year {
        total += if is_leap(y) { 366 } else { 365 };
    }
    let dim: [i64; 12] = [31, if is_leap(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for m in 1..month {
        total += *dim.get((m - 1) as usize)?;
    }
    total += day - 1;
    Some(total)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

// ── 1. amend_resolution_rate ────────────────────────────────────────────────

/// Percentage of `pipeline_amend_window` rows that ended with status='archived'
/// out of all closed windows (excludes open/amending rows).
/// Returns 0.0 when there are no closed windows.
pub fn amend_resolution_rate_query(conn: &Connection) -> Result<f64, String> {
    let sql = "SELECT \
                   CAST(SUM(CASE WHEN status='archived' THEN 1 ELSE 0 END) AS REAL), \
                   CAST(COUNT(*) AS REAL) \
               FROM pipeline_amend_window \
               WHERE status NOT IN ('open', 'amending')";
    let mut stmt = conn.prepare(sql).map_err(|_| String::new())?;
    let row = stmt.query_row([], |r| {
        Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?))
    });
    match row {
        Ok((archived, total)) if total > 0.0 => Ok(archived / total),
        _ => Ok(0.0),
    }
}

// ── 2. amend_drift_rate ─────────────────────────────────────────────────────

/// Percentage of closed windows with status='closed-amend-drift'.
/// Returns 0.0 when there are no closed windows.
pub fn amend_drift_rate_query(conn: &Connection) -> Result<f64, String> {
    let sql = "SELECT \
                   CAST(SUM(CASE WHEN status='closed-amend-drift' THEN 1 ELSE 0 END) AS REAL), \
                   CAST(COUNT(*) AS REAL) \
               FROM pipeline_amend_window \
               WHERE status NOT IN ('open', 'amending')";
    let mut stmt = conn.prepare(sql).map_err(|_| String::new())?;
    let row = stmt.query_row([], |r| {
        Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?))
    });
    match row {
        Ok((drift, total)) if total > 0.0 => Ok(drift / total),
        _ => Ok(0.0),
    }
}

// ── 3. cross_session_amend_count ─────────────────────────────────────────────

/// Count of windows carrying over to the next session (status='closed-amend-pending').
pub fn cross_session_amend_count_query(conn: &Connection) -> Result<u64, String> {
    let sql = "SELECT COUNT(*) FROM pipeline_amend_window WHERE status='closed-amend-pending'";
    let mut stmt = conn.prepare(sql).map_err(|_| String::new())?;
    let n: i64 = stmt.query_row([], |r| r.get(0)).unwrap_or(0);
    Ok(n.max(0) as u64)
}

// ── 4. amend_window_duration ────────────────────────────────────────────────

/// For each non-open/amending window, compute duration in milliseconds between
/// `closed_at` and the latest `pipeline.amend_close` event timestamp for the
/// same (spec_id, session_id). Returns a Vec of durations; empty if no data.
pub fn amend_window_duration_query(conn: &Connection) -> Result<Vec<i64>, String> {
    // Step 1: load closed windows that have both spec_id, session_id, closed_at.
    let win_sql = "SELECT spec_id, session_id, closed_at \
                   FROM pipeline_amend_window \
                   WHERE status NOT IN ('open','amending') \
                     AND closed_at IS NOT NULL \
                     AND spec_id IS NOT NULL \
                     AND session_id IS NOT NULL";

    let mut stmt = match conn.prepare(win_sql) {
        Ok(s) => s,
        Err(_) => return Ok(vec![]),
    };

    struct WinRow { spec_id: String, session_id: String, closed_at: String }

    let windows: Vec<WinRow> = match stmt.query_map([], |r| {
        Ok(WinRow {
            spec_id:    r.get::<_, String>(0)?,
            session_id: r.get::<_, String>(1)?,
            closed_at:  r.get::<_, String>(2)?,
        })
    }) {
        Ok(rows) => rows.flatten().collect(),
        Err(_) => return Ok(vec![]),
    };

    if windows.is_empty() {
        return Ok(vec![]);
    }

    // Step 2: for each window, query max ts of pipeline.amend_close event for same (spec, session).
    let mut durations = Vec::with_capacity(windows.len());

    for win in &windows {
        let ev_sql = "SELECT MAX(ts) FROM events \
                      WHERE event='pipeline.amend_close' \
                        AND spec = ?1 \
                        AND session_id = ?2";
        let max_ts: Option<String> = match conn.prepare(ev_sql) {
            Ok(mut s) => s.query_row(params![win.spec_id, win.session_id], |r| r.get(0)).ok().flatten(),
            Err(_) => None,
        };

        if let (Some(event_ts), Some(closed_ms)) = (max_ts, iso_to_ms(&win.closed_at)) {
            if let Some(event_ms) = iso_to_ms(&event_ts) {
                let diff = (closed_ms - event_ms).abs();
                durations.push(diff);
            }
        }
    }

    Ok(durations)
}

// ── Tauri command wrappers ───────────────────────────────────────────────────

#[tauri::command]
pub fn amend_resolution_rate(
    repo_path: String,
) -> Result<f64, String> {
    let base = std::path::PathBuf::from(&repo_path);
    match db::with_db(&base, amend_resolution_rate_query) {
        Some(Ok(v)) => Ok(v),
        Some(Err(e)) => {
            if !e.is_empty() {
                eprintln!("[amend_queries] amend_resolution_rate error: {e}");
            }
            Ok(0.0)
        }
        None => Ok(0.0),
    }
}

#[tauri::command]
pub fn amend_drift_rate(
    repo_path: String,
) -> Result<f64, String> {
    let base = std::path::PathBuf::from(&repo_path);
    match db::with_db(&base, amend_drift_rate_query) {
        Some(Ok(v)) => Ok(v),
        Some(Err(e)) => {
            if !e.is_empty() {
                eprintln!("[amend_queries] amend_drift_rate error: {e}");
            }
            Ok(0.0)
        }
        None => Ok(0.0),
    }
}

#[tauri::command]
pub fn cross_session_amend_count(
    repo_path: String,
) -> Result<u64, String> {
    let base = std::path::PathBuf::from(&repo_path);
    match db::with_db(&base, cross_session_amend_count_query) {
        Some(Ok(v)) => Ok(v),
        Some(Err(e)) => {
            if !e.is_empty() {
                eprintln!("[amend_queries] cross_session_amend_count error: {e}");
            }
            Ok(0)
        }
        None => Ok(0),
    }
}

#[tauri::command]
pub fn amend_window_duration(
    repo_path: String,
) -> Result<Vec<i64>, String> {
    let base = std::path::PathBuf::from(&repo_path);
    match db::with_db(&base, amend_window_duration_query) {
        Some(Ok(v)) => Ok(v),
        Some(Err(e)) => {
            if !e.is_empty() {
                eprintln!("[amend_queries] amend_window_duration error: {e}");
            }
            Ok(vec![])
        }
        None => Ok(vec![]),
    }
}
