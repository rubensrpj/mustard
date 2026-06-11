use notify::RecommendedWatcher;
use notify_debouncer_mini::{new_debouncer, Debouncer, DebounceEventResult};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

/// Emission throttle window: at most one push per `last_emit` key inside this
/// window. The debouncer already coalesces a write burst into one ~200 ms
/// batch, so this mainly dedupes WITHIN a batch (same kind seen N times → one
/// emit) and guards pathological back-to-back batches.
const EMIT_THROTTLE: Duration = Duration::from_millis(100);

/// Throttle-map key suffix for the aggregated specs-snapshot push. Distinct
/// from every `classify_kind` kind so it never collides with an fs-change key.
const SNAPSHOT_KEY: &str = "specs-snapshot";

/// What one debounced batch tells the frontend: the `dashboard:fs-change`
/// kinds to emit (at most one per kind per batch — the throttle dedupes) and
/// whether ONE aggregated specs-snapshot rebuild must be scheduled. Split from
/// the debouncer callback so tests can drive batches without a Tauri
/// `AppHandle` and assert the one-rebuild-one-emission-per-burst contract.
struct BatchEmissions {
    fs_change_kinds: Vec<&'static str>,
    rebuild_snapshot: bool,
}

/// Process one debounced batch.
///
/// Surgical cache maintenance runs FIRST, for EVERY path in the batch — before
/// any emit throttling, which coalesces same-kind notifications and must never
/// skip an invalidation along with the emit:
///
///   * `events` — mark exactly the changed `.ndjson` shard dirty; the next
///     read re-parses ONLY that file (the incremental contract). Non-ndjson
///     `events` writes (telemetry.db + WAL/SHM) never feed the parsed
///     snapshot and are ignored inside `invalidate_events_cache_path`.
///   * `spec` — drop the cached spec list (spec.md / wave-plan.md / meta.json
///     back it). A spec path that no longer exists means a deletion the
///     per-path marking can't see (the shards under it vanished without their
///     own events) — fall back to a full sweep of the events cache.
///   * `knowledge` / `pipeline-state` — read on-disk files, not the cached
///     event vec; no cache work needed.
///
/// Emission decisions are batch-level: `dashboard:fs-change` keeps its
/// per-kind throttle (compatibility channel for every non-specs page), and the
/// aggregated snapshot is scheduled AT MOST ONCE per batch — a burst of NDJSON
/// writes lands here as one ~200 ms debounced batch, and the shared
/// [`EMIT_THROTTLE`] additionally guards back-to-back batches.
fn process_batch(
    state: &Mutex<WatcherState>,
    repo: &str,
    paths: &[PathBuf],
    now: Instant,
) -> BatchEmissions {
    let mut fs_change_kinds: Vec<&'static str> = Vec::new();
    let mut snapshot_dirty = false;
    for path in paths {
        let Some(kind) = classify_kind(path) else { continue };
        match kind {
            "events" => {
                crate::telemetry::invalidate_events_cache_path(repo, path);
                // Only parsed `.ndjson` shards feed the aggregated snapshot;
                // telemetry.db (+ WAL/SHM) classifies as `events` but never
                // changes the spec list / active-pipeline projections.
                if path.extension().and_then(|s| s.to_str()) == Some("ndjson") {
                    snapshot_dirty = true;
                }
            }
            "spec" => {
                crate::invalidate_specs_cache(repo);
                if !path.exists() {
                    crate::telemetry::invalidate_events_cache(repo);
                }
                snapshot_dirty = true;
            }
            _ => {}
        }
        if throttle_allows(state, &format!("{repo}|{kind}"), now) {
            fs_change_kinds.push(kind);
        }
    }
    let rebuild_snapshot =
        snapshot_dirty && throttle_allows(state, &format!("{repo}|{SNAPSHOT_KEY}"), now);
    BatchEmissions {
        fs_change_kinds,
        rebuild_snapshot,
    }
}

/// One throttle probe-and-mark over `WatcherState::last_emit`: true when `key`
/// has not emitted inside [`EMIT_THROTTLE`], marking it as emitted at `now` in
/// that case. Fail-open on a poisoned lock: no emission — invalidation already
/// happened, so the next batch (or the page's own fetch) corrects staleness.
fn throttle_allows(state: &Mutex<WatcherState>, key: &str, now: Instant) -> bool {
    let Ok(mut guard) = state.lock() else {
        return false;
    };
    let allows = guard
        .last_emit
        .get(key)
        .is_none_or(|t| now.duration_since(*t) > EMIT_THROTTLE);
    if allows {
        guard.last_emit.insert(key.to_string(), now);
    }
    allows
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
            let paths: Vec<PathBuf> = events.into_iter().map(|ev| ev.path).collect();
            let emissions = process_batch(&state_clone, &repo_clone, &paths, Instant::now());
            // `dashboard:fs-change` stays the compatibility channel for every
            // page keyed off a kind (events / spec / knowledge / pipeline-state)
            // — the snapshot push below only covers the specs views.
            for kind in emissions.fs_change_kinds {
                let _ = app_clone.emit(
                    "dashboard:fs-change",
                    FsChangePayload {
                        repo_path: repo_clone.clone(),
                        kind: kind.to_string(),
                    },
                );
            }
            if emissions.rebuild_snapshot {
                // Rebuild the aggregated snapshot OFF this thread: the
                // callback runs on the debouncer's own thread and must never
                // pay the fold itself. `spawn_blocking` moves the rebuild to
                // the runtime's blocking pool; it starts from the incremental
                // caches invalidated inside `process_batch` (milliseconds when
                // warm — only the touched shards are re-parsed) and the emit
                // ships the payload ready to render. At most one rebuild +
                // one emission per burst: scheduling is batch-level and
                // throttled in `process_batch`.
                let repo = repo_clone.clone();
                let app = app_clone.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let snapshot = crate::build_specs_snapshot(&repo);
                    let _ = app.emit("dashboard:specs-snapshot", snapshot);
                });
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
    use super::{classify_kind, process_batch, WatcherState};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    fn kind(p: &str) -> Option<&'static str> {
        classify_kind(Path::new(p))
    }

    /// One NDJSON event line the parsed-events cache accepts.
    fn event_line(spec: &str, ts: &str) -> String {
        format!(
            r#"{{"event":"pipeline.phase","kind":"pipeline","ts":"{ts}","spec":"{spec}","payload":{{"phase":"EXECUTE"}}}}"#
        )
    }

    /// Create `.claude/spec/{spec}/.events/{name}` under `root` with `body`.
    fn write_shard(root: &Path, spec: &str, name: &str, body: &str) -> PathBuf {
        let dir = root.join(".claude").join("spec").join(spec).join(".events");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn ndjson_burst_yields_one_snapshot_schedule_and_one_fs_change() {
        // A burst of NDJSON writes arrives as ONE debounced batch — it must
        // schedule exactly ONE snapshot rebuild and ONE `events` fs-change.
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().to_string_lossy().into_owned();
        let paths: Vec<PathBuf> = (0..4)
            .map(|i| {
                write_shard(
                    tmp.path(),
                    "s1",
                    &format!("{i}.ndjson"),
                    &event_line("s1", "2026-06-11T10:00:00.000Z"),
                )
            })
            .collect();
        let state = Mutex::new(WatcherState::default());
        let now = Instant::now();

        let out = process_batch(&state, &repo, &paths, now);
        assert_eq!(out.fs_change_kinds, vec!["events"]);
        assert!(out.rebuild_snapshot);

        // A trailing batch inside the 100 ms throttle window (same burst)
        // must not schedule a second rebuild nor a second fs-change.
        let out2 = process_batch(&state, &repo, &paths, now + Duration::from_millis(50));
        assert!(out2.fs_change_kinds.is_empty());
        assert!(!out2.rebuild_snapshot);

        // Past the window the channel re-arms for the next real change.
        let out3 = process_batch(&state, &repo, &paths, now + Duration::from_millis(250));
        assert_eq!(out3.fs_change_kinds, vec!["events"]);
        assert!(out3.rebuild_snapshot);
    }

    #[test]
    fn non_snapshot_kinds_do_not_schedule_a_rebuild() {
        // knowledge files and telemetry.db (classifies `events` but is not a
        // parsed shard) keep their fs-change channel without ever scheduling
        // the aggregated snapshot rebuild.
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().to_string_lossy().into_owned();
        let state = Mutex::new(WatcherState::default());
        let paths = vec![
            tmp.path().join(".claude").join("knowledge.json"),
            tmp.path().join(".claude").join(".harness").join("telemetry.db"),
        ];
        let out = process_batch(&state, &repo, &paths, Instant::now());
        assert_eq!(out.fs_change_kinds, vec!["knowledge", "events"]);
        assert!(!out.rebuild_snapshot);
    }

    #[test]
    fn spec_write_schedules_a_rebuild() {
        // A spec.md write changes the spec list — the snapshot must follow.
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().to_string_lossy().into_owned();
        let spec_dir = tmp.path().join(".claude").join("spec").join("s1");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let spec_md = spec_dir.join("spec.md");
        std::fs::write(&spec_md, "# s1\n").unwrap();
        let state = Mutex::new(WatcherState::default());
        let out = process_batch(&state, &repo, std::slice::from_ref(&spec_md), Instant::now());
        assert_eq!(out.fs_change_kinds, vec!["spec"]);
        assert!(out.rebuild_snapshot);
    }

    #[test]
    fn touched_shard_is_the_only_file_reparsed_by_the_rebuild() {
        // The incremental contract THROUGH the snapshot path: a warm rebuild
        // reads nothing; touching one shard re-parses exactly that file.
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().to_string_lossy().into_owned();
        let spec_dir = tmp.path().join(".claude").join("spec").join("s1");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# s1\n").unwrap();
        let a = write_shard(
            tmp.path(),
            "s1",
            "a.ndjson",
            &format!("{}\n", event_line("s1", "2026-06-11T10:00:00.000Z")),
        );
        let _b = write_shard(
            tmp.path(),
            "s1",
            "b.ndjson",
            &format!("{}\n", event_line("s1", "2026-06-11T10:00:01.000Z")),
        );

        // Cold rebuild parses both shards and sees the spec in the list.
        let snap = crate::build_specs_snapshot(&repo);
        assert!(snap.specs.iter().any(|r| r.name == "s1"));
        assert_eq!(snap.repo_path, repo);
        let cold = crate::telemetry::events_cache_parsed_files(tmp.path());
        assert_eq!(cold, 2);

        // Warm rebuild with nothing marked dirty re-reads NOTHING.
        let _ = crate::build_specs_snapshot(&repo);
        assert_eq!(crate::telemetry::events_cache_parsed_files(tmp.path()), cold);

        // Append to ONE shard, run the same invalidation the watcher batch
        // does, rebuild: exactly one extra parse.
        {
            use std::io::Write as _;
            let mut f = std::fs::OpenOptions::new().append(true).open(&a).unwrap();
            writeln!(f, "{}", event_line("s1", "2026-06-11T10:00:02.000Z")).unwrap();
        }
        let state = Mutex::new(WatcherState::default());
        let out = process_batch(&state, &repo, std::slice::from_ref(&a), Instant::now());
        assert!(out.rebuild_snapshot);
        let _ = crate::build_specs_snapshot(&repo);
        assert_eq!(
            crate::telemetry::events_cache_parsed_files(tmp.path()),
            cold + 1
        );
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
