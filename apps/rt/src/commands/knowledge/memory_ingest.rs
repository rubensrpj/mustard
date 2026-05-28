//! `mustard-rt run memory-ingest` — one-shot legacy JSON → markdown migration.
//!
//! W4B: SQLite sinks removed. Reads the three legacy JSON files (when they
//! exist) and the legacy `.claude/.agent-memory/` rolling JSON dir, and emits
//! one markdown file per entry under `.claude/{memory,knowledge}/` via
//! [`mustard_core::io::atomic_md::MarkdownStore`].
//!
//! | Source JSON                     | Destination dir                          |
//! |---------------------------------|------------------------------------------|
//! | `.claude/knowledge.json`        | `.claude/knowledge/`                     |
//! | `.claude/memory/decisions.json` | `.claude/memory/decisions/`              |
//! | `.claude/memory/lessons.json`   | `.claude/memory/lessons/`                |
//! | `.claude/.agent-memory/*.json`  | `.claude/memory/agent/`                  |
//!
//! With `--delete`, each source file is removed after a successful ingest.
//! A bad JSON in one file is reported in `errors` and does not abort the rest.
//! Output: one JSON line:
//! `{ "ingested": { "knowledge": N, "decisions": M, "lessons": K, "agent_memory": Z }, "deleted": bool, "errors": [...] }`.

use crate::shared::context::project_dir as env_project_dir;
use crate::util::slug::slug_for;
use mustard_core::io::atomic_md::frontmatter::Frontmatter;
use mustard_core::io::atomic_md::{MarkdownDoc, MarkdownStore};
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Doc helpers (the slug helper now lives in `crate::util::slug`)
// ---------------------------------------------------------------------------

fn write_md(dir: &Path, slug: &str, fm: Map<String, Value>, body: String) -> std::io::Result<()> {
    fs::create_dir_all(dir).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    let dest = dir.join(format!("{slug}.md"));
    let doc = MarkdownDoc {
        path: dest.clone(),
        frontmatter: Some(Frontmatter(Value::Object(fm))),
        body,
    };
    MarkdownStore::write_atomic(&dest, &doc)
}

// ---------------------------------------------------------------------------
// Per-file ingest helpers
// ---------------------------------------------------------------------------

fn ingest_knowledge(claude_dir: &Path, errors: &mut Vec<Value>) -> usize {
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
    let dest_dir = claude_dir.join("knowledge");

    let mut count = 0usize;
    for entry in &entries {
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let description = entry
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() || description.is_empty() {
            continue;
        }
        let confidence = entry
            .get("confidence")
            .and_then(Value::as_f64)
            .filter(|&c| (0.0..=1.0).contains(&c))
            .unwrap_or(0.3);
        let source = entry.get("source").and_then(Value::as_str).map(str::to_string);
        let captured_at = entry
            .get("lastSeen")
            .or_else(|| entry.get("updatedAt"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(mustard_core::time::now_iso8601);

        let mut fm = Map::new();
        fm.insert("kind".into(), json!("pattern"));
        fm.insert("name".into(), json!(name));
        fm.insert("captured_at".into(), json!(captured_at));
        fm.insert("confidence".into(), json!(confidence));
        if let Some(s) = source {
            fm.insert("source".into(), json!(s));
        }
        fm.insert("status".into(), json!("active"));
        let pattern = format!("{name}: {description}");
        let slug = slug_for(&captured_at, &pattern);
        match write_md(&dest_dir, &slug, fm, format!("{description}\n")) {
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

fn ingest_memory_file(
    file_path: &Path,
    dest_dir: &Path,
    is_decision: bool,
    errors: &mut Vec<Value>,
) -> usize {
    if !file_path.exists() {
        return 0;
    }
    let file_name = file_path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().to_string());
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

    let kind = if is_decision { "decision" } else { "lesson" };
    let mut count = 0usize;
    for entry in &entries {
        let content = entry
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if content.is_empty() {
            continue;
        }
        let source = entry.get("source").and_then(Value::as_str).map(str::to_string);
        let context = entry.get("context").and_then(Value::as_str).map(str::to_string);
        let captured_at = entry
            .get("at")
            .or_else(|| entry.get("timestamp"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(mustard_core::time::now_iso8601);

        let mut fm = Map::new();
        fm.insert("kind".into(), json!(kind));
        fm.insert("captured_at".into(), json!(captured_at));
        if let Some(s) = source {
            fm.insert("source".into(), json!(s));
        }
        if let Some(c) = context {
            fm.insert("context".into(), json!(c));
        }
        fm.insert("status".into(), json!("active"));

        let slug = slug_for(&captured_at, &content);
        match write_md(dest_dir, &slug, fm, format!("{content}\n")) {
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

fn ingest_agent_memory_dir(agent_dir: &Path, dest_dir: &Path, errors: &mut Vec<Value>) -> (usize, Vec<PathBuf>) {
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
            consumed.push(path.clone());
            continue;
        }
        let captured_at = v
            .get("timestamp")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(mustard_core::time::now_iso8601);
        let mut fm = Map::new();
        if let Some(s) = v.get("session").and_then(Value::as_str) {
            fm.insert("session_id".into(), json!(s));
        }
        if let Some(s) = v.get("pipeline").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            fm.insert("spec".into(), json!(s));
        }
        if let Some(w) = v.get("wave").and_then(Value::as_i64) {
            fm.insert("wave".into(), json!(w));
        }
        if let Some(r) = v.get("agent_type").and_then(Value::as_str) {
            fm.insert("role".into(), json!(r));
        }
        fm.insert("summary".into(), json!(summary));
        fm.insert("confidence".into(), json!(0.5));
        fm.insert("status".into(), json!("active"));
        fm.insert("at".into(), json!(captured_at.clone()));
        fm.insert("last_used".into(), json!(captured_at.clone()));
        let body = v
            .get("details")
            .map(|d| d.to_string())
            .unwrap_or_default();

        let slug = slug_for(&captured_at, &summary);
        match write_md(dest_dir, &slug, fm, body) {
            Ok(()) => {
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

#[derive(Debug, Default, Clone, Copy)]
pub struct MemoryIngestOpts {
    pub delete: bool,
    pub agent_memory: bool,
}

pub fn run_with(opts: MemoryIngestOpts) {
    let cwd = env_project_dir();
    let claude = ClaudePaths::for_project(Path::new(&cwd))
        .map(|p| p.claude_dir())
        .unwrap_or_else(|_| ClaudePaths::compose_unchecked(Path::new(&cwd)).claude_dir());

    if opts.agent_memory {
        let agent_src = claude.join(".agent-memory");
        let dest = claude.join("memory").join("agent");
        let dir_existed = agent_src.exists();
        let mut errors: Vec<Value> = Vec::new();
        let (count, _consumed) = ingest_agent_memory_dir(&agent_src, &dest, &mut errors);
        let deleted = if dir_existed && errors.is_empty() {
            std::fs::remove_dir_all(&agent_src).is_ok()
        } else {
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

    run_legacy(&claude, opts.delete);
}

#[allow(dead_code)]
pub fn run(delete: bool) {
    run_with(MemoryIngestOpts {
        delete,
        agent_memory: false,
    });
}

fn run_legacy(claude: &Path, delete: bool) {
    let mem = claude.join("memory");
    let mut errors: Vec<Value> = Vec::new();
    let knowledge_path = claude.join("knowledge.json");
    let decisions_path = mem.join("decisions.json");
    let lessons_path = mem.join("lessons.json");
    let decisions_dest = mem.join("decisions");
    let lessons_dest = mem.join("lessons");

    let knowledge_ok = knowledge_path.exists();
    let decisions_ok = decisions_path.exists();
    let lessons_ok = lessons_path.exists();

    let knowledge_count = ingest_knowledge(claude, &mut errors);
    let decisions_count = ingest_memory_file(&decisions_path, &decisions_dest, true, &mut errors);
    let lessons_count = ingest_memory_file(&lessons_path, &lessons_dest, false, &mut errors);

    let knowledge_had_error =
        errors.iter().any(|e| e.get("file").and_then(Value::as_str) == Some("knowledge.json"));
    let decisions_had_error =
        errors.iter().any(|e| e.get("file").and_then(Value::as_str) == Some("decisions.json"));
    let lessons_had_error =
        errors.iter().any(|e| e.get("file").and_then(Value::as_str) == Some("lessons.json"));

    let mut deleted_any = false;
    if delete {
        if knowledge_ok && !knowledge_had_error && fs::remove_file(&knowledge_path).is_ok() {
            deleted_any = true;
        }
        if decisions_ok && !decisions_had_error && fs::remove_file(&decisions_path).is_ok() {
            deleted_any = true;
        }
        if lessons_ok && !lessons_had_error && fs::remove_file(&lessons_path).is_ok() {
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
