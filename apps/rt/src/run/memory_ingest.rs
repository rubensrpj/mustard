//! `mustard-rt run memory-ingest` — one-shot JSON → SQLite migration.
//!
//! Reads the three legacy JSON files (if they exist) and inserts their entries
//! into the Wave 6a SQLite tables:
//!
//! | Source JSON | Target table |
//! |-------------|--------------|
//! | `.claude/knowledge.json` | `knowledge_patterns` |
//! | `.claude/memory/decisions.json` | `memory_decisions` |
//! | `.claude/memory/lessons.json` | `memory_lessons` |
//!
//! With `--delete`, each source file is removed after a successful ingest.
//! A bad JSON in one file is reported in `errors` and does not abort the rest.
//! Output: one JSON line:
//! `{ "ingested": { "knowledge": N, "decisions": M, "lessons": K }, "deleted": bool, "errors": [...] }`.

use crate::run::env::project_dir as env_project_dir;
use crate::run::memory::{insert_decision, insert_lesson, upsert_knowledge_pattern};
use mustard_core::fs;
use mustard_core::store::sqlite_store::SqliteEventStore;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::path::Path;

// ---------------------------------------------------------------------------
// Per-file ingest helpers
// ---------------------------------------------------------------------------

/// Ingest `.claude/knowledge.json` → `knowledge_patterns`.
///
/// Each entry is expected to have at least a `name` string. The stored pattern
/// is `"{name}: {description}"` mirroring how `run_knowledge` formats it.
/// Fields preserved from JSON when present: `confidence`, `lastSeen` /
/// `updatedAt`, `createdAt`.
fn ingest_knowledge(conn: &Connection, claude_dir: &Path, errors: &mut Vec<Value>) -> usize {
    let path = claude_dir.join("knowledge.json");
    if !path.exists() {
        return 0;
    }
    let raw = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            errors.push(json!({ "file": "knowledge.json", "error": e.to_string() }));
            return 0;
        }
    };
    let kb: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            errors.push(json!({ "file": "knowledge.json", "error": e.to_string() }));
            return 0;
        }
    };
    let entries = match kb.get("entries").and_then(Value::as_array) {
        Some(a) => a.clone(),
        None => return 0,
    };

    let mut count = 0usize;
    for entry in &entries {
        let name = entry.get("name").and_then(Value::as_str).unwrap_or("").trim().to_string();
        let description =
            entry.get("description").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if name.is_empty() || description.is_empty() {
            continue;
        }
        let pattern = format!("{name}: {description}");
        let confidence = entry
            .get("confidence")
            .and_then(Value::as_f64)
            .filter(|&c| (0.0..=1.0).contains(&c))
            .unwrap_or(0.3);
        let source = entry.get("source").and_then(Value::as_str).map(str::to_string);
        let last_seen = entry
            .get("lastSeen")
            .or_else(|| entry.get("updatedAt"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let created_at = entry
            .get("createdAt")
            .and_then(Value::as_str)
            .unwrap_or(last_seen.as_str())
            .to_string();
        let ts = if last_seen.is_empty() { crate::util::now_iso8601() } else { last_seen };
        let cat = if created_at.is_empty() { ts.clone() } else { created_at };

        match upsert_knowledge_pattern(conn, &pattern, confidence, source.as_deref(), &ts, &cat) {
            Ok(()) => count += 1,
            Err(e) => errors.push(json!({
                "file": "knowledge.json",
                "entry": pattern,
                "error": e.to_string()
            })),
        }
    }
    count
}

/// Ingest a `memory/{decisions,lessons}.json` → `memory_decisions` /
/// `memory_lessons`. `table_label` is for error reporting only.
fn ingest_memory_file(
    conn: &Connection,
    file_path: &Path,
    table_label: &str,
    is_decision: bool,
    errors: &mut Vec<Value>,
) -> usize {
    if !file_path.exists() {
        return 0;
    }
    let file_name = file_path
        .file_name()
        .map_or_else(|| table_label.to_string(), |n| n.to_string_lossy().to_string());
    let raw = match fs::read_to_string(file_path) {
        Ok(t) => t,
        Err(e) => {
            errors.push(json!({ "file": file_name, "error": e.to_string() }));
            return 0;
        }
    };
    let data: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            errors.push(json!({ "file": file_name, "error": e.to_string() }));
            return 0;
        }
    };
    let entries = match data.get("entries").and_then(Value::as_array) {
        Some(a) => a.clone(),
        None => return 0,
    };

    let mut count = 0usize;
    for entry in &entries {
        let content = entry.get("content").and_then(Value::as_str).unwrap_or("").to_string();
        if content.is_empty() {
            continue;
        }
        let source = entry.get("source").and_then(Value::as_str).map(str::to_string);
        let context = entry.get("context").and_then(Value::as_str).map(str::to_string);
        let at = entry
            .get("at")
            .or_else(|| entry.get("timestamp"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let result = if is_decision {
            insert_decision(conn, &content, source.as_deref(), context.as_deref(), at.as_deref())
        } else {
            insert_lesson(conn, &content, source.as_deref(), context.as_deref(), at.as_deref())
        };
        match result {
            Ok(()) => count += 1,
            Err(e) => errors.push(json!({
                "file": file_name,
                "entry": content,
                "error": e.to_string()
            })),
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// `mustard-rt run memory-ingest [--delete]`.
pub fn run(delete: bool) {
    let cwd = env_project_dir();
    let claude = Path::new(&cwd).join(".claude");
    let mem = claude.join("memory");

    // Open the store (creates + applies schema if absent).
    let store = match SqliteEventStore::for_project(&cwd) {
        Ok(s) => s,
        Err(e) => {
            let out = json!({
                "ingested": { "knowledge": 0, "decisions": 0, "lessons": 0 },
                "deleted": false,
                "errors": [{ "file": "(store)", "error": e.to_string() }]
            });
            println!("{out}");
            return;
        }
    };

    // Open a direct Connection for INSERTs (SqliteEventStore's Connection is
    // private and only exposes EventSink; knowledge/memory tables need DML).
    let db_path = store.path().to_path_buf();
    let conn = match Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            let out = json!({
                "ingested": { "knowledge": 0, "decisions": 0, "lessons": 0 },
                "deleted": false,
                "errors": [{ "file": "(connection)", "error": e.to_string() }]
            });
            println!("{out}");
            return;
        }
    };
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));

    let mut errors: Vec<Value> = Vec::new();
    let knowledge_path = claude.join("knowledge.json");
    let decisions_path = mem.join("decisions.json");
    let lessons_path = mem.join("lessons.json");

    let knowledge_ok = knowledge_path.exists();
    let decisions_ok = decisions_path.exists();
    let lessons_ok = lessons_path.exists();

    let knowledge_count = ingest_knowledge(&conn, &claude, &mut errors);
    let decisions_count =
        ingest_memory_file(&conn, &decisions_path, "decisions", true, &mut errors);
    let lessons_count =
        ingest_memory_file(&conn, &lessons_path, "lessons", false, &mut errors);

    // Determine which files had errors (to skip deletion).
    let knowledge_had_error =
        errors.iter().any(|e| e.get("file").and_then(Value::as_str) == Some("knowledge.json"));
    let decisions_had_error =
        errors.iter().any(|e| e.get("file").and_then(Value::as_str) == Some("decisions.json"));
    let lessons_had_error =
        errors.iter().any(|e| e.get("file").and_then(Value::as_str) == Some("lessons.json"));

    let mut deleted_any = false;
    if delete {
        if knowledge_ok && !knowledge_had_error
            && fs::remove_file(&knowledge_path).is_ok() {
            deleted_any = true;
        }
        if decisions_ok && !decisions_had_error
            && fs::remove_file(&decisions_path).is_ok() {
            deleted_any = true;
        }
        if lessons_ok && !lessons_had_error
            && fs::remove_file(&lessons_path).is_ok() {
            deleted_any = true;
        }
    }

    let out = json!({
        "ingested": {
            "knowledge": knowledge_count,
            "decisions": decisions_count,
            "lessons": lessons_count
        },
        "deleted": deleted_any,
        "errors": errors
    });
    println!("{out}");
}
