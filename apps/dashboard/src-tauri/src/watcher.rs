use notify::RecommendedWatcher;
use notify_debouncer_mini::{new_debouncer, Debouncer, DebounceEventResult};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub struct WatcherState {
    pub watchers: HashMap<String, Debouncer<RecommendedWatcher>>,
    pub last_emit: HashMap<String, Instant>,
}

impl Default for WatcherState {
    fn default() -> Self {
        Self {
            watchers: HashMap::new(),
            last_emit: HashMap::new(),
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "snake_case")]
struct FsChangePayload {
    repo_path: String,
    kind: String,
}

pub fn classify_kind(path: &Path) -> Option<&'static str> {
    let s = path.to_string_lossy();
    // mustard.db / telemetry.db and their WAL/SHM companions all trigger a
    // data-change refresh. telemetry.db (run_usage / usage_totals) is written
    // by the OTEL collector; its writes must refresh the economy/telemetry
    // views just like a mustard.db write does.
    if s.contains(".harness") && (s.contains("mustard.db") || s.contains("telemetry.db")) {
        Some("events")
    } else if s.contains(".pipeline-states") {
        Some("pipeline-state")
    } else if (s.contains("/spec/") || s.contains("\\spec\\")) && !s.contains(".pipeline-states") {
        // Flat layout: .claude/spec/{name}/spec.md — any write inside the
        // spec directory (regardless of bucket) is a spec-change event.
        Some("spec")
    } else if is_knowledge_path(&s) {
        // Wave 3 (2026-05-22): re-enable event-driven knowledge refresh. The
        // knowledge base now lives in `mustard.db` (tables
        // `knowledge_patterns` / `memory_decisions` / `memory_lessons`), so a DB
        // write is the primary trigger — the frontend `events` branch also
        // invalidates the knowledge query keys for that reason. This branch
        // additionally classifies any legacy/file-based knowledge or memory
        // path (`knowledge.json`, `memory/decisions.json`,
        // `memory/lessons.json`) as `knowledge` so a file writer is covered too,
        // letting the Knowledge page stop relying on a 10s `refetchInterval`.
        Some("knowledge")
    } else {
        None
    }
}

/// Recognise file paths that back the knowledge base (file-based variants).
/// SQLite-backed knowledge changes arrive via the `mustard.db` → `events`
/// branch; this covers the JSON fallbacks that some installs still write.
fn is_knowledge_path(s: &str) -> bool {
    let has_knowledge_json = s.contains("knowledge.json");
    let has_memory_doc = (s.contains("/memory/") || s.contains("\\memory\\"))
        && (s.contains("decisions.json") || s.contains("lessons.json"));
    has_knowledge_json || has_memory_doc
}

pub fn ensure_watching(
    state: Arc<Mutex<WatcherState>>,
    repo_path: String,
    app: AppHandle,
) -> Result<(), String> {
    {
        let s = state.lock().map_err(|e| e.to_string())?;
        if s.watchers.contains_key(&repo_path) {
            return Ok(());
        }
    }

    let watch_root = Path::new(&repo_path).join(".claude");
    if !watch_root.exists() {
        return Ok(());
    }

    let state_clone = state.clone();
    let repo_clone = repo_path.clone();
    let app_clone = app.clone();

    let mut debouncer = new_debouncer(
        Duration::from_millis(200),
        move |res: DebounceEventResult| {
            let Ok(events) = res else { return };
            for ev in events {
                let Some(kind) = classify_kind(&ev.path) else { continue };
                let key = format!("{}|{}", repo_clone, kind);
                let now = Instant::now();
                let mut guard = match state_clone.lock() {
                    Ok(g) => g,
                    Err(_) => continue,
                };
                let should_emit = guard
                    .last_emit
                    .get(&key)
                    .map_or(true, |t| now.duration_since(*t) > Duration::from_millis(100));
                if !should_emit {
                    continue;
                }
                guard.last_emit.insert(key, now);
                drop(guard);
                let _ = app_clone.emit(
                    "dashboard:fs-change",
                    FsChangePayload {
                        repo_path: repo_clone.clone(),
                        kind: kind.to_string(),
                    },
                );
            }
        },
    )
    .map_err(|e| {
        eprintln!("watcher: debouncer init failed for {}: {}", repo_path, e);
        e.to_string()
    })?;

    debouncer
        .watcher()
        .watch(&watch_root, notify::RecursiveMode::Recursive)
        .map_err(|e| {
            eprintln!("watcher: watch failed for {}: {}", watch_root.display(), e);
            e.to_string()
        })?;

    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.watchers.insert(repo_path, debouncer);
    Ok(())
}
