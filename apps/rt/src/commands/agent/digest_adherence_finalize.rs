//! `mustard-rt run digest-adherence-finalize` — fold the active session's
//! events into an `analyze.digest.summary` adherence report.
//!
//! ## What it measures
//!
//! The `/feature` and `/bugfix` ANALYZE contract is "research via the scan
//! digest, never read source by hand". Two observable traces exist for it:
//! [`crate::commands::feature`] emits `analyze.digest.used` whenever the
//! digest answers a research round, and the PostToolUse heartbeat
//! (`hooks::task::metrics_observer`) records every `tool.use` with its
//! salient target. This command folds the session's own event log
//! (`.claude/.session/<id>/.events/`) into three adherence signals:
//!
//! - `digestUsed` — did any `analyze.digest.used` land in the session?
//! - `sourceReadsBeforeDigest` — `tool.use` heartbeats for Read/Grep/Glob
//!   whose target path classifies as source
//!   ([`crate::util::source_class::is_source_file`]) and whose `ts` precedes
//!   the FIRST digest use (every source read counts when the digest never
//!   ran).
//! - `sourceReadsTotal` — all source-classified Read/Grep/Glob heartbeats.
//!
//! The summary is emitted spec-scoped (`--spec`, the slug born in PLAN) via
//! [`crate::shared::events::route::emit`] and the SAME JSON is printed to
//! stdout (fixed key order, no timestamps — the run-face byte-stability
//! contract). Fire-and-forget: telemetry only, fail-open, no event log means
//! `digestUsed=false` with zero counts — never a panic.

use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use serde_json::{json, Value};
use std::path::Path;

use crate::util::source_class::is_source_file;

/// The marker `feature::run` emits when the digest answers a research round.
const EVENT_DIGEST_USED: &str = "analyze.digest.used";

/// The adherence summary this command emits.
const EVENT_DIGEST_SUMMARY: &str = "analyze.digest.summary";

/// Tools whose `tool.use` heartbeat counts as a direct read of the repo.
const SOURCE_READ_TOOLS: [&str; 3] = ["Read", "Grep", "Glob"];

/// Extract the target path of a `tool.use` heartbeat: `target.file` for Read
/// (the metrics observer maps `file_path` there) and `target.path` for
/// Grep/Glob (their `path` input). `None` when the heartbeat carries no
/// usable target — e.g. a repo-wide Grep with no `path`.
fn tool_target_path(tool: &str, payload: &Value) -> Option<String> {
    let target = payload.get("target")?;
    let key = if tool == "Read" { "file" } else { "path" };
    target.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Pure fold over a session's (ts-sorted) events:
/// `(digest_used, source_reads_before_digest, source_reads_total)`.
///
/// "Before" compares ISO-8601 `ts` strings lexicographically (the same total
/// order the event reader sorts by); a `tool.use` stamped at exactly the
/// first digest's `ts` is NOT before it. When no `analyze.digest.used`
/// exists, every source read counts as before-digest.
fn summarize(events: &[HarnessEvent]) -> (bool, u64, u64) {
    let first_digest_ts = events
        .iter()
        .filter(|e| e.event == EVENT_DIGEST_USED)
        .map(|e| e.ts.as_str())
        .min();
    let mut before = 0u64;
    let mut total = 0u64;
    for e in events {
        if e.event != "tool.use" {
            continue;
        }
        let tool = e.payload.get("tool").and_then(Value::as_str).unwrap_or_default();
        if !SOURCE_READ_TOOLS.contains(&tool) {
            continue;
        }
        let Some(path) = tool_target_path(tool, &e.payload) else {
            continue;
        };
        if !is_source_file(&path) {
            continue;
        }
        total += 1;
        match first_digest_ts {
            Some(digest_ts) if e.ts.as_str() >= digest_ts => {}
            _ => before += 1,
        }
    }
    (first_digest_ts.is_some(), before, total)
}

/// Read the session's own event log. Empty on an unresolved session id or a
/// missing `.events/` directory — fail-open, never an error.
fn session_events(project_dir: &str, session: &str) -> Vec<HarnessEvent> {
    if session.is_empty() || session == "unknown" {
        return Vec::new();
    }
    let root = Path::new(project_dir);
    let claude_dir = ClaudePaths::for_project(root)
        .map(|p| p.claude_dir().clone())
        .unwrap_or_else(|_| ClaudePaths::compose_unchecked(root).claude_dir().clone());
    let dir = claude_dir.join(".session").join(session).join(".events");
    read_harness_events_from_ndjson_dir(&dir)
}

/// Emit the spec-scoped `analyze.digest.summary` event. Fail-open: a failed
/// write never blocks the stdout report.
fn emit_summary(project_dir: &str, session: &str, spec: &str, payload: Value) {
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: mustard_core::time::now_iso8601(),
        session_id: session.to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Cli,
            id: Some("digest-adherence-finalize".to_string()),
            actor_type: None,
        },
        event: EVENT_DIGEST_SUMMARY.to_string(),
        payload,
        spec: Some(spec.to_string()),
    };
    let _ = crate::shared::events::route::emit(project_dir, &ev);
}

/// CLI face: `mustard-rt run digest-adherence-finalize --spec <slug>`.
/// Emits the summary event and prints the same JSON. Always exits 0.
pub fn run(spec: &str) {
    let project_dir = crate::shared::context::project_dir();
    let session = crate::shared::context::session_id();
    let events = session_events(&project_dir, &session);
    let (digest_used, before, total) = summarize(&events);
    let payload = json!({
        "spec": spec,
        "digestUsed": digest_used,
        "sourceReadsBeforeDigest": before,
        "sourceReadsTotal": total,
    });
    emit_summary(&project_dir, &session, spec, payload.clone());
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into())
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(name: &str, ts: &str, payload: Value) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.to_string(),
            session_id: "s-1".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Hook,
                id: None,
                actor_type: None,
            },
            event: name.to_string(),
            payload,
            spec: None,
        }
    }

    fn read_ev(ts: &str, file: &str) -> HarnessEvent {
        ev("tool.use", ts, json!({ "tool": "Read", "target": { "file": file } }))
    }

    #[test]
    fn digest_adherence_counts_source_reads_before_first_digest() {
        let events = vec![
            read_ev("2026-06-10T00:00:01.000Z", "apps/rt/src/main.rs"),
            read_ev("2026-06-10T00:00:02.000Z", "README.md"), // support: never counts
            ev(
                "analyze.digest.used",
                "2026-06-10T00:00:03.000Z",
                json!({ "queryTerms": ["x"], "miss": false }),
            ),
            read_ev("2026-06-10T00:00:04.000Z", "apps/rt/src/lib.rs"),
        ];
        let (digest_used, before, total) = summarize(&events);
        assert!(digest_used);
        assert_eq!(before, 1, "only the pre-digest source read counts as before");
        assert_eq!(total, 2, "both source reads count in the total");
    }

    #[test]
    fn digest_adherence_without_digest_counts_every_source_read_as_before() {
        let events = vec![
            read_ev("2026-06-10T00:00:01.000Z", "src/a.ts"),
            read_ev("2026-06-10T00:00:02.000Z", "src/b.ts"),
        ];
        let (digest_used, before, total) = summarize(&events);
        assert!(!digest_used);
        assert_eq!(before, 2);
        assert_eq!(total, 2);
    }

    #[test]
    fn digest_adherence_classifies_grep_and_glob_by_target_path() {
        let events = vec![
            ev(
                "tool.use",
                "2026-06-10T00:00:01.000Z",
                json!({ "tool": "Grep", "target": { "pattern": "foo", "path": "apps/rt/src/main.rs" } }),
            ),
            // Directory path: not a source FILE — must not count.
            ev(
                "tool.use",
                "2026-06-10T00:00:02.000Z",
                json!({ "tool": "Glob", "target": { "pattern": "**/*.rs", "path": "apps/rt" } }),
            ),
            // No target at all (repo-wide Grep): must not count, must not panic.
            ev("tool.use", "2026-06-10T00:00:03.000Z", json!({ "tool": "Grep" })),
            // Non-read tool on a source path: must not count.
            ev(
                "tool.use",
                "2026-06-10T00:00:04.000Z",
                json!({ "tool": "Edit", "target": { "file": "src/a.rs" } }),
            ),
        ];
        let (digest_used, before, total) = summarize(&events);
        assert!(!digest_used);
        assert_eq!(before, 1, "only the file-targeted Grep counts");
        assert_eq!(total, 1);
    }

    #[test]
    fn digest_adherence_is_zero_on_no_events() {
        let (digest_used, before, total) = summarize(&[]);
        assert!(!digest_used);
        assert_eq!(before, 0);
        assert_eq!(total, 0);
    }

    #[test]
    fn digest_adherence_session_events_fail_open_on_missing_dirs() {
        // Unknown session id and a project with no `.claude/` both degrade to
        // an empty event list — the no-events path never panics.
        assert!(session_events("/nonexistent-mustard-xyzzy", "unknown").is_empty());
        assert!(session_events("/nonexistent-mustard-xyzzy", "s-1").is_empty());
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().to_str().expect("utf8 path");
        assert!(session_events(project, "s-never-wrote").is_empty());
    }
}
