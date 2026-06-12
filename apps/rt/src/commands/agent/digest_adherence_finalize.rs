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
//! salient target. This command folds the session's events into three
//! adherence signals. Events live in ONE of two disjoint sinks chosen by
//! [`crate::shared::events::route::emit`]: the session sink
//! (`.claude/.session/<id>/.events/`) before the session→spec binding marker
//! exists, the SPEC sink (`.claude/spec/<spec>/.events/` or a
//! `wave-N-{role}/.events/` subdir) once it does — so both are read, merged
//! and filtered by the resolved session id (see [`merged_events`]):
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
use mustard_core::io::fs as mfs;
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

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

/// Read the spec sink(s) for `spec` — the parent `.claude/spec/<spec>/.events/`
/// dir plus every `wave-N-{role}/.events/` subdir — keeping ONLY events
/// stamped with the resolved `session` id (a spec accumulates events from
/// many sessions; foreign ones must not leak into this session's fold).
/// Mirrors the resolution in `shared::events::writer_ndjson::event_dir` so
/// the reader covers exactly where the router writes. Fail-open: empty spec,
/// unresolved session or missing dirs degrade to an empty list.
fn spec_events_for_session(project_dir: &str, spec: &str, session: &str) -> Vec<HarnessEvent> {
    if spec.is_empty() || session.is_empty() || session == "unknown" {
        return Vec::new();
    }
    let root = Path::new(project_dir);
    let spec_dir = ClaudePaths::for_project(root)
        .and_then(|p| p.for_spec(spec))
        .map(|sp| sp.dir().to_path_buf())
        .unwrap_or_else(|_| ClaudePaths::compose_unchecked(root).spec_dir().join(spec));
    let mut dirs: Vec<PathBuf> = vec![spec_dir.join(".events")];
    if let Ok(entries) = mfs::read_dir(&spec_dir) {
        for entry in entries {
            if entry.is_dir && entry.file_name.starts_with("wave-") {
                dirs.push(entry.path.join(".events"));
            }
        }
    }
    let mut events: Vec<HarnessEvent> = Vec::new();
    for dir in dirs {
        events.extend(
            read_harness_events_from_ndjson_dir(&dir)
                .into_iter()
                .filter(|e| e.session_id == session),
        );
    }
    events
}

/// Merge the session sink with the spec sink(s) into one ts-sorted list.
///
/// Once the session→spec binding marker exists, `route::emit` re-routes the
/// session's events (including `analyze.digest.used` and `tool.use`
/// heartbeats) to the SPEC sink — reading only `.session/<id>/.events/`
/// would fold an empty log and report a false `digestUsed=false`. The router
/// writes each event to exactly ONE sink and the two trees are disjoint, so
/// no dedup is needed; the reader sorts per-directory only, so the merged
/// vec is re-sorted (stable, lexicographic ISO-8601 `ts`) to restore the
/// global order [`summarize`]'s before-digest comparison depends on.
fn merged_events(project_dir: &str, spec: &str, session: &str) -> Vec<HarnessEvent> {
    let mut events = session_events(project_dir, session);
    events.extend(spec_events_for_session(project_dir, spec, session));
    events.sort_by(|a, b| a.ts.cmp(&b.ts));
    events
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
    // Marker-first session resolution: the session bound to `--spec` via its
    // `active-spec` marker is the one whose research this summary folds. The
    // bare `session_id()` fallback resolves newest-`.session/`-by-mtime when
    // no env var is set (none is, under the orchestrator's Bash), which
    // races against any other session touching the project between the
    // ANALYZE-time emitter and this PLAN-time reader — the field symptom was
    // a false `digestUsed: false` despite two recorded digest queries.
    let session = crate::shared::context::session_for_spec(&project_dir, spec)
        .unwrap_or_else(crate::shared::context::session_id);
    let events = merged_events(&project_dir, spec, &session);
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
        assert!(spec_events_for_session(project, "no-such-spec", "s-1").is_empty());
        assert!(spec_events_for_session(project, "", "s-1").is_empty());
        assert!(spec_events_for_session(project, "a-spec", "unknown").is_empty());
    }

    /// NDJSON record in the on-disk shape `writer_ndjson` produces (the
    /// subset `read_harness_events_from_ndjson_dir` consumes).
    fn rec(name: &str, ts: &str, session: &str, payload: Value) -> Value {
        json!({ "ts": ts, "event": name, "kind": "test", "session_id": session, "payload": payload })
    }

    fn read_rec(ts: &str, session: &str, file: &str) -> Value {
        rec("tool.use", ts, session, json!({ "tool": "Read", "target": { "file": file } }))
    }

    fn write_sink(dir: &Path, recs: &[Value]) {
        std::fs::create_dir_all(dir).expect("create sink dir");
        let body = recs.iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
        std::fs::write(dir.join("w.ndjson"), body).expect("write ndjson sink");
    }

    /// The blind-spot fix: a session already bound to a spec has its
    /// `analyze.digest.used` routed to the SPEC sink — the merged fold must
    /// see it and report `digestUsed=true` (it was a false negative before).
    #[test]
    fn digest_adherence_reads_digest_from_spec_sink_for_matching_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().to_str().expect("utf8 path");
        let spec_sink = dir.path().join(".claude").join("spec").join("tf-spec").join(".events");
        write_sink(
            &spec_sink,
            &[rec(
                "analyze.digest.used",
                "2026-06-10T00:00:01.000Z",
                "s-1",
                json!({ "queryTerms": ["x"], "miss": false }),
            )],
        );
        let (digest_used, before, total) = summarize(&merged_events(project, "tf-spec", "s-1"));
        assert!(digest_used, "spec-sink digest event must count for its own session");
        assert_eq!(before, 0);
        assert_eq!(total, 0);
    }

    /// Events routed into a `wave-N-{role}/.events/` subdir of the spec are
    /// part of the spec sink too — the reader must walk them.
    #[test]
    fn digest_adherence_reads_wave_subdir_of_spec_sink() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().to_str().expect("utf8 path");
        let wave_sink = dir
            .path()
            .join(".claude")
            .join("spec")
            .join("tf-spec")
            .join("wave-1-impl")
            .join(".events");
        write_sink(
            &wave_sink,
            &[rec(
                "analyze.digest.used",
                "2026-06-10T00:00:01.000Z",
                "s-1",
                json!({ "queryTerms": ["x"], "miss": false }),
            )],
        );
        let (digest_used, _, _) = summarize(&merged_events(project, "tf-spec", "s-1"));
        assert!(digest_used, "wave-subdir digest event must count for its own session");
    }

    /// A spec accumulates events from many sessions — another session's
    /// digest use or source reads must not leak into this session's fold.
    #[test]
    fn digest_adherence_ignores_spec_sink_events_from_other_sessions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().to_str().expect("utf8 path");
        let spec_sink = dir.path().join(".claude").join("spec").join("tf-spec").join(".events");
        write_sink(
            &spec_sink,
            &[
                rec(
                    "analyze.digest.used",
                    "2026-06-10T00:00:01.000Z",
                    "s-other",
                    json!({ "queryTerms": ["x"], "miss": false }),
                ),
                read_rec("2026-06-10T00:00:02.000Z", "s-other", "apps/rt/src/main.rs"),
            ],
        );
        let events = merged_events(project, "tf-spec", "s-1");
        assert!(events.is_empty(), "foreign-session spec-sink events must be filtered out");
        let (digest_used, before, total) = summarize(&events);
        assert!(!digest_used);
        assert_eq!(before, 0);
        assert_eq!(total, 0);
    }

    /// Merging both sinks must restore the global ts order: a source read
    /// stamped BEFORE the spec-sink digest counts as before-digest, one
    /// stamped after does not — regardless of which sink each event came from.
    #[test]
    fn digest_adherence_merges_both_sinks_in_ts_order_for_before_digest_count() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().to_str().expect("utf8 path");
        let session_sink = dir
            .path()
            .join(".claude")
            .join(".session")
            .join("s-1")
            .join(".events");
        write_sink(
            &session_sink,
            &[
                read_rec("2026-06-10T00:00:01.000Z", "s-1", "apps/rt/src/main.rs"),
                read_rec("2026-06-10T00:00:03.000Z", "s-1", "apps/rt/src/lib.rs"),
            ],
        );
        let spec_sink = dir.path().join(".claude").join("spec").join("tf-spec").join(".events");
        write_sink(
            &spec_sink,
            &[rec(
                "analyze.digest.used",
                "2026-06-10T00:00:02.000Z",
                "s-1",
                json!({ "queryTerms": ["x"], "miss": false }),
            )],
        );
        let (digest_used, before, total) = summarize(&merged_events(project, "tf-spec", "s-1"));
        assert!(digest_used);
        assert_eq!(before, 1, "only the read stamped before the spec-sink digest counts");
        assert_eq!(total, 2);
    }
}
