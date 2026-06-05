use notify::RecommendedWatcher;
use notify_debouncer_mini::{new_debouncer, Debouncer, DebounceEventResult};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use mustard_core::ClaudePaths;

#[derive(Default)]
pub struct WatcherState {
    pub watchers: HashMap<String, Debouncer<RecommendedWatcher>>,
    pub last_emit: HashMap<String, Instant>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "snake_case")]
struct FsChangePayload {
    repo_path: String,
    kind: String,
}

pub fn classify_kind(path: &Path) -> Option<&'static str> {
    let s = path.to_string_lossy();
    // Per-spec / per-session NDJSON event logs are the canonical data source
    // after the SQLite→NDJSON migration: `.claude/spec/{name}/.events/*.ndjson`,
    // `.claude/spec/{name}/wave-N-*/.events/*.ndjson`, and
    // `.claude/.session/{id}/.events/*.ndjson`. Any write to one of these is a
    // data-change event and must refresh the rebuilt views (recent-events /
    // metrics / activity / telemetry / spec-timeline / sessions / knowledge).
    // This MUST run before the generic `/spec/` branch below so a spec event
    // log is classified `events` rather than `spec`. Note `is_events_log` keys
    // off the `.events` segment, so a plain `spec.md` write (not under
    // `.events/`) falls through to the spec branch unchanged.
    if is_events_log(&s) {
        Some("events")
    } else if s.contains("telemetry.db") {
        // The OTEL collector still writes `telemetry.db` (run_usage /
        // usage_totals) and its WAL/SHM companions; those writes refresh the
        // economy/telemetry views via the same `events` channel.
        Some("events")
    } else if s.contains(".pipeline-states") {
        Some("pipeline-state")
    } else if (s.contains("/spec/") || s.contains("\\spec\\")) && !s.contains(".pipeline-states") {
        // Flat layout: .claude/spec/{name}/spec.md — any non-`.events` write
        // inside the spec directory (regardless of bucket) is a spec-change
        // event (event-log writes were already captured above).
        Some("spec")
    } else if is_knowledge_path(&s) {
        // Wave 3 (2026-05-22): event-driven knowledge refresh. The knowledge
        // base now derives from the per-spec/.session NDJSON event log (the
        // `events` branch above invalidates the knowledge query keys for that
        // reason). This branch additionally classifies any legacy/file-based
        // knowledge or memory path (`knowledge.json`, `memory/decisions.json`,
        // `memory/lessons.json`) as `knowledge` so a file writer is covered too,
        // letting the Knowledge page stop relying on a 10s `refetchInterval`.
        Some("knowledge")
    } else {
        None
    }
}

/// Recognise the per-spec / per-session NDJSON event-log writes that back every
/// rebuilt dashboard view. Matches any path whose normalized form contains an
/// `events`-log directory segment and ends with `.ndjson` — covering the dotted
/// `.claude/spec/{name}/.events/`, `wave-N-*/.events/`, and
/// `.claude/.session/{id}/.events/` AND the non-dotted `wave-N-*/events/` that
/// the events walker also reads (`telemetry::walk_ndjson_events` →
/// `wp.join("events")`). Without the non-dotted case a write to a wave's
/// `events/` dir would refresh the cached view set but never invalidate the
/// parsed-events cache. Handles both `/` and `\` separators.
fn is_events_log(s: &str) -> bool {
    if !s.ends_with(".ndjson") {
        return false;
    }
    // Dotted `.events/` (spec, wave, session) OR non-dotted `events/` directory
    // segment (wave-level walker). The leading separator boundary keeps this
    // from matching an unrelated basename like `myevents.ndjson`.
    s.contains(".events")
        || s.contains("/events/")
        || s.contains("\\events\\")
}

/// Recognise file paths that back the knowledge base (file-based variants).
/// NDJSON-backed knowledge changes arrive via the `is_events_log` → `events`
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

    // Watch the project's `.claude/` recursively — the recursive mode below
    // covers every documented child (harness DB, pipeline-states JSON,
    // spec/{name}/qa-report.json, spec/{name}/wave-N-{role}/*) without
    // narrowing. `classify_kind` is the dispatch table for the file → channel
    // mapping; this resolver is purely about WHERE to attach the watcher.
    let watch_root = match ClaudePaths::for_project(Path::new(&repo_path)) {
        Ok(paths) => paths.claude_dir(),
        // The only failure mode is the I1 guard — return early rather than
        // attach a watcher to a forbidden `.claude/.claude/` path.
        Err(_) => return Ok(()),
    };
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
                    .is_none_or(|t| now.duration_since(*t) > Duration::from_millis(100));
                if !should_emit {
                    continue;
                }
                guard.last_emit.insert(key, now);
                drop(guard);
                // Invalidate the per-project parsed-events cache BEFORE notifying
                // the frontend, so the first command in the refresh burst
                // re-parses fresh and the rest hit the warm slice. Only the kinds
                // that change the parsed NDJSON event set matter (events / spec /
                // pipeline-state); a `knowledge` change reads on-disk files, not
                // the cached event vec, so it never needs to drop the cache.
                if matches!(kind, "events" | "spec" | "pipeline-state") {
                    crate::telemetry::invalidate_events_cache(&repo_clone);
                }
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

#[cfg(test)]
mod tests {
    use super::classify_kind;
    use std::path::Path;

    fn kind(p: &str) -> Option<&'static str> {
        classify_kind(Path::new(p))
    }

    #[test]
    fn spec_event_log_classifies_as_events() {
        // Per-spec NDJSON event log — the canonical data source post-migration.
        assert_eq!(
            kind(r"C:\repo\.claude\spec\my-feature\.events\1780376456486066300-r45196.ndjson"),
            Some("events"),
        );
        assert_eq!(
            kind("/repo/.claude/spec/my-feature/.events/1780376456486066300-r45196.ndjson"),
            Some("events"),
        );
    }

    #[test]
    fn wave_event_log_classifies_as_events() {
        // wave-N-{role}/.events/*.ndjson must also refresh the rebuilt views.
        assert_eq!(
            kind(r"C:\repo\.claude\spec\my-feature\wave-1-impl\.events\x.ndjson"),
            Some("events"),
        );
        assert_eq!(
            kind("/repo/.claude/spec/my-feature/wave-1-impl/.events/x.ndjson"),
            Some("events"),
        );
    }

    #[test]
    fn non_dotted_wave_events_dir_classifies_as_events() {
        // FIX 3: the events walker also reads non-dotted `wave-N-*/events/`
        // (telemetry::walk_ndjson_events → `wp.join("events")`). A write there
        // must invalidate the events cache, so it has to classify as `events`,
        // not fall through to the `spec` branch.
        assert_eq!(
            kind(r"C:\repo\.claude\spec\my-feature\wave-1-impl\events\x.ndjson"),
            Some("events"),
        );
        assert_eq!(
            kind("/repo/.claude/spec/my-feature/wave-1-impl/events/x.ndjson"),
            Some("events"),
        );
    }

    #[test]
    fn session_event_log_classifies_as_events() {
        // `.claude/.session/{id}/.events/*.ndjson` matched nothing before the
        // fix (→ None → no refresh); it must now be an `events` change.
        assert_eq!(
            kind(r"C:\repo\.claude\.session\19a1f60b-edc3\.events\x.ndjson"),
            Some("events"),
        );
        assert_eq!(
            kind("/repo/.claude/.session/19a1f60b-edc3/.events/x.ndjson"),
            Some("events"),
        );
        // The `unknown` attribution bucket is just another session dir.
        assert_eq!(
            kind("/repo/.claude/.session/unknown/.events/x.ndjson"),
            Some("events"),
        );
    }

    #[test]
    fn spec_md_still_classifies_as_spec() {
        // A spec.md write (NOT under `.events/`) must remain a `spec` change.
        assert_eq!(
            kind(r"C:\repo\.claude\spec\my-feature\spec.md"),
            Some("spec"),
        );
        assert_eq!(
            kind("/repo/.claude/spec/my-feature/spec.md"),
            Some("spec"),
        );
        // wave plan markdown inside the spec dir is also a spec change.
        assert_eq!(
            kind("/repo/.claude/spec/my-feature/wave-1-impl/spec.md"),
            Some("spec"),
        );
    }

    #[test]
    fn telemetry_db_classifies_as_events() {
        // The OTEL collector still writes telemetry.db (+ WAL/SHM companions).
        assert_eq!(
            kind(r"C:\repo\.claude\.harness\telemetry.db"),
            Some("events"),
        );
        assert_eq!(
            kind("/repo/.claude/.harness/telemetry.db-wal"),
            Some("events"),
        );
    }

    #[test]
    fn pipeline_states_classifies_as_pipeline_state() {
        assert_eq!(
            kind("/repo/.claude/.pipeline-states/abc.json"),
            Some("pipeline-state"),
        );
    }

    #[test]
    fn knowledge_json_classifies_as_knowledge() {
        assert_eq!(
            kind("/repo/.claude/knowledge.json"),
            Some("knowledge"),
        );
        assert_eq!(
            kind("/repo/.claude/memory/decisions.json"),
            Some("knowledge"),
        );
    }

    #[test]
    fn unrelated_path_classifies_as_none() {
        assert_eq!(kind("/repo/src/main.rs"), None);
    }
}
