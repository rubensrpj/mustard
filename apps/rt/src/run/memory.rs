//! `mustard-rt run memory` — a port of `scripts/memory.js`.
//!
//! A unified persistence CLI with three subcommands:
//!
//! - `agent`     → `.claude/.agent-memory/`                    (rolling cap 20)
//! - `decision`  → `memory_decisions` / `memory_lessons` SQLite (cap 50)
//! - `knowledge` → `knowledge_patterns` SQLite                 (cap 200 / 80 per type)
//!
//! Input JSON arrives either via `--json '<JSON>'` (the Windows-friendly form)
//! or piped on stdin (the POSIX fallback). Exit is always `0` (fail-open).
//!
//! Wave 6b: `decision` and `knowledge` subcommands write to the Wave 6a SQLite
//! tables (`memory_decisions`, `memory_lessons`, `knowledge_patterns`).
//! Legacy JSON sidecars are no longer written.  Wave 6c migrates the
//! dashboard reader.

use crate::run::env::{current_spec, project_dir, session_id};
use crate::util::{now_iso8601, now_millis};
use mustard_core::fs;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use rusqlite::params;
use serde_json::{Value, json};
use std::io::Read;
use std::path::Path;

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
    let state_dir = project_dir.join(".claude").join(".agent-state");
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
    let mem_dir = project_dir.join(".claude").join(".agent-memory");
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
// Dispatch
// ---------------------------------------------------------------------------

/// Dispatch `mustard-rt run memory <subcommand>`.
///
/// `agent`, `decision`, `knowledge` are the write subcommands fed by JSON via
/// `--json`/stdin, **or** — for `agent` only — via the flat flags
/// `--agent`/`--summary`/`--files`/`--spec`/`--wave` (PowerShell-friendly,
/// no quoting gymnastics). `cross-wave` is the read subcommand; clap parses
/// its `--spec` / `--wave` flags into the dedicated arguments threaded
/// through from `RunCmd::Memory`.
/// `list` is the combined read subcommand that emits all memory entries.
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
) {
    if subcommand == "cross-wave" || subcommand == "cross_wave" {
        crate::run::memory_cross_wave::run(spec, wave);
        return;
    }
    if subcommand == "list" {
        run_list(grouped, format);
        return;
    }
    if !matches!(subcommand, "agent" | "decision" | "knowledge") {
        println!(
            "Usage: memory <agent|decision|knowledge|list|cross-wave> [--json '<JSON>']"
        );
        println!("  agent (flat form): --spec <name> --wave <N> --agent <type> --summary <text> --files <a.ts,b.ts>");
        println!("  cross-wave:        --spec <name> --wave <N>");
        println!("  list:              [--grouped] [--format table|json]");
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
        let input = json!({
            "cwd": dir.path().to_string_lossy(),
            "agent_type": "backend",
            "wave": 2,
            "pipeline": "demo",
            "summary": "did the thing",
        });
        run_agent(&input);
        let index = dir.path().join(".claude").join(".agent-memory").join("_index.json");
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
}
