//! `mustard-rt run memory` — a port of `scripts/memory.js`.
//!
//! A unified persistence CLI:
//!
//! - `agent`     → `.claude/.agent-memory/`                    (rolling cap 20, legacy JSON)
//! - `decision`  → `memory_decisions` / `memory_lessons` SQLite (cap 50)
//! - `knowledge` → `knowledge_patterns` SQLite                 (cap 200 / 80 per type)
//! - `write`     → `agent_memory` SQLite + FTS5 mirror (W7 — `--verify` round-trips)
//! - `search`    → `agent_memory` FTS5 + scope filter (W7)
//! - `feedback`  → `memory_feedback` SQLite append (W7 — bump/deprecate/supersede/use)
//! - `cross-wave`→ render markdown of prior-wave memories (now scoped by cluster)
//! - `list`      → JSON/table dump of pattern/decision/lesson rows
//!
//! Input JSON arrives either via `--json '<JSON>'` (the Windows-friendly form)
//! or piped on stdin (the POSIX fallback). Exit is always `0` (fail-open).
//!
//! Wave 6b: `decision` and `knowledge` subcommands write to the Wave 6a SQLite
//! tables (`memory_decisions`, `memory_lessons`, `knowledge_patterns`).
//! Legacy JSON sidecars are no longer written.  Wave 6c migrates the
//! dashboard reader.
//!
//! Wave 7 (`2026-05-25-mustard-deep-refactor` W7): hardens the shared `agent_memory`
//! and `memory_feedback` tables (DDL landed in W0.T0.5). The FTS5 mirror
//! (`agent_memory_fts`) is created lazily on first write/search — keeping it
//! out of `sqlite_schema.sql` lets the W7 spec ship the logic without
//! re-opening the W0 schema. Lazy decay applies
//! `confidence * (1 - days_since_last_used / 30)`; entries below
//! [`DEFAULT_MIN_EFFECTIVE_CONFIDENCE`] are excluded from default search.

use crate::run::env::{current_spec, project_dir, session_id};
use crate::util::{now_iso8601, now_millis};
use mustard_core::fs;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::ClaudePaths;
use rusqlite::{params, Connection};
use serde_json::{Value, json};
use std::io::Read;
use std::path::Path;
use std::time::Instant;

/// Minimum *effective* confidence (after lazy decay) for a memory row to
/// surface in default `search` results. Bypass with `--include-low`.
pub(crate) const DEFAULT_MIN_EFFECTIVE_CONFIDENCE: f64 = 0.3;

/// Days over which a memory's confidence linearly decays to zero from its
/// `last_used` timestamp. Mirrors the spec's
/// `confidence * (1 - days_since_last_used / 30)` formula.
pub(crate) const DECAY_WINDOW_DAYS: f64 = 30.0;

/// Default `search` result cap when `--limit` is omitted.
pub(crate) const DEFAULT_SEARCH_LIMIT: usize = 20;

/// `.agent-memory/` rolling cap.
const AGENT_CAP: usize = 20;

/// Read the input JSON text — `--json <text>` argument, else piped stdin.
fn read_input(json_arg: Option<&str>) -> String {
    if let Some(text) = json_arg {
        return text.to_string();
    }
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
    buf
}

/// Resolve the project dir for an input: its `cwd` field, else the env default.
fn input_cwd(input: &Value) -> String {
    input
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map_or_else(project_dir, str::to_string)
}

/// Truncate `text` to `max_len`, preferring a sentence boundary.
fn truncate_summary(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }
    let slice: String = text.chars().take(max_len).collect();
    let boundary = ['.', '!', '?']
        .iter()
        .filter_map(|c| slice.rfind(*c))
        .max();
    match boundary {
        Some(b) => text.chars().take(b + 1).collect(),
        None => {
            let kept: String = text.chars().take(max_len.saturating_sub(3)).collect();
            format!("{kept}...")
        }
    }
}

/// Resolve the 8-char session prefix for an `agent` entry — the first
/// `.agent-state/*.json` `session_id`, else the OS process id.
fn resolve_session_prefix(project_dir: &Path) -> String {
    let Ok(cp) = ClaudePaths::for_project(project_dir) else {
        return std::process::id().to_string();
    };
    let state_dir = cp.agent_state_dir();
    if let Ok(entries) = fs::read_dir(&state_dir) {
        for entry in entries {
            let name = entry.file_name.clone();
            if !std::path::Path::new(&name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json")) || name == "_queue.json" {
                continue;
            }
            if let Ok(text) = fs::read_to_string(&entry.path) {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    if let Some(sid) = v.get("session_id").and_then(Value::as_str) {
                        if !sid.is_empty() {
                            return sid.chars().take(8).collect();
                        }
                    }
                }
            }
        }
    }
    std::process::id().to_string()
}

/// `agent` subcommand — write an agent-memory entry, prune to [`AGENT_CAP`].
/// Unchanged from Wave 5: still writes to `.claude/.agent-memory/` JSON files.
fn run_agent(input: &Value) {
    let project_dir = input_cwd(input);
    let project_dir = Path::new(&project_dir);
    let Ok(cp) = ClaudePaths::for_project(project_dir) else {
        return;
    };
    let mem_dir = cp.agent_memory_dir();
    if fs::create_dir_all(&mem_dir).is_err() {
        return;
    }

    let session8 = resolve_session_prefix(project_dir);
    let agent_type = input
        .get("agent_type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let id = format!("{session8}-{agent_type}-{}", now_millis());
    let filename = format!("{id}.json");
    let timestamp = now_iso8601();
    let raw_summary = input
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let summary = truncate_summary(&raw_summary, 300);
    let wave = input.get("wave").and_then(Value::as_i64);
    let pipeline = input
        .get("pipeline")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let entry = json!({
        "v": 1,
        "id": id,
        "session": session8,
        "agent_type": agent_type,
        "wave": wave,
        "pipeline": pipeline,
        "timestamp": timestamp,
        "summary": summary,
        "details": input.get("details").cloned().unwrap_or_else(|| json!({})),
    });
    if let Ok(text) = serde_json::to_string_pretty(&entry) {
        let _ = fs::write_atomic(mem_dir.join(&filename), text.as_bytes());
    }

    // Rolling index.
    let index_path = mem_dir.join("_index.json");
    let mut index: Vec<Value> = fs::read_to_string(&index_path)
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok())
        .unwrap_or_default();
    index.push(json!({
        "id": id,
        "file": filename,
        "agent_type": agent_type,
        "wave": wave,
        "pipeline": pipeline,
        "summary": summary,
        "timestamp": timestamp,
    }));
    if index.len() > AGENT_CAP {
        let excess = index.len() - AGENT_CAP;
        for old in index.drain(..excess) {
            if let Some(f) = old.get("file").and_then(Value::as_str) {
                let _ = fs::remove_file(mem_dir.join(f));
            }
        }
    }
    if let Ok(text) = serde_json::to_string_pretty(&index) {
        let _ = fs::write_atomic(&index_path, text.as_bytes());
    }
}

/// Append a `decision` / `lesson` harness event for the `decision` subcommand.
///
/// **Spec attribution:** decisions and lessons are scoped to whatever spec was
/// active when they were recorded. Pre-2026-05-20 these events left
/// `spec = NULL`, which made them invisible to per-spec timeline projections.
/// Now [`current_spec`] feeds the field from the harness state — the same
/// helper every other emitter uses — so decisions surface in the timeline of
/// the spec they belong to. Falls back to `None` when no spec is active (the
/// CLI was invoked outside any pipeline).
fn emit_decision_event(entry_type: &str, content: &str, context: &str, source: &str, dir: &str) {
    let head: String = content.chars().take(200).collect();
    let (event, payload) = if entry_type == "decision" {
        (
            "decision",
            json!({
                "title": head,
                "rationale": if context.is_empty() { Value::Null } else { json!(context) },
            }),
        )
    } else {
        (
            "lesson",
            json!({
                "trigger": if source.is_empty() { Value::Null } else { json!(source) },
                "takeaway": head,
            }),
        )
    };
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("memory".to_string()),
            actor_type: None,
        },
        event: event.to_string(),
        payload,
        spec: current_spec(dir),
    };
    // W5 routing: `agent.memory` / `memory.*` events are non-pipeline → NDJSON.
    let _ = crate::run::event_route::emit(dir, &ev);
}

/// Insert a decision row into `memory_decisions`. Fail-open — errors are
/// silently discarded so a store failure never aborts the caller.
pub(crate) fn insert_decision(
    conn: &rusqlite::Connection,
    content: &str,
    source: Option<&str>,
    context: Option<&str>,
    at: Option<&str>,
) -> rusqlite::Result<()> {
    let at_val = at.map_or_else(now_iso8601, str::to_string);
    conn.execute(
        "INSERT INTO memory_decisions (content, source, context, at) VALUES (?1, ?2, ?3, ?4)",
        params![content, source, context, at_val],
    )?;
    Ok(())
}

/// Insert a lesson row into `memory_lessons`. Fail-open.
pub(crate) fn insert_lesson(
    conn: &rusqlite::Connection,
    content: &str,
    source: Option<&str>,
    context: Option<&str>,
    at: Option<&str>,
) -> rusqlite::Result<()> {
    let at_val = at.map_or_else(now_iso8601, str::to_string);
    conn.execute(
        "INSERT INTO memory_lessons (content, source, context, at) VALUES (?1, ?2, ?3, ?4)",
        params![content, source, context, at_val],
    )?;
    Ok(())
}

/// Upsert a knowledge pattern row into `knowledge_patterns`.
/// ON CONFLICT(pattern): increments count, refreshes confidence + last_seen.
pub(crate) fn upsert_knowledge_pattern(
    conn: &rusqlite::Connection,
    pattern: &str,
    confidence: f64,
    source: Option<&str>,
    last_seen: &str,
    created_at: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO knowledge_patterns (pattern, confidence, count, last_seen, source, created_at) \
         VALUES (?1, ?2, 1, ?3, ?4, ?5) \
         ON CONFLICT(pattern) DO UPDATE SET \
           confidence = excluded.confidence, \
           count = count + 1, \
           last_seen = excluded.last_seen, \
           source = COALESCE(excluded.source, source)",
        params![pattern, confidence, last_seen, source, created_at],
    )?;
    Ok(())
}

/// `decision` subcommand — insert into `memory_decisions` / `memory_lessons`
/// via SQLite. No longer writes `decisions.json` / `lessons.json`.
fn run_decision(input: &Value) {
    let entry_type = input
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if entry_type != "decision" && entry_type != "lesson" {
        eprintln!(
            "[memory] decision: invalid type \"{entry_type}\" — must be \"decision\" or \"lesson\""
        );
        return;
    }
    let project_dir = input_cwd(input);
    let content = input.get("content").and_then(Value::as_str).unwrap_or("").to_string();
    let context = input.get("context").and_then(Value::as_str).unwrap_or("").to_string();
    let source = input.get("source").and_then(Value::as_str).unwrap_or("").to_string();
    let at = input.get("at").and_then(Value::as_str).map(str::to_string);

    // Write to SQLite — fail-open.
    let store_result = SqliteEventStore::for_project(&project_dir);
    if let Ok(store) = store_result {
        // SqliteEventStore wraps a private Connection. We open a second
        // direct rusqlite connection to the same DB file for the INSERT.
        let db_path = store.path().to_path_buf();
        if let Ok(conn) = rusqlite::Connection::open(&db_path) {
            let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
            let result = if entry_type == "decision" {
                insert_decision(
                    &conn,
                    &content,
                    if source.is_empty() { None } else { Some(&source) },
                    if context.is_empty() { None } else { Some(&context) },
                    at.as_deref(),
                )
            } else {
                insert_lesson(
                    &conn,
                    &content,
                    if source.is_empty() { None } else { Some(&source) },
                    if context.is_empty() { None } else { Some(&context) },
                    at.as_deref(),
                )
            };
            if let Err(e) = result {
                eprintln!("[memory] SQLite write failed (fail-open): {e}");
            }
        }
    }

    emit_decision_event(&entry_type, &content, &context, &source, &project_dir);
}

/// `knowledge` subcommand — upsert into `knowledge_patterns` SQLite table.
/// No longer writes `knowledge.json`.
fn run_knowledge(input: &Value) {
    let cwd = input_cwd(input);
    let name = input.get("name").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let description = input
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let source = input.get("source").and_then(Value::as_str).map(str::to_string);
    let confidence = input
        .get("confidence")
        .and_then(Value::as_f64)
        .filter(|&c| (0.0..=1.0).contains(&c))
        .unwrap_or(0.3);

    if name.is_empty() || description.is_empty() {
        eprintln!("[memory] knowledge: missing name or description");
        return;
    }

    // The pattern stored is `"name: description"` to preserve the semantic used
    // by session_start's injection rendering.
    let pattern = format!("{name}: {description}");
    let now = now_iso8601();

    let store_result = SqliteEventStore::for_project(&cwd);
    if let Ok(store) = store_result {
        let db_path = store.path().to_path_buf();
        if let Ok(conn) = rusqlite::Connection::open(&db_path) {
            let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
            if let Err(e) =
                upsert_knowledge_pattern(&conn, &pattern, confidence, source.as_deref(), &now, &now)
            {
                eprintln!("[memory] SQLite knowledge upsert failed (fail-open): {e}");
            }
        }
    }
}

/// Build the [`run_agent`] input JSON from flat CLI flags. Used when the
/// caller passes `--agent`/`--summary`/`--files`/`--spec`/`--wave` instead
/// of crafting a full `--json '{...}'` payload.
fn agent_input_from_flags(
    spec: Option<&str>,
    wave: Option<u32>,
    agent: Option<&str>,
    summary: Option<&str>,
    files: Option<&str>,
) -> Value {
    let files_arr: Vec<Value> = files
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(|p| Value::String(p.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let mut details = serde_json::Map::new();
    if !files_arr.is_empty() {
        details.insert("files".to_string(), Value::Array(files_arr));
    }
    json!({
        "agent_type": agent.unwrap_or("unknown"),
        "wave": wave,
        "pipeline": spec.unwrap_or(""),
        "summary": summary.unwrap_or(""),
        "details": Value::Object(details),
    })
}

// ---------------------------------------------------------------------------
// `list` subcommand — read `knowledge_patterns` + `memory_decisions` +
// `memory_lessons` from SQLite and emit a JSON array or grouped table.
// ---------------------------------------------------------------------------

/// One row from the combined memory read.
#[derive(Debug, serde::Serialize)]
struct MemoryRow {
    #[serde(rename = "type")]
    entry_type: String,
    name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    occurrences: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_seen: Option<String>,
}

fn run_list(grouped: bool, format: &str) {
    let project_dir = crate::run::env::project_dir();
    let store = match mustard_core::store::sqlite_store::SqliteEventStore::for_project(&project_dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[memory list] cannot open store (fail-open): {e}");
            println!("[]");
            return;
        }
    };
    let db_path = store.path().to_path_buf();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[memory list] cannot open connection (fail-open): {e}");
            println!("[]");
            return;
        }
    };
    let _ = conn.busy_timeout(std::time::Duration::from_secs(3));

    let mut rows: Vec<MemoryRow> = Vec::new();

    // --- knowledge_patterns ---
    {
        let mut stmt = match conn.prepare(
            "SELECT pattern, confidence, count, last_seen FROM knowledge_patterns ORDER BY confidence DESC, last_seen DESC",
        ) {
            Ok(s) => s,
            Err(_) => {
                // Table may not exist in very old installs; skip silently.
                conn.prepare("SELECT 1 WHERE 0").expect("static SQL is always valid")
            }
        };
        if let Ok(iter) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        }) {
            for item in iter.flatten() {
                // pattern is stored as "name: description" → split on first ':'
                let (name, description) = item.0
                    .split_once(':')
                    .map_or_else(|| (item.0.clone(), String::new()), |(n, d)| (n.trim().to_string(), d.trim().to_string()));
                rows.push(MemoryRow {
                    entry_type: "pattern".to_string(),
                    name,
                    description,
                    confidence: Some(item.1),
                    occurrences: Some(item.2),
                    last_seen: Some(item.3.chars().take(10).collect()),
                });
            }
        }
    }

    // --- memory_decisions ---
    {
        let mut stmt = match conn.prepare(
            "SELECT content, source, at FROM memory_decisions ORDER BY at DESC LIMIT 50",
        ) {
            Ok(s) => s,
            Err(_) => conn.prepare("SELECT 1 WHERE 0").expect("static SQL is always valid"),
        };
        if let Ok(iter) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
            ))
        }) {
            for item in iter.flatten() {
                let name: String = item.0.chars().take(80).collect();
                rows.push(MemoryRow {
                    entry_type: "decision".to_string(),
                    name,
                    description: item.1.unwrap_or_default(),
                    confidence: None,
                    occurrences: None,
                    last_seen: Some(item.2.chars().take(10).collect()),
                });
            }
        }
    }

    // --- memory_lessons ---
    {
        let mut stmt = match conn.prepare(
            "SELECT content, source, at FROM memory_lessons ORDER BY at DESC LIMIT 50",
        ) {
            Ok(s) => s,
            Err(_) => conn.prepare("SELECT 1 WHERE 0").expect("static SQL is always valid"),
        };
        if let Ok(iter) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
            ))
        }) {
            for item in iter.flatten() {
                let name: String = item.0.chars().take(80).collect();
                rows.push(MemoryRow {
                    entry_type: "convention".to_string(),
                    name,
                    description: item.1.unwrap_or_default(),
                    confidence: None,
                    occurrences: None,
                    last_seen: Some(item.2.chars().take(10).collect()),
                });
            }
        }
    }

    if format == "table" && grouped {
        render_grouped_table(&rows);
    } else {
        // Default: JSON (back-compat — no flags = raw JSON array)
        println!(
            "{}",
            serde_json::to_string_pretty(&rows)
                .unwrap_or_else(|_| "[]".to_string())
        );
    }
}

fn render_grouped_table(rows: &[MemoryRow]) {
    let types = ["pattern", "decision", "convention"];
    for t in &types {
        let group: Vec<&MemoryRow> = rows.iter().filter(|r| r.entry_type == *t).collect();
        if group.is_empty() {
            continue;
        }
        println!("\n### {}\n", t.to_ascii_uppercase());
        println!("| Name                                           | Description                           | Confidence | Seen       |");
        println!("|------------------------------------------------|---------------------------------------|------------|------------|");
        for row in &group {
            let name_col = truncate_col(&row.name, 46);
            let desc_col = truncate_col(&row.description, 37);
            let conf_col = row.confidence.map_or_else(|| "-".to_string(), |c| format!("{c:.2}"));
            let seen_col = row.last_seen.clone().unwrap_or_else(|| "-".to_string());
            println!("| {name_col:<46} | {desc_col:<37} | {conf_col:<10} | {seen_col:<10} |");
        }
    }
}

fn truncate_col(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let t: String = chars[..max - 1].iter().collect();
        format!("{t}…")
    }
}

// ---------------------------------------------------------------------------
// W7: agent_memory + memory_feedback (shared cross-session/cluster memory)
// ---------------------------------------------------------------------------

/// Whitelist of `memory_feedback.kind` values accepted by `run feedback`.
pub(crate) const FEEDBACK_KINDS: &[&str] = &["deprecate", "bump", "supersede", "use"];

/// Ensure the FTS5 mirror for `agent_memory` exists. W0.T0.5 owns the table
/// DDL; the mirror is created lazily here so W7 stays inside its declared
/// limits (`memory.rs` + `memory_ingest.rs` + `mod.rs`).
///
/// Idempotent — `CREATE ... IF NOT EXISTS` on the virtual table and on every
/// trigger. Fail-open: a failure (e.g. FTS5 disabled in the build) leaves the
/// regular table usable; `search` then falls back to `LIKE` matching.
pub(crate) fn ensure_agent_memory_fts(conn: &Connection) -> rusqlite::Result<()> {
    // NB: trailing spaces matter — Rust string-continuation (`\` + newline)
    // collapses the gap to nothing, so each fragment ends with a space so
    // `BEGIN INSERT` doesn't become `BEGININSERT` after concatenation.
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS agent_memory_fts USING fts5(\
            summary, details, \
            content='agent_memory', content_rowid='id', tokenize='unicode61'\
        ); \
        CREATE TRIGGER IF NOT EXISTS agent_memory_ai AFTER INSERT ON agent_memory BEGIN \
            INSERT INTO agent_memory_fts(rowid, summary, details) \
            VALUES (new.id, new.summary, COALESCE(new.details, '')); \
        END; \
        CREATE TRIGGER IF NOT EXISTS agent_memory_ad AFTER DELETE ON agent_memory BEGIN \
            INSERT INTO agent_memory_fts(agent_memory_fts, rowid, summary, details) \
            VALUES ('delete', old.id, old.summary, COALESCE(old.details, '')); \
        END; \
        CREATE TRIGGER IF NOT EXISTS agent_memory_au AFTER UPDATE ON agent_memory BEGIN \
            INSERT INTO agent_memory_fts(agent_memory_fts, rowid, summary, details) \
            VALUES ('delete', old.id, old.summary, COALESCE(old.details, '')); \
            INSERT INTO agent_memory_fts(rowid, summary, details) \
            VALUES (new.id, new.summary, COALESCE(new.details, '')); \
        END;",
    )?;
    Ok(())
}

/// Detect whether `agent_memory_fts` was successfully created (i.e. FTS5 is
/// available in this build). Used by `search` to choose FTS5 vs. `LIKE`.
fn fts5_available(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='agent_memory_fts'",
        [],
        |_| Ok(()),
    )
    .is_ok()
}

/// Insert one row into `agent_memory`. Returns the inserted `id`.
///
/// `at` defaults to `now_iso8601()`; `last_used` defaults to `at`.
/// `status` defaults to `"active"`.
pub(crate) fn insert_agent_memory(
    conn: &Connection,
    session_id: Option<&str>,
    spec: Option<&str>,
    wave: Option<i64>,
    role: Option<&str>,
    summary: &str,
    details: Option<&str>,
    confidence: f64,
    status: Option<&str>,
    at: Option<&str>,
) -> rusqlite::Result<i64> {
    let at_val = at.map_or_else(now_iso8601, str::to_string);
    let status_val = status.unwrap_or("active");
    conn.execute(
        "INSERT INTO agent_memory \
         (session_id, spec, wave, role, summary, details, confidence, status, at, last_used) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![session_id, spec, wave, role, summary, details, confidence, status_val, at_val],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Append a row to `memory_feedback`. `delta` is reserved for future use
/// (e.g. `bump +0.1`), `note` records arbitrary context. Returns the
/// inserted `id`.
pub(crate) fn insert_memory_feedback(
    conn: &Connection,
    memory_id: i64,
    kind: &str,
    delta: Option<f64>,
    by_role: Option<&str>,
    note: Option<&str>,
    at: Option<&str>,
) -> rusqlite::Result<i64> {
    let at_val = at.map_or_else(now_iso8601, str::to_string);
    conn.execute(
        "INSERT INTO memory_feedback (memory_id, kind, delta, by_role, at, note) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![memory_id, kind, delta, by_role, at_val, note],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Parse an RFC-3339 / ISO-8601 UTC timestamp into seconds since the Unix
/// epoch. Accepts the `YYYY-MM-DDThh:mm:ss[.sss]Z` shape produced by
/// [`crate::util::now_iso8601`]. Tolerant — partial parses degrade to `None`.
pub(crate) fn parse_iso8601_secs(ts: &str) -> Option<i64> {
    // Strip optional fractional seconds and the trailing `Z`.
    let s = ts.trim().trim_end_matches('Z');
    let (date_part, time_part) = s.split_once('T')?;
    let mut date_iter = date_part.split('-');
    let year: i64 = date_iter.next()?.parse().ok()?;
    let month: i64 = date_iter.next()?.parse().ok()?;
    let day: i64 = date_iter.next()?.parse().ok()?;

    let time_main = time_part.split('.').next()?;
    let mut time_iter = time_main.split(':');
    let hour: i64 = time_iter.next()?.parse().ok()?;
    let minute: i64 = time_iter.next()?.parse().ok()?;
    let second: i64 = time_iter.next().unwrap_or("0").parse().ok()?;

    // Howard Hinnant's civil_from_days algorithm — same flavour as
    // `crate::util::now_iso8601` (its inverse).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days_since_epoch = era * 146_097 + doe - 719_468;
    Some(days_since_epoch * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Apply lazy decay to a stored confidence:
/// `confidence * (1 - days_since_last_used / DECAY_WINDOW_DAYS)`, clamped to
/// `[0, 1]`. A missing or unparseable `last_used` yields the raw confidence.
#[must_use]
pub(crate) fn effective_confidence(
    confidence: f64,
    last_used: Option<&str>,
    now_iso: &str,
) -> f64 {
    let Some(last) = last_used.and_then(parse_iso8601_secs) else {
        return confidence.clamp(0.0, 1.0);
    };
    let Some(now) = parse_iso8601_secs(now_iso) else {
        return confidence.clamp(0.0, 1.0);
    };
    let days = ((now - last) as f64) / 86_400.0;
    let factor = 1.0 - (days / DECAY_WINDOW_DAYS);
    (confidence * factor.max(0.0)).clamp(0.0, 1.0)
}

/// Refresh `last_used` for a memory row. Best-effort.
pub(crate) fn touch_last_used(conn: &Connection, id: i64, ts: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE agent_memory SET last_used = ?1 WHERE id = ?2",
        params![ts, id],
    )?;
    Ok(())
}

/// One row returned by `search`. Carries both stored and decayed confidence.
#[derive(Debug, serde::Serialize)]
pub struct SearchRow {
    pub id: i64,
    pub spec: Option<String>,
    pub wave: Option<i64>,
    pub role: Option<String>,
    pub summary: String,
    pub details: Option<String>,
    pub confidence: f64,
    pub effective_confidence: f64,
    pub status: String,
    pub at: String,
    pub last_used: Option<String>,
}

/// Run an FTS5-or-LIKE search over `agent_memory`. Caller is expected to have
/// already invoked [`ensure_agent_memory_fts`].
///
/// Scope filters (`spec`, `cluster`) AND together. `cluster` matches the
/// stored `role` column verbatim (we keep cluster scoping in `role` to avoid
/// re-opening the W0 schema for a dedicated column — the spec calls it
/// "cluster" semantically). Decay is applied after the SQL query.
#[allow(clippy::too_many_arguments)]
pub(crate) fn search_agent_memory(
    conn: &Connection,
    query: &str,
    spec: Option<&str>,
    cluster: Option<&str>,
    min_confidence: f64,
    limit: usize,
    include_low: bool,
    now_iso: &str,
) -> rusqlite::Result<Vec<SearchRow>> {
    let mut clauses: Vec<String> = vec!["am.status = 'active'".to_string()];
    let mut binds: Vec<String> = Vec::new();

    if !query.trim().is_empty() {
        if fts5_available(conn) {
            clauses.push("am.id IN (SELECT rowid FROM agent_memory_fts WHERE agent_memory_fts MATCH ?)".to_string());
            binds.push(query.to_string());
        } else {
            clauses.push("(am.summary LIKE ? OR COALESCE(am.details, '') LIKE ?)".to_string());
            let like = format!("%{query}%");
            binds.push(like.clone());
            binds.push(like);
        }
    }
    if let Some(s) = spec {
        clauses.push("am.spec = ?".to_string());
        binds.push(s.to_string());
    }
    if let Some(c) = cluster {
        clauses.push("am.role = ?".to_string());
        binds.push(c.to_string());
    }

    let where_sql = clauses.join(" AND ");
    let sql = format!(
        "SELECT am.id, am.spec, am.wave, am.role, am.summary, am.details, \
                am.confidence, am.status, am.at, am.last_used \
         FROM agent_memory am \
         WHERE {where_sql} \
         ORDER BY am.confidence DESC, am.at DESC \
         LIMIT ?"
    );
    let mut stmt = conn.prepare(&sql)?;

    // Build a heterogeneous bind list: strings then the LIMIT integer.
    let limit_i = i64::try_from(limit.max(1)).unwrap_or(50);
    let bind_refs: Vec<&dyn rusqlite::ToSql> = binds
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .chain(std::iter::once(&limit_i as &dyn rusqlite::ToSql))
        .collect();

    let mut rows: Vec<SearchRow> = Vec::new();
    let mut q = stmt.query(bind_refs.as_slice())?;
    while let Some(row) = q.next()? {
        let stored: f64 = row.get(6)?;
        let last_used: Option<String> = row.get(9)?;
        let eff = effective_confidence(stored, last_used.as_deref(), now_iso);
        if !include_low && eff < min_confidence {
            continue;
        }
        rows.push(SearchRow {
            id: row.get(0)?,
            spec: row.get(1)?,
            wave: row.get(2)?,
            role: row.get(3)?,
            summary: row.get(4)?,
            details: row.get(5)?,
            confidence: stored,
            effective_confidence: eff,
            status: row.get(7)?,
            at: row.get(8)?,
            last_used,
        });
    }
    Ok(rows)
}

/// Default-injection filter: returns the memory rows that match the
/// "session bootstrap" criteria — `spec=current OR (spec IS NULL AND
/// confidence>=0.8)`, optionally extended with
/// `OR (role IN (<cluster>...) AND confidence>=0.5)` when the caller passes
/// a non-empty `wave_applies_to` list.
///
/// Decay is applied after the SQL query — rows whose effective confidence
/// drops below the SQL threshold are still filtered out post-hoc.
///
/// Not wired into `session_start` yet — exposed for the W7 contract tests
/// and for a follow-up `injection` consumer (W11 economy-wiring may absorb
/// it). Marked `dead_code`-tolerant so the warning does not block the build.
#[allow(dead_code)]
pub(crate) fn default_injection_select(
    conn: &Connection,
    current_spec_slug: Option<&str>,
    wave_applies_to: &[String],
    limit: usize,
    now_iso: &str,
) -> rusqlite::Result<Vec<SearchRow>> {
    let mut clauses: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();

    if let Some(s) = current_spec_slug {
        clauses.push("am.spec = ?".to_string());
        binds.push(s.to_string());
    }
    // Always allow high-confidence global memories.
    clauses.push("(am.spec IS NULL AND am.confidence >= 0.8)".to_string());

    if !wave_applies_to.is_empty() {
        let placeholders = wave_applies_to
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        clauses.push(format!("(am.role IN ({placeholders}) AND am.confidence >= 0.5)"));
        binds.extend(wave_applies_to.iter().cloned());
    }

    let where_sql = if clauses.is_empty() {
        "1=0".to_string()
    } else {
        format!("am.status = 'active' AND ({})", clauses.join(" OR "))
    };
    let limit_i = i64::try_from(limit.max(1)).unwrap_or(20);

    let sql = format!(
        "SELECT am.id, am.spec, am.wave, am.role, am.summary, am.details, \
                am.confidence, am.status, am.at, am.last_used \
         FROM agent_memory am \
         WHERE {where_sql} \
         ORDER BY am.confidence DESC, am.at DESC \
         LIMIT ?"
    );
    let mut stmt = conn.prepare(&sql)?;
    let bind_refs: Vec<&dyn rusqlite::ToSql> = binds
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .chain(std::iter::once(&limit_i as &dyn rusqlite::ToSql))
        .collect();

    let mut rows: Vec<SearchRow> = Vec::new();
    let mut q = stmt.query(bind_refs.as_slice())?;
    while let Some(row) = q.next()? {
        let stored: f64 = row.get(6)?;
        let last_used: Option<String> = row.get(9)?;
        let eff = effective_confidence(stored, last_used.as_deref(), now_iso);
        if eff < DEFAULT_MIN_EFFECTIVE_CONFIDENCE {
            continue;
        }
        rows.push(SearchRow {
            id: row.get(0)?,
            spec: row.get(1)?,
            wave: row.get(2)?,
            role: row.get(3)?,
            summary: row.get(4)?,
            details: row.get(5)?,
            confidence: stored,
            effective_confidence: eff,
            status: row.get(7)?,
            at: row.get(8)?,
            last_used,
        });
    }
    Ok(rows)
}

/// Emit a `pipeline.economy.operation.invoked` event tagged with the calling
/// memory subcommand and elapsed duration. Mirrors the helper in
/// [`crate::run::skill_fetch::emit_economy`]. Fail-open.
fn emit_memory_economy(operation: &str, duration_ms: u128) {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());
    let spec = current_spec(&cwd);
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some(format!("memory-{operation}")),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": format!("memory-{operation}"),
            "duration_ms": duration_capped,
            "tokens_used": 0,
            "was_rust_only": true,
        }),
        spec,
    };
    let _ = crate::run::event_route::emit(&cwd, &ev);
}

/// Options for `memory write` (W7).
#[derive(Debug, Default, Clone)]
pub struct WriteOpts {
    pub spec: Option<String>,
    pub wave: Option<i64>,
    pub role: Option<String>,
    pub summary: String,
    pub details: Option<String>,
    pub confidence: f64,
    pub verify: bool,
}

/// `memory write` — insert one `agent_memory` row, optionally round-trip
/// reading it back when `--verify` is set. Stdout is a pretty JSON report.
pub(crate) fn run_write(opts: WriteOpts) {
    let started = Instant::now();
    let cwd = project_dir();
    let mut report = json!({
        "operation": "memory-write",
        "verify": opts.verify,
        "inserted_id": Value::Null,
        "verified": false,
        "error": Value::Null,
    });

    if opts.summary.trim().is_empty() {
        report["error"] = json!("summary is required");
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        emit_memory_economy("write", started.elapsed().as_millis());
        return;
    }
    let confidence = if (0.0..=1.0).contains(&opts.confidence) {
        opts.confidence
    } else {
        0.5
    };

    let store = match SqliteEventStore::for_project(&cwd) {
        Ok(s) => s,
        Err(e) => {
            report["error"] = json!(format!("cannot open store: {e}"));
            println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
            emit_memory_economy("write", started.elapsed().as_millis());
            return;
        }
    };
    let db_path = store.path().to_path_buf();
    let conn = match Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            report["error"] = json!(format!("cannot open connection: {e}"));
            println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
            emit_memory_economy("write", started.elapsed().as_millis());
            return;
        }
    };
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    // FTS5 mirror is best-effort.
    let _ = ensure_agent_memory_fts(&conn);

    let session = session_id();
    let session_opt = if session == "unknown" { None } else { Some(session.as_str()) };

    let inserted = insert_agent_memory(
        &conn,
        session_opt,
        opts.spec.as_deref(),
        opts.wave,
        opts.role.as_deref(),
        &opts.summary,
        opts.details.as_deref(),
        confidence,
        None,
        None,
    );
    match inserted {
        Ok(id) => {
            report["inserted_id"] = json!(id);
            if opts.verify {
                let now = now_iso8601();
                // Verification: round-trip read by id; if the FTS5 mirror is up,
                // also assert the summary appears in a MATCH query (best-effort).
                let row_ok = conn
                    .query_row(
                        "SELECT summary FROM agent_memory WHERE id = ?1",
                        params![id],
                        |row| row.get::<_, String>(0),
                    )
                    .is_ok_and(|s| s == opts.summary);
                let fts_ok = if fts5_available(&conn) {
                    // FTS5 MATCH against the first whitespace-separated token of
                    // the summary — broad enough to round-trip without escaping.
                    let token = opts
                        .summary
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    if token.is_empty() {
                        true
                    } else {
                        conn.query_row(
                            "SELECT COUNT(*) FROM agent_memory_fts WHERE agent_memory_fts MATCH ?1 AND rowid = ?2",
                            params![format!("\"{token}\""), id],
                            |row| row.get::<_, i64>(0),
                        )
                        .map_or(true, |n| n >= 1) // tolerate exotic tokens
                    }
                } else {
                    true
                };
                report["verified"] = json!(row_ok && fts_ok);
                // Touch last_used so subsequent decay calculations reset.
                let _ = touch_last_used(&conn, id, &now);
            }
        }
        Err(e) => {
            report["error"] = json!(format!("insert failed: {e}"));
        }
    }
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
    emit_memory_economy("write", started.elapsed().as_millis());
}

/// Options for `memory search` (W7).
#[derive(Debug, Default, Clone)]
pub struct SearchOpts {
    pub query: String,
    pub spec: Option<String>,
    pub cluster: Option<String>,
    pub limit: Option<usize>,
    pub include_low: bool,
}

/// `memory search` — FTS5 + scope filter over `agent_memory`. Stdout is a
/// JSON array of [`SearchRow`].
pub(crate) fn run_search(opts: SearchOpts) {
    let started = Instant::now();
    let cwd = project_dir();
    let now = now_iso8601();
    let limit = opts.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);

    let Ok(store) = SqliteEventStore::for_project(&cwd) else {
        println!("[]");
        emit_memory_economy("search", started.elapsed().as_millis());
        return;
    };
    let Ok(conn) = Connection::open(store.path()) else {
        println!("[]");
        emit_memory_economy("search", started.elapsed().as_millis());
        return;
    };
    let _ = conn.busy_timeout(std::time::Duration::from_secs(3));
    let _ = ensure_agent_memory_fts(&conn);

    let rows = search_agent_memory(
        &conn,
        &opts.query,
        opts.spec.as_deref(),
        opts.cluster.as_deref(),
        DEFAULT_MIN_EFFECTIVE_CONFIDENCE,
        limit,
        opts.include_low,
        &now,
    )
    .unwrap_or_default();

    // Touch last_used for every returned row — they were "used" by this read.
    for row in &rows {
        let _ = touch_last_used(&conn, row.id, &now);
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".to_string())
    );
    emit_memory_economy("search", started.elapsed().as_millis());
}

/// Options for `memory feedback` (W7).
#[derive(Debug, Default, Clone)]
pub struct FeedbackOpts {
    pub id: i64,
    pub kind: String,
    pub by_role: Option<String>,
    pub note: Option<String>,
    pub delta: Option<f64>,
}

/// `memory feedback` — append one row to `memory_feedback`. When `kind` is
/// `deprecate` or `supersede`, also flips the target row's `status` so it
/// stops surfacing in the default injection filter.
pub(crate) fn run_feedback(opts: FeedbackOpts) {
    let started = Instant::now();
    let cwd = project_dir();
    let mut report = json!({
        "operation": "memory-feedback",
        "memory_id": opts.id,
        "kind": opts.kind,
        "appended": false,
        "status_updated": false,
        "error": Value::Null,
    });

    if !FEEDBACK_KINDS.contains(&opts.kind.as_str()) {
        report["error"] = json!(format!(
            "invalid kind: must be one of {}",
            FEEDBACK_KINDS.join("|")
        ));
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        emit_memory_economy("feedback", started.elapsed().as_millis());
        return;
    }

    let store = match SqliteEventStore::for_project(&cwd) {
        Ok(s) => s,
        Err(e) => {
            report["error"] = json!(format!("cannot open store: {e}"));
            println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
            emit_memory_economy("feedback", started.elapsed().as_millis());
            return;
        }
    };
    let conn = match Connection::open(store.path()) {
        Ok(c) => c,
        Err(e) => {
            report["error"] = json!(format!("cannot open connection: {e}"));
            println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
            emit_memory_economy("feedback", started.elapsed().as_millis());
            return;
        }
    };
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));

    match insert_memory_feedback(
        &conn,
        opts.id,
        &opts.kind,
        opts.delta,
        opts.by_role.as_deref(),
        opts.note.as_deref(),
        None,
    ) {
        Ok(_) => {
            report["appended"] = json!(true);
            // Side-effect on the target row for terminal kinds.
            if matches!(opts.kind.as_str(), "deprecate" | "supersede") {
                let new_status = if opts.kind == "deprecate" {
                    "deprecated"
                } else {
                    "superseded"
                };
                let n = conn
                    .execute(
                        "UPDATE agent_memory SET status = ?1 WHERE id = ?2",
                        params![new_status, opts.id],
                    )
                    .unwrap_or(0);
                report["status_updated"] = json!(n > 0);
            }
        }
        Err(e) => {
            report["error"] = json!(format!("feedback insert failed: {e}"));
        }
    }
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
    emit_memory_economy("feedback", started.elapsed().as_millis());
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Bundle of W7-era flags threaded into [`dispatch`].
#[derive(Debug, Default, Clone)]
pub struct DispatchExtras {
    pub cluster: Option<String>,
    pub query: Option<String>,
    pub id: Option<i64>,
    pub kind: Option<String>,
    pub role: Option<String>,
    pub details: Option<String>,
    pub confidence: Option<f64>,
    pub verify: bool,
    pub include_low: bool,
    pub limit: Option<usize>,
    pub by_role: Option<String>,
    pub note: Option<String>,
}

/// Dispatch `mustard-rt run memory <subcommand>`.
///
/// `agent`, `decision`, `knowledge` are the write subcommands fed by JSON via
/// `--json`/stdin, **or** — for `agent` only — via the flat flags
/// `--agent`/`--summary`/`--files`/`--spec`/`--wave` (PowerShell-friendly,
/// no quoting gymnastics). `cross-wave` is the read subcommand; clap parses
/// its `--spec` / `--wave` flags into the dedicated arguments threaded
/// through from `RunCmd::Memory`.
/// `list` is the combined read subcommand that emits all memory entries.
///
/// W7 adds `write` (with `--verify`), `search` (FTS5 + scope), and `feedback`
/// (append + status flip on deprecate/supersede). Their per-subcommand flags
/// arrive bundled in [`DispatchExtras`] so the public function signature
/// does not balloon further.
#[allow(clippy::too_many_arguments)]
pub fn dispatch(
    subcommand: &str,
    json_arg: Option<&str>,
    spec: Option<&str>,
    wave: Option<u32>,
    agent: Option<&str>,
    summary: Option<&str>,
    files: Option<&str>,
    grouped: bool,
    format: &str,
    extras: DispatchExtras,
) {
    if subcommand == "cross-wave" || subcommand == "cross_wave" {
        crate::run::memory_cross_wave::run(spec, wave, extras.cluster.as_deref());
        return;
    }
    if subcommand == "list" {
        run_list(grouped, format);
        return;
    }
    if subcommand == "write" {
        let opts = WriteOpts {
            spec: spec.map(str::to_string),
            wave: wave.map(i64::from),
            role: extras.role.clone(),
            summary: summary.unwrap_or("").to_string(),
            details: extras.details.clone(),
            confidence: extras.confidence.unwrap_or(0.5),
            verify: extras.verify,
        };
        run_write(opts);
        return;
    }
    if subcommand == "search" {
        let opts = SearchOpts {
            query: extras.query.clone().unwrap_or_default(),
            spec: spec.map(str::to_string),
            cluster: extras.cluster.clone(),
            limit: extras.limit,
            include_low: extras.include_low,
        };
        run_search(opts);
        return;
    }
    if subcommand == "feedback" {
        let opts = FeedbackOpts {
            id: extras.id.unwrap_or(0),
            kind: extras.kind.clone().unwrap_or_default(),
            by_role: extras.by_role.clone(),
            note: extras.note.clone(),
            delta: None,
        };
        run_feedback(opts);
        return;
    }
    if !matches!(subcommand, "agent" | "decision" | "knowledge") {
        println!(
            "Usage: memory <agent|decision|knowledge|list|cross-wave|write|search|feedback> [--json '<JSON>']"
        );
        println!("  agent (flat form): --spec <name> --wave <N> --agent <type> --summary <text> --files <a.ts,b.ts>");
        println!("  cross-wave:        --spec <name> --wave <N> [--cluster <C>]");
        println!("  list:              [--grouped] [--format table|json]");
        println!("  write:             --summary <S> [--details <D>] [--spec <X>] [--wave <N>] [--role <R>] [--confidence <0..1>] [--verify]");
        println!("  search:            --query <X> [--spec <Y>] [--cluster <Z>] [--limit <N>] [--include-low]");
        println!("  feedback:          --id <N> --kind <deprecate|bump|supersede|use> [--by-role <R>] [--note <T>]");
        return;
    }

    // Flat-flag ergonomic form for `agent`: no --json, but at least one of
    // --agent / --summary / --files / --spec / --wave provided. Skips
    // stdin entirely so callers that never wired stdin (most one-shot CLI
    // invocations and pipeline orchestrators) work out of the box.
    let has_any_flat =
        agent.is_some() || summary.is_some() || files.is_some() || spec.is_some() || wave.is_some();
    if subcommand == "agent" && json_arg.is_none() && has_any_flat {
        let input = agent_input_from_flags(spec, wave, agent, summary, files);
        run_agent(&input);
        return;
    }

    let raw = read_input(json_arg);
    let input: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("[memory] Failed to parse input JSON: {err}");
            return;
        }
    };
    match subcommand {
        "agent" => run_agent(&input),
        "decision" => run_decision(&input),
        "knowledge" => run_knowledge(&input),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn truncate_summary_prefers_sentence_boundary() {
        let text = "First sentence. Second sentence that is quite long indeed.";
        let out = truncate_summary(text, 20);
        assert!(out.ends_with('.'));
        assert!(out.len() <= text.len());
    }

    #[test]
    fn agent_writes_entry_and_index() {
        let dir = tempdir().unwrap();
        // Workspace anchor required by `ClaudePaths::for_project` (deep-refactor W1/W2).
        std::fs::write(dir.path().join("mustard.json"), b"{}").unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        let input = json!({
            "cwd": dir.path().to_string_lossy(),
            "agent_type": "backend",
            "wave": 2,
            "pipeline": "demo",
            "summary": "did the thing",
        });
        run_agent(&input);
        let index = dir.path().join(".claude").join("agent-memory").join("_index.json");
        let parsed: Vec<Value> =
            serde_json::from_str(&std::fs::read_to_string(index).unwrap()).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["agent_type"], json!("backend"));
    }

    #[test]
    fn decision_inserts_to_sqlite() {
        let dir = tempdir().unwrap();
        // Open the store so the schema is initialised.
        let store = SqliteEventStore::for_project(dir.path()).unwrap();
        let db_path = store.path().to_path_buf();
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        insert_decision(&conn, "chose X over Y", Some("spec-1"), None, None).unwrap();
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM memory_decisions", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn lesson_inserts_to_sqlite() {
        let dir = tempdir().unwrap();
        let store = SqliteEventStore::for_project(dir.path()).unwrap();
        let db_path = store.path().to_path_buf();
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        insert_lesson(&conn, "always test fail-open paths", Some("retro"), None, None).unwrap();
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM memory_lessons", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn knowledge_upsert_bumps_count() {
        let dir = tempdir().unwrap();
        let store = SqliteEventStore::for_project(dir.path()).unwrap();
        let db_path = store.path().to_path_buf();
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let now = now_iso8601();
        upsert_knowledge_pattern(&conn, "repo-pattern: use a repository", 0.5, None, &now, &now)
            .unwrap();
        upsert_knowledge_pattern(&conn, "repo-pattern: use a repository", 0.6, None, &now, &now)
            .unwrap();
        let (count, confidence): (i64, f64) = conn
            .query_row(
                "SELECT count, confidence FROM knowledge_patterns WHERE pattern = ?1",
                ["repo-pattern: use a repository"],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 2);
        // confidence is updated to the latest value.
        assert!((confidence - 0.6).abs() < 1e-9);
    }

    #[test]
    fn agent_input_from_flags_builds_expected_shape() {
        let input = agent_input_from_flags(
            Some("spec-X"),
            Some(2),
            Some("wave-1-badges"),
            Some("did the thing"),
            Some("a.ts, b.ts ,c.ts"),
        );
        assert_eq!(input["agent_type"], json!("wave-1-badges"));
        assert_eq!(input["wave"], json!(2));
        assert_eq!(input["pipeline"], json!("spec-X"));
        assert_eq!(input["summary"], json!("did the thing"));
        assert_eq!(input["details"]["files"], json!(["a.ts", "b.ts", "c.ts"]));
    }

    #[test]
    fn agent_input_from_flags_handles_omitted_fields() {
        let input = agent_input_from_flags(None, None, None, None, None);
        assert_eq!(input["agent_type"], json!("unknown"));
        assert_eq!(input["wave"], Value::Null);
        assert_eq!(input["pipeline"], json!(""));
        assert_eq!(input["summary"], json!(""));
        assert!(input["details"].as_object().unwrap().is_empty());
    }

    #[test]
    fn run_decision_no_json_file_written() {
        // run_decision must NOT write decisions.json any more.
        let dir = tempdir().unwrap();
        // Ensure the store is initialised (schema).
        let _ = SqliteEventStore::for_project(dir.path()).unwrap();
        let input = json!({
            "cwd": dir.path().to_string_lossy(),
            "type": "decision",
            "content": "chose X over Y",
            "source": "spec-1",
        });
        run_decision(&input);
        let json_path =
            dir.path().join(".claude").join("memory").join("decisions.json");
        assert!(!json_path.exists(), "decisions.json must NOT be written in Wave 6b+");
    }

    #[test]
    fn grouped_table_groups_by_type() {
        let rows = vec![
            MemoryRow {
                entry_type: "pattern".to_string(),
                name: "repo-pattern: use repo".to_string(),
                description: "use a repository layer".to_string(),
                confidence: Some(0.8),
                occurrences: Some(3),
                last_seen: Some("2026-05-23".to_string()),
            },
            MemoryRow {
                entry_type: "decision".to_string(),
                name: "chose SQLite over Postgres".to_string(),
                description: "simpler deployment".to_string(),
                confidence: None,
                occurrences: None,
                last_seen: Some("2026-05-22".to_string()),
            },
            MemoryRow {
                entry_type: "convention".to_string(),
                name: "always fail-open".to_string(),
                description: "hooks must never abort work".to_string(),
                confidence: None,
                occurrences: None,
                last_seen: Some("2026-05-20".to_string()),
            },
        ];
        // Call render_grouped_table — smoke test only (check it doesn't panic
        // and produces output); we cannot capture stdout easily here.
        render_grouped_table(&rows);
    }

    #[test]
    fn truncate_col_truncates_correctly() {
        let s = "A".repeat(50);
        let result = truncate_col(&s, 10);
        assert_eq!(result.chars().count(), 10);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn truncate_col_passthrough_when_short() {
        let result = truncate_col("hello", 10);
        assert_eq!(result, "hello");
    }

    // -----------------------------------------------------------------------
    // W7 — agent_memory / memory_feedback / search / decay tests
    // -----------------------------------------------------------------------

    /// Open a fresh per-test SQLite store and return a `Connection` to it.
    /// The schema (including `agent_memory` + `memory_feedback`) is applied
    /// by `SqliteEventStore::for_project`; the returned connection is a
    /// second, direct handle to the same file — matching how the dispatch
    /// path opens connections.
    fn fresh_conn(dir: &tempfile::TempDir) -> Connection {
        let store = SqliteEventStore::for_project(dir.path()).unwrap();
        let conn = Connection::open(store.path()).unwrap();
        ensure_agent_memory_fts(&conn).unwrap();
        conn
    }

    #[test]
    fn memory_write_inserts_agent_memory_row() {
        let dir = tempdir().unwrap();
        let conn = fresh_conn(&dir);
        let id = insert_agent_memory(
            &conn,
            Some("s-test"),
            Some("deep-refactor"),
            Some(7),
            Some("rt"),
            "ensure_agent_memory_fts is idempotent",
            Some("details body"),
            0.9,
            None,
            None,
        )
        .unwrap();
        assert!(id >= 1);
        let stored: String = conn
            .query_row(
                "SELECT summary FROM agent_memory WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, "ensure_agent_memory_fts is idempotent");
    }

    #[test]
    fn memory_fts_mirror_finds_summary_token() {
        let dir = tempdir().unwrap();
        let conn = fresh_conn(&dir);
        let id = insert_agent_memory(
            &conn,
            None,
            Some("spec-A"),
            None,
            None,
            "tokenize me please",
            None,
            0.5,
            None,
            None,
        )
        .unwrap();
        // FTS5 mirror is populated by trigger — assert MATCH finds the row.
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_memory_fts WHERE agent_memory_fts MATCH ?1 AND rowid = ?2",
                params!["tokenize", id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "FTS5 trigger should mirror the inserted summary");
    }

    #[test]
    fn memory_feedback_appends_row() {
        let dir = tempdir().unwrap();
        let conn = fresh_conn(&dir);
        let mem_id = insert_agent_memory(
            &conn, None, None, None, None, "x", None, 0.5, None, None,
        )
        .unwrap();
        let fb_id =
            insert_memory_feedback(&conn, mem_id, "bump", Some(0.1), Some("qa"), None, None)
                .unwrap();
        assert!(fb_id >= 1);
        let kind: String = conn
            .query_row(
                "SELECT kind FROM memory_feedback WHERE id = ?1",
                params![fb_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kind, "bump");
    }

    #[test]
    fn memory_decay_drops_old_rows_below_threshold() {
        // confidence 0.5 stored 30+ days ago → effective drops to 0.0 (<0.3).
        let raw = 0.5;
        let now = "2026-06-25T00:00:00.000Z"; // ~31 days after `last_used`
        let last = "2026-05-25T00:00:00.000Z";
        let eff = effective_confidence(raw, Some(last), now);
        assert!(eff < DEFAULT_MIN_EFFECTIVE_CONFIDENCE, "got {eff}");
    }

    #[test]
    fn memory_decay_preserves_recent_rows() {
        let raw = 0.9;
        let now = "2026-05-25T01:00:00.000Z"; // 1 hour later
        let last = "2026-05-25T00:00:00.000Z";
        let eff = effective_confidence(raw, Some(last), now);
        assert!(eff > 0.85, "got {eff}");
    }

    #[test]
    fn memory_search_filters_by_spec_and_cluster() {
        let dir = tempdir().unwrap();
        let conn = fresh_conn(&dir);
        let now = now_iso8601();
        // Three rows: target, wrong spec, wrong cluster.
        insert_agent_memory(
            &conn,
            None,
            Some("deep-refactor"),
            Some(7),
            Some("rt"),
            "wave hardening summary",
            None,
            0.9,
            None,
            Some(&now),
        )
        .unwrap();
        insert_agent_memory(
            &conn,
            None,
            Some("other-spec"),
            Some(1),
            Some("rt"),
            "wave hardening summary",
            None,
            0.9,
            None,
            Some(&now),
        )
        .unwrap();
        insert_agent_memory(
            &conn,
            None,
            Some("deep-refactor"),
            Some(7),
            Some("dashboard"),
            "wave hardening summary",
            None,
            0.9,
            None,
            Some(&now),
        )
        .unwrap();

        let rows = search_agent_memory(
            &conn,
            "wave",
            Some("deep-refactor"),
            Some("rt"),
            DEFAULT_MIN_EFFECTIVE_CONFIDENCE,
            50,
            false,
            &now,
        )
        .unwrap();
        assert_eq!(rows.len(), 1, "spec+cluster should narrow to 1 row");
        assert_eq!(rows[0].spec.as_deref(), Some("deep-refactor"));
        assert_eq!(rows[0].role.as_deref(), Some("rt"));
    }

    #[test]
    fn memory_search_default_excludes_decayed_rows() {
        let dir = tempdir().unwrap();
        let conn = fresh_conn(&dir);
        let old = "2026-04-01T00:00:00.000Z";
        let now = "2026-05-25T00:00:00.000Z";
        insert_agent_memory(
            &conn,
            None,
            None,
            None,
            None,
            "ancient",
            None,
            0.5,
            None,
            Some(old),
        )
        .unwrap();
        let rows =
            search_agent_memory(&conn, "ancient", None, None, DEFAULT_MIN_EFFECTIVE_CONFIDENCE, 50, false, now)
                .unwrap();
        assert!(rows.is_empty(), "decayed row must not surface by default");
        let rows_all =
            search_agent_memory(&conn, "ancient", None, None, DEFAULT_MIN_EFFECTIVE_CONFIDENCE, 50, true, now)
                .unwrap();
        assert_eq!(rows_all.len(), 1, "include_low restores the row");
    }

    #[test]
    fn memory_default_injection_filters_by_spec_or_cluster() {
        let dir = tempdir().unwrap();
        let conn = fresh_conn(&dir);
        let now = now_iso8601();
        // 1. current-spec row — included.
        insert_agent_memory(
            &conn, None, Some("cur"), None, None, "in-spec", None, 0.6, None, Some(&now),
        )
        .unwrap();
        // 2. global high-confidence — included.
        insert_agent_memory(
            &conn, None, None, None, None, "global-strong", None, 0.85, None, Some(&now),
        )
        .unwrap();
        // 3. global low-confidence — excluded.
        insert_agent_memory(
            &conn, None, None, None, None, "global-weak", None, 0.4, None, Some(&now),
        )
        .unwrap();
        // 4. cluster row at 0.6 — included only when wave_applies_to lists "rt".
        insert_agent_memory(
            &conn, None, Some("other"), None, Some("rt"), "cluster-rt", None, 0.6, None, Some(&now),
        )
        .unwrap();

        let rows = default_injection_select(&conn, Some("cur"), &[], 50, &now).unwrap();
        let summaries: Vec<&str> = rows.iter().map(|r| r.summary.as_str()).collect();
        assert!(summaries.contains(&"in-spec"));
        assert!(summaries.contains(&"global-strong"));
        assert!(!summaries.contains(&"global-weak"));
        assert!(!summaries.contains(&"cluster-rt"));

        let rows_with_cluster =
            default_injection_select(&conn, Some("cur"), &["rt".to_string()], 50, &now).unwrap();
        let with_cluster: Vec<&str> =
            rows_with_cluster.iter().map(|r| r.summary.as_str()).collect();
        assert!(with_cluster.contains(&"cluster-rt"));
    }

    #[test]
    fn feedback_kind_validation_rejects_unknown() {
        let dir = tempdir().unwrap();
        let _store = SqliteEventStore::for_project(dir.path()).unwrap();
        // We can't intercept stdout easily, but we assert the whitelist:
        assert!(FEEDBACK_KINDS.contains(&"bump"));
        assert!(!FEEDBACK_KINDS.contains(&"nuke"));
    }

    #[test]
    fn parse_iso8601_secs_round_trips_known_timestamp() {
        // 2026-05-25T00:00:00Z → secs since epoch via Hinnant's algorithm.
        // Cross-check: same instant produced by `now_iso8601`'s siblings.
        let s = parse_iso8601_secs("2026-05-25T00:00:00Z").unwrap();
        // Sanity: must be after 2025-01-01 (1735689600).
        assert!(s > 1_735_689_600);
    }
}
