//! `mustard-rt run transcript-watcher` — opt-in JSONL transcript watcher daemon.
//!
//! ## Scope (Wave 3 — economia-moat-unification)
//!
//! Tails `~/.claude/projects/<encoded-cwd>/*.jsonl` (recursive across every
//! project directory Claude Code maintains) and re-ingests the active session
//! transcripts into the unified economy `spans` table whenever Claude Code
//! appends a turn. The complement to `session_cleanup`'s one-shot ingest at
//! `SessionEnd`: this daemon lands frames live so the dashboard sees costs
//! during the session, not only after it ends.
//!
//! ## Lifecycle
//!
//! Spawned detached by [`crate::hooks::session_start`] when the env var
//! `MUSTARD_TRANSCRIPT_WATCH=1` is set. Runs until process termination.
//! Cross-platform signal handling without `unsafe` is limited; the watcher
//! relies on the OS default action (terminate) and an in-process `Atomic`
//! flag exposed only via tests. A `recv_timeout(1s)` polling loop guarantees
//! the loop wakes regularly so a future SIGINT installer can flip the flag
//! cleanly without a stuck `recv`.
//!
//! ## Fail-open
//!
//! A missing `notify` watcher init, a missing home directory, or a per-event
//! ingest failure is logged via `eprintln!` and the loop continues. The
//! watcher never propagates an error to the parent.

use mustard_core::economy::{self, sources::transcript, sources::IngestContext};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// How long [`recv_timeout`] blocks before re-checking the shutdown flag.
/// Short enough that an out-of-band kill propagates quickly, long enough that
/// idle CPU stays near zero.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Resolve the user's home directory cross-platform without a `dirs` crate
/// dependency. Mirrors the helper in [`crate::hooks::session_cleanup`] — kept
/// duplicated rather than pulled into `util` so each module stays self-contained.
fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Filesystem root the watcher recurses under. Returns `None` when the home
/// directory cannot be resolved (the caller logs and exits cleanly).
fn watch_root() -> Option<PathBuf> {
    Some(home_dir()?.join(".claude").join("projects"))
}

/// Decide whether `path` is a transcript file the ingest should run on.
/// Two filters: the suffix must be `.jsonl`, and the path must not be inside
/// a `.cache` directory (Claude Code writes scratch files there).
fn is_transcript_path(path: &Path) -> bool {
    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return false;
    }
    path.components()
        .all(|c| c.as_os_str().to_string_lossy() != ".cache")
}

/// Re-ingest a single transcript file into the W1 economy writer. Fail-open
/// at every step — a single bad file must not stop the daemon.
fn ingest_one(path: &Path) {
    // Best-effort project attribution: `notify` reports the absolute path, so
    // the project root is the parent of the project directory (which is the
    // URL-encoded cwd Claude Code generated). We cannot reliably round-trip
    // that back to the original cwd here, so we anchor the records on the
    // `cwd` of the daemon process — the watcher is normally spawned by the
    // session's own SessionStart, so `current_dir()` is the right project.
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());

    let ctx = IngestContext::for_project(&cwd);
    let frames = match transcript::ingest(path, &ctx) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "transcript_watcher: transcript::ingest failed for {}: {e}",
                path.display()
            );
            return;
        }
    };
    if frames.is_empty() {
        return;
    }
    let conn = match economy::store::open_for(&cwd) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("transcript_watcher: economy::store::open_for failed: {e}");
            return;
        }
    };
    for frame in frames {
        if let Err(e) = economy::writer::record_api_cost(&conn, frame) {
            eprintln!("transcript_watcher: record_api_cost failed: {e}");
        }
    }
}

/// Watch every `*.jsonl` under `~/.claude/projects/` for create/modify events
/// and re-ingest them as they change.
///
/// Runs until the in-process `shutdown` flag flips (test-only seam) or until
/// the channel breaks (production: the OS terminates the process, the
/// `mpsc::channel` drops, `recv_timeout` returns `Disconnected`, and the loop
/// exits cleanly).
fn watch_loop(shutdown: &AtomicBool) {
    let Some(root) = watch_root() else {
        eprintln!(
            "transcript_watcher: could not resolve home directory; exiting (set HOME or USERPROFILE)"
        );
        return;
    };
    if !root.exists() {
        eprintln!(
            "transcript_watcher: watch root {} does not exist yet; exiting (will be created by Claude Code on first session)",
            root.display()
        );
        return;
    }

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    // `recommended_watcher` picks the best backend per platform
    // (inotify/FSEvents/ReadDirectoryChangesW). `notify = "6"` returns
    // `notify::Result`; we degrade to fail-open on watcher init failure.
    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("transcript_watcher: notify::recommended_watcher failed: {e}; exiting");
            return;
        }
    };
    if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
        eprintln!(
            "transcript_watcher: watch({}) failed: {e}; exiting",
            root.display()
        );
        return;
    }

    while !shutdown.load(Ordering::SeqCst) {
        match rx.recv_timeout(POLL_INTERVAL) {
            Ok(Ok(event)) => handle_event(&event),
            Ok(Err(e)) => {
                eprintln!("transcript_watcher: notify event error: {e}; continuing");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // The watcher dropped its send half — terminate cleanly.
                return;
            }
        }
    }
}

/// Dispatch one `notify::Event`: re-ingest every `*.jsonl` path it touches.
///
/// `notify` v6 reports `Modify` for appends on most backends and `Create` for
/// the initial JSONL write. Both reach this branch; non-matching kinds are
/// silently skipped.
fn handle_event(event: &Event) {
    if !matches!(
        event.kind,
        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Any
    ) {
        return;
    }
    for path in &event.paths {
        if is_transcript_path(path) {
            ingest_one(path);
        }
    }
}

/// Dispatch `mustard-rt run transcript-watcher`. Runs the watch loop until
/// termination.
pub fn run() {
    let shutdown = AtomicBool::new(false);
    watch_loop(&shutdown);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_transcript_path_accepts_jsonl_and_rejects_cache() {
        assert!(is_transcript_path(Path::new(
            "/home/u/.claude/projects/proj/abc.jsonl"
        )));
        assert!(!is_transcript_path(Path::new(
            "/home/u/.claude/projects/proj/.cache/abc.jsonl"
        )));
        assert!(!is_transcript_path(Path::new(
            "/home/u/.claude/projects/proj/abc.txt"
        )));
    }

    #[test]
    fn handle_event_ignores_non_matching_kinds() {
        // Sanity check: a `Remove` event is silently dropped.
        let event = Event {
            kind: EventKind::Remove(notify::event::RemoveKind::File),
            paths: vec![PathBuf::from("/tmp/x.jsonl")],
            attrs: notify::event::EventAttributes::default(),
        };
        // Just exercising the no-panic path — nothing observable to assert
        // without a real ingest target.
        handle_event(&event);
    }
}
