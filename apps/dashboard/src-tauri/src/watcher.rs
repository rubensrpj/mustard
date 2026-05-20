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
    // mustard.db and its WAL/SHM companions all trigger a data-change refresh.
    if s.contains(".harness") && (s.contains("mustard.db")) {
        Some("events")
    } else if s.contains(".pipeline-states") {
        Some("pipeline-state")
    } else if s.contains("spec") && (s.contains("spec/active") || s.contains("spec\\active")) {
        Some("spec")
    } else if s.contains(".claude") && s.contains("knowledge.json") {
        Some("knowledge")
    } else if s.contains(".claude") && s.contains("memory") && (s.contains("decisions.json") || s.contains("lessons.json")) {
        Some("memory")
    } else {
        None
    }
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
