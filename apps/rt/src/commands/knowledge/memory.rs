//! `mustard-rt run memory` — filesystem-backed memory CLI (W4B migration).
//!
//! A unified persistence CLI. All persistence is markdown-atomic via
//! [`mustard_core::atomic_md::MarkdownStore`] — no SQLite.
//!
//! Subcommands:
//!
//! - `agent`     → `.claude/.agent-memory/` (rolling cap 20, legacy JSON kept)
//! - `decision`  → `.claude/memory/decisions/{slug}.md`
//! - `knowledge` → `.claude/knowledge/{slug}.md`
//! - `write`     → `.claude/memory/agent/{slug}.md`
//! - `search`    → scan `.claude/memory/agent/` + LIKE match + scope filter
//! - `feedback`  → append to `.claude/memory/agent/{slug}.feedback.ndjson`
//!                 (deprecate/supersede flip `status` in source frontmatter)
//! - `list`      → scan memory + knowledge dirs, emit JSON/table
//! - `cross-wave`→ render markdown of prior-wave memories
//!
//! Input JSON arrives via `--json '<JSON>'` (Windows-friendly) or stdin
//! (POSIX fallback). Exit is always `0` (fail-open).

use crate::shared::context::{current_spec, project_dir, session_id};
use crate::util::{now_iso8601, now_millis};
use mustard_core::atomic_md::frontmatter::Frontmatter;
use mustard_core::atomic_md::{MarkdownDoc, MarkdownStore};
use mustard_core::fs;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::ClaudePaths;
use serde_json::{json, Map, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Minimum *effective* confidence (after lazy decay) for a memory row to
/// surface in default `search` results. Bypass with `--include-low`.
pub(crate) const DEFAULT_MIN_EFFECTIVE_CONFIDENCE: f64 = 0.3;

/// Days over which a memory's confidence linearly decays to zero from its
/// `last_used` timestamp.
pub(crate) const DECAY_WINDOW_DAYS: f64 = 30.0;

/// Default `search` result cap when `--limit` is omitted.
pub(crate) const DEFAULT_SEARCH_LIMIT: usize = 20;

/// `.agent-memory/` rolling cap.
const AGENT_CAP: usize = 20;

/// Whitelist of `memory feedback` kinds.
pub(crate) const FEEDBACK_KINDS: &[&str] = &["deprecate", "bump", "supersede", "use"];

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn read_input(json_arg: Option<&str>) -> String {
    if let Some(text) = json_arg {
        return text.to_string();
    }
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
    buf
}

fn input_cwd(input: &Value) -> String {
    input
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map_or_else(project_dir, str::to_string)
}

fn truncate_summary(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }
    let slice: String = text.chars().take(max_len).collect();
    let boundary = ['.', '!', '?'].iter().filter_map(|c| slice.rfind(*c)).max();
    match boundary {
        Some(b) => text.chars().take(b + 1).collect(),
        None => {
            let kept: String = text.chars().take(max_len.saturating_sub(3)).collect();
            format!("{kept}...")
        }
    }
}

fn resolve_session_prefix(project_dir: &Path) -> String {
    let Ok(cp) = ClaudePaths::for_project(project_dir) else {
        return std::process::id().to_string();
    };
    let state_dir = cp.agent_state_dir();
    if let Ok(entries) = fs::read_dir(&state_dir) {
        for entry in entries {
            let name = entry.file_name.clone();
            if !Path::new(&name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
                || name == "_queue.json"
            {
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

/// Compute a short FNV-1a hash of `s` (8 hex chars). Slug suffix only — not
/// security-relevant.
fn fnv1a8(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:016x}", h).chars().take(8).collect()
}

/// Slug shape: `{compact_ts}-{hash8}` — filename-safe, deterministic per
/// `(timestamp, content)` pair.
fn slug_for(captured_at: &str, content: &str) -> String {
    let ts_compact: String = captured_at
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    format!("{ts_compact}-{}", fnv1a8(content))
}

/// Build a `MarkdownDoc` with a JSON-object frontmatter and a UTF-8 body.
fn doc_with_frontmatter(path: PathBuf, fm: Map<String, Value>, body: String) -> MarkdownDoc {
    MarkdownDoc {
        path,
        frontmatter: Some(Frontmatter(Value::Object(fm))),
        body,
    }
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

fn memory_root(project: &Path) -> Option<PathBuf> {
    ClaudePaths::for_project(project)
        .ok()
        .map(|p| p.claude_dir().join("memory"))
}

fn knowledge_root(project: &Path) -> Option<PathBuf> {
    ClaudePaths::for_project(project)
        .ok()
        .map(|p| p.claude_dir().join("knowledge"))
}

fn agent_dir(project: &Path) -> Option<PathBuf> {
    memory_root(project).map(|p| p.join("agent"))
}

fn decisions_dir(project: &Path) -> Option<PathBuf> {
    memory_root(project).map(|p| p.join("decisions"))
}

fn lessons_dir(project: &Path) -> Option<PathBuf> {
    memory_root(project).map(|p| p.join("lessons"))
}

// ---------------------------------------------------------------------------
// `agent` — preserved legacy JSON sink under .claude/.agent-memory/
// ---------------------------------------------------------------------------

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

/// Emit a `decision` / `lesson` event into the per-spec NDJSON sink.
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
    let _ = crate::shared::events::route::emit(dir, &ev);
}

// ---------------------------------------------------------------------------
// `decision` / `knowledge` — markdown atomic writes
// ---------------------------------------------------------------------------

fn write_decision_or_lesson(
    project: &Path,
    entry_type: &str,
    content: &str,
    source: &str,
    context: &str,
) -> bool {
    let dir = if entry_type == "decision" {
        decisions_dir(project)
    } else {
        lessons_dir(project)
    };
    let Some(dir) = dir else {
        return false;
    };
    if fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let captured_at = now_iso8601();
    let slug = slug_for(&captured_at, content);
    let dest = dir.join(format!("{slug}.md"));

    let mut fm = Map::new();
    fm.insert("kind".into(), json!(entry_type));
    fm.insert("captured_at".into(), json!(captured_at));
    if !source.is_empty() {
        fm.insert("source".into(), json!(source));
    }
    if !context.is_empty() {
        fm.insert("context".into(), json!(context));
    }
    fm.insert("status".into(), json!("active"));

    let body = format!("{content}\n");
    let doc = doc_with_frontmatter(dest.clone(), fm, body);
    MarkdownStore::write_atomic(&dest, &doc).is_ok()
}

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
    let project_dir_str = input_cwd(input);
    let project = Path::new(&project_dir_str);
    let content = input
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let context = input
        .get("context")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let source = input
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if content.is_empty() {
        eprintln!("[memory] decision: missing content");
        return;
    }
    if !write_decision_or_lesson(project, &entry_type, &content, &source, &context) {
        eprintln!("[memory] decision: markdown write failed (fail-open)");
    }
    emit_decision_event(&entry_type, &content, &context, &source, &project_dir_str);
}

fn run_knowledge(input: &Value) {
    let cwd = input_cwd(input);
    let project = Path::new(&cwd);
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let description = input
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let source = input
        .get("source")
        .and_then(Value::as_str)
        .map(str::to_string);
    let confidence = input
        .get("confidence")
        .and_then(Value::as_f64)
        .filter(|&c| (0.0..=1.0).contains(&c))
        .unwrap_or(0.3);

    if name.is_empty() || description.is_empty() {
        eprintln!("[memory] knowledge: missing name or description");
        return;
    }
    let Some(dir) = knowledge_root(project) else {
        return;
    };
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let captured_at = now_iso8601();
    let pattern = format!("{name}: {description}");
    let slug = slug_for(&captured_at, &pattern);
    let dest = dir.join(format!("{slug}.md"));

    let mut fm = Map::new();
    fm.insert("kind".into(), json!("pattern"));
    fm.insert("name".into(), json!(name));
    fm.insert("captured_at".into(), json!(captured_at));
    fm.insert("confidence".into(), json!(confidence));
    if let Some(s) = source {
        fm.insert("source".into(), json!(s));
    }
    fm.insert("status".into(), json!("active"));

    let body = format!("{description}\n");
    let doc = doc_with_frontmatter(dest.clone(), fm, body);
    let _ = MarkdownStore::write_atomic(&dest, &doc);
}

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
    let mut details = Map::new();
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

/// Persist one agent_memory entry as `.claude/memory/agent/{slug}.md`.
/// Public for hook consumers (auto_capture_summary, stop) that used to call
/// `insert_agent_memory` against SQLite. Fail-open: returns `false` on error.
#[allow(clippy::too_many_arguments)]
pub fn persist_agent_memory_md(
    cwd: &str,
    session_id: Option<&str>,
    spec: Option<&str>,
    wave: Option<i64>,
    role: Option<&str>,
    summary: &str,
    details: Option<&str>,
    confidence: f64,
    status: Option<&str>,
) -> bool {
    let Some(dir) = agent_dir(Path::new(cwd)) else {
        return false;
    };
    if fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let captured_at = now_iso8601();
    let slug = slug_for(&captured_at, summary);
    let dest = dir.join(format!("{slug}.md"));
    let mut fm = Map::new();
    if let Some(s) = session_id {
        fm.insert("session_id".into(), json!(s));
    }
    if let Some(s) = spec {
        fm.insert("spec".into(), json!(s));
    }
    if let Some(w) = wave {
        fm.insert("wave".into(), json!(w));
    }
    if let Some(r) = role {
        fm.insert("role".into(), json!(r));
    }
    fm.insert("summary".into(), json!(summary));
    fm.insert("confidence".into(), json!(confidence));
    fm.insert(
        "status".into(),
        json!(status.unwrap_or("active")),
    );
    fm.insert("at".into(), json!(captured_at.clone()));
    fm.insert("last_used".into(), json!(captured_at));
    let body = details.unwrap_or("").to_string();
    let doc = doc_with_frontmatter(dest.clone(), fm, body);
    MarkdownStore::write_atomic(&dest, &doc).is_ok()
}

// ---------------------------------------------------------------------------
// `list` — scan memory + knowledge dirs, emit a JSON/table dump
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Serialize)]
struct MemoryRow {
    #[serde(rename = "type")]
    entry_type: String,
    name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_seen: Option<String>,
}

fn collect_dir_rows(dir: &Path, kind_label: &str) -> Vec<MemoryRow> {
    let mut rows = Vec::new();
    for doc in MarkdownStore::scan_dir(dir) {
        let fm = match &doc.frontmatter {
            Some(f) => f,
            None => continue,
        };
        let name = fm
            .get_str("name")
            .map(str::to_string)
            .unwrap_or_else(|| {
                // Fall back to a derived short name from body / file.
                doc.path
                    .file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
        // Body is lazy under scan_dir — read once for description.
        let description = MarkdownStore::read_one(&doc.path)
            .map(|d| d.body.trim().chars().take(200).collect::<String>())
            .unwrap_or_default();
        let confidence = fm
            .as_object()
            .and_then(|o| o.get("confidence"))
            .and_then(Value::as_f64);
        let last_seen = fm.get_str("captured_at").map(|s| s.chars().take(10).collect());
        rows.push(MemoryRow {
            entry_type: kind_label.to_string(),
            name,
            description,
            confidence,
            last_seen,
        });
    }
    rows
}

fn run_list(grouped: bool, format: &str) {
    let cwd = project_dir();
    let project = Path::new(&cwd);
    let mut rows: Vec<MemoryRow> = Vec::new();
    if let Some(d) = knowledge_root(project) {
        rows.extend(collect_dir_rows(&d, "pattern"));
    }
    if let Some(d) = decisions_dir(project) {
        rows.extend(collect_dir_rows(&d, "decision"));
    }
    if let Some(d) = lessons_dir(project) {
        rows.extend(collect_dir_rows(&d, "convention"));
    }
    rows.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));

    if format == "table" && grouped {
        render_grouped_table(&rows);
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".to_string())
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
// `write` / `search` / `feedback` — agent_memory under .claude/memory/agent/
// ---------------------------------------------------------------------------

/// Decay applied lazily over `last_used`.
fn parse_iso8601_secs(ts: &str) -> Option<i64> {
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
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

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

pub(crate) fn run_write(opts: WriteOpts) {
    let started = Instant::now();
    let cwd = project_dir();
    let mut report = json!({
        "operation": "memory-write",
        "verify": opts.verify,
        "inserted_path": Value::Null,
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
    let Some(dir) = agent_dir(Path::new(&cwd)) else {
        report["error"] = json!("could not resolve agent dir");
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        emit_memory_economy("write", started.elapsed().as_millis());
        return;
    };
    if fs::create_dir_all(&dir).is_err() {
        report["error"] = json!("could not create agent dir");
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        emit_memory_economy("write", started.elapsed().as_millis());
        return;
    }
    let captured_at = now_iso8601();
    let slug = slug_for(&captured_at, &opts.summary);
    let dest = dir.join(format!("{slug}.md"));

    let session = session_id();
    let mut fm = Map::new();
    if session != "unknown" {
        fm.insert("session_id".into(), json!(session));
    }
    if let Some(s) = &opts.spec {
        fm.insert("spec".into(), json!(s));
    }
    if let Some(w) = opts.wave {
        fm.insert("wave".into(), json!(w));
    }
    if let Some(r) = &opts.role {
        fm.insert("role".into(), json!(r));
    }
    fm.insert("summary".into(), json!(opts.summary));
    fm.insert("confidence".into(), json!(confidence));
    fm.insert("status".into(), json!("active"));
    fm.insert("at".into(), json!(captured_at));
    fm.insert("last_used".into(), json!(captured_at));

    let body = opts.details.clone().unwrap_or_default();
    let doc = doc_with_frontmatter(dest.clone(), fm, body);
    match MarkdownStore::write_atomic(&dest, &doc) {
        Ok(()) => {
            report["inserted_path"] = json!(dest.to_string_lossy().to_string());
            if opts.verify {
                let round_trip = MarkdownStore::read_one(&dest)
                    .ok()
                    .and_then(|d| {
                        d.frontmatter
                            .and_then(|f| f.get_str("summary").map(str::to_string))
                    })
                    .is_some_and(|s| s == opts.summary);
                report["verified"] = json!(round_trip);
            }
        }
        Err(e) => report["error"] = json!(format!("write failed: {e}")),
    }
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
    emit_memory_economy("write", started.elapsed().as_millis());
}

#[derive(Debug, serde::Serialize)]
pub struct SearchRow {
    pub path: PathBuf,
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

#[derive(Debug, Default, Clone)]
pub struct SearchOpts {
    pub query: String,
    pub spec: Option<String>,
    pub cluster: Option<String>,
    pub limit: Option<usize>,
    pub include_low: bool,
}

pub(crate) fn run_search(opts: SearchOpts) {
    let started = Instant::now();
    let cwd = project_dir();
    let now = now_iso8601();
    let limit = opts.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
    let Some(dir) = agent_dir(Path::new(&cwd)) else {
        println!("[]");
        emit_memory_economy("search", started.elapsed().as_millis());
        return;
    };
    let query_lc = opts.query.to_lowercase();
    let mut rows: Vec<SearchRow> = Vec::new();
    for doc in MarkdownStore::scan_dir(&dir) {
        let Some(fm) = &doc.frontmatter else { continue };
        let summary = fm.get_str("summary").map(str::to_string).unwrap_or_default();
        let status = fm
            .get_str("status")
            .map(str::to_string)
            .unwrap_or_else(|| "active".to_string());
        if status != "active" {
            continue;
        }
        if let Some(s) = &opts.spec {
            if fm.get_str("spec").map_or(true, |v| v != s.as_str()) {
                continue;
            }
        }
        if let Some(c) = &opts.cluster {
            if fm.get_str("role").map_or(true, |v| v != c.as_str()) {
                continue;
            }
        }
        // Load body for LIKE matching when needed.
        let full = MarkdownStore::read_one(&doc.path).ok();
        let body = full.as_ref().map(|d| d.body.clone()).unwrap_or_default();
        if !query_lc.is_empty() {
            let hay = format!("{summary}\n{body}").to_lowercase();
            if !hay.contains(&query_lc) {
                continue;
            }
        }
        let confidence = fm
            .as_object()
            .and_then(|o| o.get("confidence"))
            .and_then(Value::as_f64)
            .unwrap_or(0.5);
        let last_used = fm.get_str("last_used").map(str::to_string);
        let eff = effective_confidence(confidence, last_used.as_deref(), &now);
        if !opts.include_low && eff < DEFAULT_MIN_EFFECTIVE_CONFIDENCE {
            continue;
        }
        rows.push(SearchRow {
            path: doc.path.clone(),
            spec: fm.get_str("spec").map(str::to_string),
            wave: fm
                .as_object()
                .and_then(|o| o.get("wave"))
                .and_then(Value::as_i64),
            role: fm.get_str("role").map(str::to_string),
            summary,
            details: Some(body),
            confidence,
            effective_confidence: eff,
            status,
            at: fm.get_str("at").map(str::to_string).unwrap_or_default(),
            last_used,
        });
    }
    rows.sort_by(|a, b| {
        b.effective_confidence
            .partial_cmp(&a.effective_confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.at.cmp(&a.at))
    });
    rows.truncate(limit.max(1));

    // Touch last_used for every returned row.
    for row in &rows {
        let _ = touch_last_used_md(&row.path, &now);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".to_string())
    );
    emit_memory_economy("search", started.elapsed().as_millis());
}

pub(crate) fn touch_last_used_md(path: &Path, ts: &str) -> std::io::Result<()> {
    let mut doc = MarkdownStore::read_one(path)?;
    if let Some(fm) = &mut doc.frontmatter {
        if let Value::Object(map) = &mut fm.0 {
            map.insert("last_used".into(), json!(ts));
        }
    }
    MarkdownStore::write_atomic(path, &doc)
}

#[derive(Debug, Default, Clone)]
pub struct FeedbackOpts {
    pub path: PathBuf,
    pub kind: String,
    pub by_role: Option<String>,
    pub note: Option<String>,
    pub delta: Option<f64>,
}

pub(crate) fn run_feedback(opts: FeedbackOpts) {
    let started = Instant::now();
    let cwd = project_dir();
    let mut report = json!({
        "operation": "memory-feedback",
        "memory_path": opts.path.to_string_lossy().to_string(),
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
    if !opts.path.exists() {
        report["error"] = json!("memory file not found");
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        emit_memory_economy("feedback", started.elapsed().as_millis());
        return;
    }
    let _ = cwd; // used below if dirs needed
    // Append to {slug}.feedback.ndjson sibling.
    let feedback_path = opts
        .path
        .with_extension("feedback.ndjson");
    let at = now_iso8601();
    let line = json!({
        "kind": opts.kind,
        "delta": opts.delta,
        "by_role": opts.by_role,
        "note": opts.note,
        "at": at,
    })
    .to_string()
        + "\n";
    let existing = fs::read_to_string(&feedback_path).unwrap_or_default();
    if fs::write_atomic(&feedback_path, format!("{existing}{line}").as_bytes()).is_ok() {
        report["appended"] = json!(true);
    }
    // For deprecate/supersede, flip frontmatter status.
    if matches!(opts.kind.as_str(), "deprecate" | "supersede") {
        let new_status = if opts.kind == "deprecate" {
            "deprecated"
        } else {
            "superseded"
        };
        if let Ok(mut doc) = MarkdownStore::read_one(&opts.path) {
            if let Some(fm) = &mut doc.frontmatter {
                if let Value::Object(map) = &mut fm.0 {
                    map.insert("status".into(), json!(new_status));
                }
            }
            if MarkdownStore::write_atomic(&opts.path, &doc).is_ok() {
                report["status_updated"] = json!(true);
            }
        }
    }
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
    emit_memory_economy("feedback", started.elapsed().as_millis());
}

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
    let _ = crate::shared::events::route::emit(&cwd, &ev);
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
pub struct DispatchExtras {
    pub cluster: Option<String>,
    pub query: Option<String>,
    pub kind: Option<String>,
    pub role: Option<String>,
    pub details: Option<String>,
    pub confidence: Option<f64>,
    pub verify: bool,
    pub include_low: bool,
    pub limit: Option<usize>,
    pub by_role: Option<String>,
    pub note: Option<String>,
    pub feedback_path: Option<PathBuf>,
}

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
        crate::commands::knowledge::memory_cross_wave::run(spec, wave, extras.cluster.as_deref());
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
            path: extras.feedback_path.clone().unwrap_or_default(),
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
        return;
    }
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
    fn slug_is_deterministic() {
        let a = slug_for("2026-05-27T00:00:00.000Z", "hello");
        let b = slug_for("2026-05-27T00:00:00.000Z", "hello");
        assert_eq!(a, b);
        let c = slug_for("2026-05-27T00:00:00.000Z", "world");
        assert_ne!(a, c);
    }

    #[test]
    fn agent_writes_entry_and_index() {
        let dir = tempdir().unwrap();
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
        let index = dir
            .path()
            .join(".claude")
            .join("agent-memory")
            .join("_index.json");
        let parsed: Vec<Value> =
            serde_json::from_str(&std::fs::read_to_string(index).unwrap()).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["agent_type"], json!("backend"));
    }

    #[test]
    fn decision_writes_markdown_file() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), b"{}").unwrap();
        let input = json!({
            "cwd": dir.path().to_string_lossy(),
            "type": "decision",
            "content": "chose markdown over sqlite",
            "source": "spec-1",
        });
        run_decision(&input);
        let dir_path = dir.path().join(".claude").join("memory").join("decisions");
        assert!(dir_path.exists());
        let count = std::fs::read_dir(&dir_path)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(count, 1, "one .md file expected");
    }

    #[test]
    fn knowledge_writes_pattern_markdown() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), b"{}").unwrap();
        let input = json!({
            "cwd": dir.path().to_string_lossy(),
            "name": "fail-open",
            "description": "hooks never abort user work",
            "confidence": 0.8,
        });
        run_knowledge(&input);
        let dir_path = dir.path().join(".claude").join("knowledge");
        let count = std::fs::read_dir(&dir_path)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(count, 1);
    }

    #[test]
    fn decay_drops_old_rows_below_threshold() {
        let now = "2026-06-25T00:00:00.000Z";
        let last = "2026-05-25T00:00:00.000Z";
        let eff = effective_confidence(0.5, Some(last), now);
        assert!(eff < DEFAULT_MIN_EFFECTIVE_CONFIDENCE);
    }

    #[test]
    fn decay_preserves_recent_rows() {
        let now = "2026-05-25T01:00:00.000Z";
        let last = "2026-05-25T00:00:00.000Z";
        let eff = effective_confidence(0.9, Some(last), now);
        assert!(eff > 0.85);
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
    fn feedback_kind_validation_rejects_unknown() {
        assert!(FEEDBACK_KINDS.contains(&"bump"));
        assert!(!FEEDBACK_KINDS.contains(&"nuke"));
    }

    #[test]
    fn parse_iso8601_secs_round_trips_known_timestamp() {
        let s = parse_iso8601_secs("2026-05-25T00:00:00Z").unwrap();
        assert!(s > 1_735_689_600);
    }
}
