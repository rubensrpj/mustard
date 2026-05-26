//! `stop_observer` — SubagentStop reinforcement observer (W8.T8.5).
//!
//! When a subagent stops, we scan its terminal output for any `agent_memory`
//! row whose `summary` substring already lives in the project's memory store
//! and bump its `last_used` timestamp. This signals "this memory was used in
//! this run" to the W8.T8.6 promotion logic and the W7 lazy-decay model.
//!
//! Pure [`Observer`] — never blocks.

use mustard_core::model::contract::{Ctx, HookInput, Observer};
use std::path::Path;

/// The W8 stop-observer hook.
pub struct StopObserver;

/// Resolve the project dir for an invocation.
fn project_dir(input: &HookInput, ctx: &Ctx) -> String {
    if !ctx.project_dir.is_empty() {
        return ctx.project_dir.clone();
    }
    match input.cwd.as_deref() {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => ".".to_string(),
    }
}

/// Best-effort extraction of the subagent's final output. The SubagentStop
/// payload shape varies; probe common locations.
fn final_output(input: &HookInput) -> String {
    for key in ["result", "final_output", "output", "tool_response", "tool_result"] {
        if let Some(v) = input.raw.get(key) {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    return s.to_string();
                }
            }
            if let Some(s) = v.get("text").and_then(|x| x.as_str()) {
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
    }
    String::new()
}

/// Walk `agent_memory` and bump `last_used` for every row whose `summary` is
/// a substring of `text`. Fail-open: errors degrade silently.
fn bump_last_used(cwd: &str, text: &str) {
    let db_path = match std::env::var("MUSTARD_DB_PATH") {
        Ok(p) if !p.trim().is_empty() => std::path::PathBuf::from(p),
        _ => Path::new(cwd)
            .join(".claude")
            .join(".harness")
            .join("mustard.db"),
    };
    if !db_path.exists() {
        return;
    }
    let Ok(conn) = rusqlite::Connection::open(&db_path) else {
        return;
    };
    // Pull a bounded set of recent rows — bumping every row in the table for a
    // long-lived project is wasteful.
    let Ok(mut stmt) = conn.prepare(
        "SELECT id, summary FROM agent_memory \
         WHERE status = 'active' \
         ORDER BY at DESC LIMIT 200",
    ) else {
        return;
    };
    let rows: Vec<(i64, String)> = match stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
    {
        Ok(it) => it.filter_map(std::result::Result::ok).collect(),
        Err(_) => return,
    };
    let now = crate::util::now_iso8601();
    for (id, summary) in rows {
        let trimmed = summary.trim();
        if trimmed.len() < 6 {
            continue;
        }
        if text.contains(trimmed) {
            let _ = conn.execute(
                "UPDATE agent_memory SET last_used = ?1 WHERE id = ?2",
                rusqlite::params![now, id],
            );
        }
    }
}

/// Emit `pipeline.economy.operation.invoked`. Fail-open.
fn emit_economy_operation(cwd: &str, operation: &str) {
    use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use mustard_core::store::event_store::EventSink;
    use mustard_core::store::sqlite_store::SqliteEventStore;
    use serde_json::json;

    let Ok(store) = SqliteEventStore::for_project(cwd) else {
        return;
    };
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: crate::util::now_iso8601(),
        session_id: crate::run::env::session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("stop_observer".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({ "operation": operation, "duration_ms": 0, "tokens_used": 0 }),
        spec: crate::run::env::current_spec(cwd),
    };
    let _ = store.append(&event);
}

impl Observer for StopObserver {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        let output = final_output(input);
        if output.is_empty() {
            return;
        }
        let cwd = project_dir(input, ctx);
        bump_last_used(&cwd, &output);
        emit_economy_operation(&cwd, "stop_observer.bump_last_used");
    }
}

// ---------------------------------------------------------------------------
// W8.T8.6 — SessionEnd consolidation
// ---------------------------------------------------------------------------

/// The W8 SessionEnd consolidation observer.
///
/// Promotes high-confidence (`>= 0.85`) `agent_memory` rows captured during the
/// session into permanent `memory_decisions` / `memory_lessons` rows, then
/// marks the source rows as `promoted` so they are not promoted twice.
pub struct SessionEndConsolidate;

/// Confidence threshold — at or above this, an `agent_memory` row is promoted
/// to a permanent decision/lesson on `SessionEnd`.
pub const PROMOTION_CONFIDENCE_THRESHOLD: f64 = 0.85;

/// Classify a summary as a decision or a lesson. Heuristic: if the leading
/// verb / keyword looks imperative ("Use", "Adopt", "Prefer", "Reject"), it's
/// a decision; otherwise it's a lesson.
fn classify(summary: &str) -> &'static str {
    let head = summary
        .trim_start()
        .split(|c: char| !c.is_ascii_alphabetic())
        .next()
        .unwrap_or("");
    let head_lower = head.to_ascii_lowercase();
    let decision_verbs = [
        "use", "adopt", "prefer", "reject", "switch", "ban", "require", "enforce",
    ];
    if decision_verbs.iter().any(|v| *v == head_lower) {
        "memory_decisions"
    } else {
        "memory_lessons"
    }
}

/// Promote eligible rows. Fail-open on every step.
fn promote_high_confidence(cwd: &str) -> usize {
    let db_path = match std::env::var("MUSTARD_DB_PATH") {
        Ok(p) if !p.trim().is_empty() => std::path::PathBuf::from(p),
        _ => Path::new(cwd)
            .join(".claude")
            .join(".harness")
            .join("mustard.db"),
    };
    if !db_path.exists() {
        return 0;
    }
    let Ok(conn) = rusqlite::Connection::open(&db_path) else {
        return 0;
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT id, summary, details, spec FROM agent_memory \
         WHERE status = 'active' AND confidence >= ?1",
    ) else {
        return 0;
    };
    let rows: Vec<(i64, String, Option<String>, Option<String>)> = match stmt
        .query_map(rusqlite::params![PROMOTION_CONFIDENCE_THRESHOLD], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        }) {
        Ok(it) => it.filter_map(std::result::Result::ok).collect(),
        Err(_) => return 0,
    };
    let now = crate::util::now_iso8601();
    let mut promoted = 0usize;
    for (id, summary, details, spec) in rows {
        let table = classify(&summary);
        let content = if let Some(d) = details {
            format!("{summary}\n\n{d}")
        } else {
            summary.clone()
        };
        let source = spec.unwrap_or_else(|| "agent_memory_promotion".to_string());
        // `memory_decisions` / `memory_lessons` schema: (id, content, source, context, at)
        let sql = format!(
            "INSERT INTO {table} (content, source, context, at) VALUES (?1, ?2, ?3, ?4)"
        );
        if conn
            .execute(
                &sql,
                rusqlite::params![content, source, Option::<String>::None, now],
            )
            .is_ok()
        {
            let _ = conn.execute(
                "UPDATE agent_memory SET status = 'promoted' WHERE id = ?1",
                rusqlite::params![id],
            );
            promoted += 1;
        }
    }
    promoted
}

impl Observer for SessionEndConsolidate {
    fn observe(&self, _input: &HookInput, ctx: &Ctx) {
        let cwd = if ctx.project_dir.is_empty() {
            ".".to_string()
        } else {
            ctx.project_dir.clone()
        };
        let n = promote_high_confidence(&cwd);
        if n > 0 {
            emit_economy_operation(&cwd, "session_end_consolidate.promote");
        }
    }
}

// ---------------------------------------------------------------------------
// W8.T8.7 — PreCompact: add up to 3 recent agent_memory entries to the snapshot
// ---------------------------------------------------------------------------

/// The W8 PreCompact memory snippet — surfaces the three most-recently-used
/// `agent_memory` rows as additional context just before the compaction.
/// Registered separately from `pre_compact` so the W8 deliverable lands inside
/// its declared file boundary (`stop_observer.rs`).
pub struct PreCompactMemorySnippet;

/// Read at most 3 active `agent_memory` summaries ordered by `last_used DESC`.
fn recent_agent_memory(cwd: &str) -> Vec<String> {
    let db_path = match std::env::var("MUSTARD_DB_PATH") {
        Ok(p) if !p.trim().is_empty() => std::path::PathBuf::from(p),
        _ => Path::new(cwd)
            .join(".claude")
            .join(".harness")
            .join("mustard.db"),
    };
    if !db_path.exists() {
        return Vec::new();
    }
    let Ok(conn) = rusqlite::Connection::open(&db_path) else {
        return Vec::new();
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT summary FROM agent_memory \
         WHERE status = 'active' \
         ORDER BY COALESCE(last_used, at) DESC LIMIT 3",
    ) else {
        return Vec::new();
    };
    let rows = stmt.query_map([], |r| r.get::<_, String>(0));
    match rows {
        Ok(it) => it.filter_map(std::result::Result::ok).collect(),
        Err(_) => Vec::new(),
    }
}

impl mustard_core::model::contract::Check for PreCompactMemorySnippet {
    fn evaluate(
        &self,
        input: &HookInput,
        ctx: &Ctx,
    ) -> Result<mustard_core::model::contract::Verdict, mustard_core::error::Error> {
        use mustard_core::model::contract::{Trigger, Verdict};
        if ctx.trigger != Some(Trigger::PreCompact) {
            return Ok(Verdict::Allow);
        }
        let cwd = project_dir(input, ctx);
        let entries = recent_agent_memory(&cwd);
        if entries.is_empty() {
            return Ok(Verdict::Allow);
        }
        emit_economy_operation(&cwd, "pre_compact_memory_snippet.inject");
        let body: String = entries
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(Verdict::Inject {
            context: format!("[Agent memory — recent]\n{body}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use tempfile::tempdir;

    fn seed_row(conn: &rusqlite::Connection, summary: &str) -> i64 {
        // Apply the W0 DDL for agent_memory if not already present (the W5
        // schema migration is applied by `SqliteEventStore::for_project`; in
        // these unit tests we apply it inline).
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS agent_memory ( \
                id INTEGER PRIMARY KEY AUTOINCREMENT, \
                session_id TEXT, spec TEXT, wave INTEGER, role TEXT, \
                summary TEXT NOT NULL, details TEXT, \
                confidence REAL NOT NULL DEFAULT 0.5, \
                status TEXT NOT NULL DEFAULT 'active', \
                at TEXT NOT NULL, last_used TEXT \
            );",
        )
        .unwrap();
        let now = crate::util::now_iso8601();
        conn.execute(
            "INSERT INTO agent_memory (summary, at, last_used) VALUES (?1, ?2, ?2)",
            params![summary, now],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn bump_last_used_updates_matching_row() {
        let dir = tempdir().unwrap();
        let db_dir = dir.path().join(".claude").join(".harness");
        std::fs::create_dir_all(&db_dir).unwrap();
        let db_path = db_dir.join("mustard.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let id = seed_row(&conn, "MUSTARD-W8-MARKER-XYZZY-PROOF");
        // Snapshot the original last_used.
        let before: String = conn
            .query_row(
                "SELECT last_used FROM agent_memory WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        drop(conn);

        // Sleep a couple ms to ensure ISO8601 differs at the second/ms level
        // on platforms with coarse clocks.
        std::thread::sleep(std::time::Duration::from_millis(20));
        bump_last_used(
            &dir.path().to_string_lossy(),
            "stuff before MUSTARD-W8-MARKER-XYZZY-PROOF stuff after",
        );

        let conn2 = rusqlite::Connection::open(&db_path).unwrap();
        let after: String = conn2
            .query_row(
                "SELECT last_used FROM agent_memory WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_ne!(after, before, "last_used should have advanced");
    }
}
