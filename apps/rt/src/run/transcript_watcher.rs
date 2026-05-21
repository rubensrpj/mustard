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
//! ## `--once` (backfill mode)
//!
//! `mustard-rt run transcript-watcher --once` runs a single sweep of the
//! current cwd's project directory under `~/.claude/projects/<encoded-cwd>/`,
//! ingests every `*.jsonl` it finds, and exits. Useful for seeding the economy
//! tables from existing transcripts without leaving the daemon resident.
//!
//! ## Fail-open
//!
//! A missing `notify` watcher init, a missing home directory, or a per-event
//! ingest failure is logged via `eprintln!` and the loop continues. The
//! watcher never propagates an error to the parent.

use mustard_core::economy::{self, sources::transcript, sources::IngestContext};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// How long [`recv_timeout`] blocks before re-checking the shutdown flag.
/// Short enough that an out-of-band kill propagates quickly, long enough that
/// idle CPU stays near zero.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

// `home_dir` (and `encode_cwd`, used by `session_cleanup`) live in
// `crate::util` after the b3 Wave 5 review-bundle consolidation: a duplicated
// `:`-collapsing rule would silently break transcript discovery if one side
// drifted. Imported here.
use crate::util::{encode_cwd, home_dir};

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

/// List every `*.jsonl` file (non-recursive) under `project_dir` that the
/// transcript ingest should run on. Skips `.cache/` files implicitly because
/// the listing is non-recursive and Claude Code never writes `.cache` siblings
/// at the project root.
///
/// Pure (filesystem read only — no DB, no ingest). Returns an empty Vec when
/// `project_dir` does not exist or cannot be enumerated; the caller fails open.
fn enumerate_jsonl(project_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(project_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if is_transcript_path(&path) {
            out.push(path);
        }
    }
    out
}

/// Backfill every transcript currently in `project_dir` into the economy
/// `spans` table for `project_path`. Returns `(files_processed, frames_persisted)`.
///
/// Fail-open per file: a malformed transcript line emits an `eprintln!` warning
/// from `transcript::ingest` and the surrounding files still process. Opens the
/// DB once and reuses the connection for every frame across every file.
fn backfill_once(project_dir: &Path, project_path: &str) -> (usize, usize) {
    let paths = enumerate_jsonl(project_dir);
    if paths.is_empty() {
        return (0, 0);
    }
    let ctx = IngestContext::for_project(project_path);
    let conn = match economy::store::open_for(project_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "transcript-backfill: economy::store::open_for({project_path}) failed: {e}; aborting backfill"
            );
            return (0, 0);
        }
    };
    let mut files_processed = 0usize;
    let mut frames_persisted = 0usize;
    for path in &paths {
        let frames = match transcript::ingest(path, &ctx) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "transcript-backfill: transcript::ingest failed for {}: {e}; continuing",
                    path.display()
                );
                continue;
            }
        };
        files_processed += 1;
        for frame in frames {
            match economy::writer::record_api_cost(&conn, frame) {
                Ok(()) => frames_persisted += 1,
                Err(e) => {
                    eprintln!(
                        "transcript-backfill: record_api_cost failed for {}: {e}",
                        path.display()
                    );
                }
            }
        }
    }
    (files_processed, frames_persisted)
}

/// Dispatch `mustard-rt run transcript-watcher`.
///
/// `once = false` (default): runs the long-lived notify-based daemon until
/// process termination — the original behaviour, invoked by
/// [`crate::hooks::session_start`] when `MUSTARD_TRANSCRIPT_WATCH=1`.
///
/// `once = true`: resolves `~/.claude/projects/<encoded(current_dir)>/` and
/// ingests every `*.jsonl` file under it exactly once, then exits. Useful as
/// a one-shot backfill to seed the economy `spans` table from transcripts
/// captured before the daemon was wired up.
pub fn run(once: bool) {
    if once {
        run_once();
        return;
    }
    let shutdown = AtomicBool::new(false);
    watch_loop(&shutdown);
}

/// One-shot backfill of the current cwd's transcript directory. Fail-open at
/// every step: missing home, missing project dir, or per-file ingest errors
/// degrade to an `eprintln!` and a clean exit.
fn run_once() {
    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("transcript-backfill: current_dir() failed: {e}; exiting");
            return;
        }
    };
    let cwd_str = cwd.to_string_lossy().into_owned();
    let Some(home) = home_dir() else {
        eprintln!(
            "transcript-backfill: could not resolve home directory; exiting (set HOME or USERPROFILE)"
        );
        return;
    };
    let project_dir = home
        .join(".claude")
        .join("projects")
        .join(encode_cwd(&cwd_str));
    if !project_dir.exists() {
        eprintln!(
            "transcript-backfill: no transcript dir for this cwd ({}); nothing to backfill",
            project_dir.display()
        );
        return;
    }
    let (files, frames) = backfill_once(&project_dir, &cwd_str);
    println!(
        "[transcript-backfill] processed {files} files, {frames} frames from {}",
        project_dir.display()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write as _;
    use tempfile::tempdir;

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

    #[test]
    fn enumerate_jsonl_lists_only_jsonl_files() {
        let dir = tempdir().expect("tempdir");
        let p = dir.path();
        File::create(p.join("a.jsonl")).expect("a");
        File::create(p.join("b.jsonl")).expect("b");
        File::create(p.join("ignore.txt")).expect("c");
        let mut got: Vec<String> = enumerate_jsonl(p)
            .into_iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        got.sort();
        assert_eq!(got, vec!["a.jsonl".to_string(), "b.jsonl".to_string()]);
    }

    #[test]
    fn backfill_once_persists_frames_and_tolerates_malformed_lines() {
        // Layout: <tempdir>/project/{...code...} is the "project root" passed
        // to `economy::store::open_for`; the harness DB lands at
        // <project>/.claude/.harness/mustard.db. The transcript dir is a
        // sibling tempdir holding one .jsonl with 3 valid + 1 malformed line.
        let tmp = tempdir().expect("tempdir");
        let project = tmp.path().join("project");
        std::fs::create_dir_all(&project).expect("mkdir project");

        let transcripts = tmp.path().join("transcripts");
        std::fs::create_dir_all(&transcripts).expect("mkdir transcripts");
        let jsonl_path = transcripts.join("session.jsonl");
        let mut f = File::create(&jsonl_path).expect("create jsonl");
        // Three valid assistant turns with usage.
        for i in 0..3 {
            writeln!(
                f,
                "{{\"type\":\"assistant\",\"message\":{{\"model\":\"claude-opus-4-7\",\"usage\":{{\"input_tokens\":{},\"output_tokens\":{}}}}}}}",
                100 + i,
                250 + i
            )
            .expect("write valid");
        }
        // One malformed line — must emit a warn from `transcript::ingest`
        // (skipping the line) and let the surrounding scan keep going.
        writeln!(f, "{{not json at all").expect("write bad");
        drop(f);

        let project_str = project.to_string_lossy().into_owned();
        let (files, frames) = backfill_once(&transcripts, &project_str);
        assert_eq!(files, 1, "one .jsonl file enumerated");
        assert_eq!(frames, 3, "3 valid frames persisted, malformed line skipped");

        // Sanity: the harness DB was created under the project root.
        let db = project.join(".claude").join(".harness").join("mustard.db");
        assert!(db.exists(), "harness DB created at {}", db.display());
    }

    #[test]
    fn backfill_once_returns_zero_for_empty_dir() {
        let tmp = tempdir().expect("tempdir");
        let project = tmp.path().join("project");
        std::fs::create_dir_all(&project).expect("mkdir project");
        let transcripts = tmp.path().join("empty");
        std::fs::create_dir_all(&transcripts).expect("mkdir transcripts");
        let project_str = project.to_string_lossy().into_owned();
        let (files, frames) = backfill_once(&transcripts, &project_str);
        assert_eq!((files, frames), (0, 0));
    }
}
