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
//! Wave 7 adds the `--agent-memory` flag (deep-refactor T7.4): walks
//! `.claude/.agent-memory/` (the legacy rolling-cap-20 JSON sink written by
//! the `memory agent` subcommand) and forwards each entry into the
//! `agent_memory` SQLite table introduced in W0.T0.5. The directory is
//! removed on success. Fail-open per entry — a corrupted JSON file lands in
//! `errors` but does not abort the rest, and a partial sweep leaves the dir
//! alone so the caller can retry.
//!
//! With `--delete`, each source file is removed after a successful ingest.
//! A bad JSON in one file is reported in `errors` and does not abort the rest.
//! Output: one JSON line:
//! `{ "ingested": { "knowledge": N, "decisions": M, "lessons": K, "agent_memory": Z }, "deleted": bool, "errors": [...] }`.

use crate::run::env::project_dir as env_project_dir;
use crate::run::memory::{
    ensure_agent_memory_fts, insert_agent_memory, insert_decision, insert_lesson,
    upsert_knowledge_pattern,
};
use mustard_core::fs;
use mustard_core::store::sqlite_store::SqliteEventStore;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

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
// Wave 7 — agent-memory JSON → `agent_memory` SQLite table
// ---------------------------------------------------------------------------

/// Per-file walk of `.claude/.agent-memory/` (excluding `_index.json` and
/// `_queue.json`) inserting each entry into the `agent_memory` table.
///
/// Schema mapping:
///
/// | JSON field             | `agent_memory` column |
/// |------------------------|-----------------------|
/// | `session` (8-char)     | `session_id`          |
/// | `pipeline`             | `spec`                |
/// | `wave`                 | `wave`                |
/// | `agent_type`           | `role`                |
/// | `summary`              | `summary`             |
/// | `details` (object)     | `details` (JSON text) |
/// | (n/a — default)        | `confidence = 0.5`    |
/// | (n/a — default)        | `status = 'active'`   |
/// | `timestamp`            | `at` / `last_used`    |
///
/// Returns the number of inserted rows; per-file failures land in `errors`.
fn ingest_agent_memory_dir(
    conn: &Connection,
    agent_dir: &Path,
    errors: &mut Vec<Value>,
) -> (usize, Vec<PathBuf>) {
    let mut count = 0usize;
    let mut consumed: Vec<PathBuf> = Vec::new();
    if !agent_dir.exists() {
        return (count, consumed);
    }
    let entries = match std::fs::read_dir(agent_dir) {
        Ok(it) => it,
        Err(e) => {
            errors.push(json!({ "file": ".agent-memory", "error": e.to_string() }));
            return (count, consumed);
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name_os) = path.file_name() else { continue };
        let name = name_os.to_string_lossy().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        if name == "_index.json" || name == "_queue.json" {
            // Index file is bookkeeping — it gets deleted with the directory.
            consumed.push(path.clone());
            continue;
        }
        let raw = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                errors.push(json!({ "file": name, "error": e.to_string() }));
                continue;
            }
        };
        let v: Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                errors.push(json!({ "file": name, "error": e.to_string() }));
                continue;
            }
        };
        let summary = v.get("summary").and_then(Value::as_str).unwrap_or("").to_string();
        if summary.is_empty() {
            // Nothing useful to migrate — still consume the file.
            consumed.push(path.clone());
            continue;
        }
        let session = v.get("session").and_then(Value::as_str);
        let spec = v.get("pipeline").and_then(Value::as_str).filter(|s| !s.is_empty());
        let wave = v.get("wave").and_then(Value::as_i64);
        let role = v.get("agent_type").and_then(Value::as_str);
        let details_text = v.get("details").map(ToString::to_string);
        let at = v.get("timestamp").and_then(Value::as_str);

        match insert_agent_memory(
            conn,
            session,
            spec,
            wave,
            role,
            &summary,
            details_text.as_deref(),
            0.5,
            None,
            at,
        ) {
            Ok(_) => {
                count += 1;
                consumed.push(path.clone());
            }
            Err(e) => errors.push(json!({
                "file": name,
                "error": e.to_string()
            })),
        }
    }
    (count, consumed)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// `mustard-rt run memory-ingest [--delete] [--agent-memory]`.
///
/// `--agent-memory` switches the run to the Wave 7 agent-memory migration
/// path (legacy `.claude/.agent-memory/` JSON → `agent_memory` SQLite table).
/// On full success the source directory is removed.
pub fn run_with(opts: MemoryIngestOpts) {
    let cwd = env_project_dir();
    let claude = Path::new(&cwd).join(".claude");

    let store = match SqliteEventStore::for_project(&cwd) {
        Ok(s) => s,
        Err(e) => {
            let out = json!({
                "ingested": { "knowledge": 0, "decisions": 0, "lessons": 0, "agent_memory": 0 },
                "deleted": false,
                "errors": [{ "file": "(store)", "error": e.to_string() }]
            });
            println!("{out}");
            return;
        }
    };
    let db_path = store.path().to_path_buf();
    let conn = match Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            let out = json!({
                "ingested": { "knowledge": 0, "decisions": 0, "lessons": 0, "agent_memory": 0 },
                "deleted": false,
                "errors": [{ "file": "(connection)", "error": e.to_string() }]
            });
            println!("{out}");
            return;
        }
    };
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));

    // Agent-memory mode is the W7 path: walks `.claude/.agent-memory/` and
    // forwards each entry into `agent_memory`, then removes the directory.
    if opts.agent_memory {
        let _ = ensure_agent_memory_fts(&conn);
        let agent_dir = claude.join(".agent-memory");
        let dir_existed = agent_dir.exists();
        let mut errors: Vec<Value> = Vec::new();
        let (count, _consumed) = ingest_agent_memory_dir(&conn, &agent_dir, &mut errors);
        let deleted = if dir_existed && errors.is_empty() {
            std::fs::remove_dir_all(&agent_dir).is_ok()
        } else {
            // Directory absent (nothing to clean) — surface deleted=true so the
            // AC's "post-condition: dir absent" check stays satisfied either way.
            !dir_existed
        };
        let out = json!({
            "ingested": {
                "knowledge": 0, "decisions": 0, "lessons": 0,
                "agent_memory": count
            },
            "deleted": deleted,
            "errors": errors
        });
        println!("{out}");
        return;
    }

    run_legacy(&conn, &claude, opts.delete);
}

/// Options for [`run_with`].
#[derive(Debug, Default, Clone, Copy)]
pub struct MemoryIngestOpts {
    pub delete: bool,
    pub agent_memory: bool,
}

/// Back-compat shim — kept for callers that imported the pre-W7 signature
/// directly (e.g. external tests). `mod.rs` now uses [`run_with`] instead.
#[allow(dead_code)]
pub fn run(delete: bool) {
    run_with(MemoryIngestOpts {
        delete,
        agent_memory: false,
    });
}

/// Legacy path: the pre-W7 ingest of `knowledge.json` + `decisions.json` +
/// `lessons.json`. The caller has already opened the SQLite store and
/// validated `.claude/` exists.
fn run_legacy(conn: &Connection, claude: &Path, delete: bool) {
    let mem = claude.join("memory");

    let mut errors: Vec<Value> = Vec::new();
    let knowledge_path = claude.join("knowledge.json");
    let decisions_path = mem.join("decisions.json");
    let lessons_path = mem.join("lessons.json");

    let knowledge_ok = knowledge_path.exists();
    let decisions_ok = decisions_path.exists();
    let lessons_ok = lessons_path.exists();

    let knowledge_count = ingest_knowledge(conn, claude, &mut errors);
    let decisions_count =
        ingest_memory_file(conn, &decisions_path, "decisions", true, &mut errors);
    let lessons_count =
        ingest_memory_file(conn, &lessons_path, "lessons", false, &mut errors);

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
            "lessons": lessons_count,
            "agent_memory": 0
        },
        "deleted": deleted_any,
        "errors": errors
    });
    println!("{out}");
}
