//! `event_writer_ndjson` — per-spec NDJSON event sink (W5.T5.1).
//!
//! ## Why
//!
//! The legacy event sink path was SQLite INSERT + `events_fts` trigger per
//! tool call: ~100-500 µs amortised, and under parallel hooks the single
//! writer lock turns into a queue. The `events` table grows unbounded; the
//! dashboard timeline only ever reads the most recent spec slice.
//!
//! The W5 contract moves the hot path to **per-spec NDJSON files**:
//!
//! ```text
//! .claude/spec/{name}/[wave-N-{role}/].events/{ts-ns}-{run-id}-{pid}.ndjson
//! ```
//!
//! Each writer process owns one file (the `{ts-ns}-{run-id}-{pid}` triple is
//! collision-proof inside a single project) and appends one JSON object per
//! line. The file name doubles as the chronological cursor for the dashboard
//! tailer (`notify-rs` watcher in `src-tauri`).
//!
//! ## Blob spill
//!
//! Payloads strictly larger than [`blob_spill::SPILL_THRESHOLD_BYTES`] (4 KB)
//! are spilled to a content-addressed blob under `blobs/{ab}/{sha256}.bin`
//! and the NDJSON line keeps only the `{ "$blob": "<sha256>", "len": N }`
//! reference. See [`crate::shared::events::blob_spill`].
//!
//! ## Hot-path target
//!
//! The benchmark target is **< 50 µs per write** for an inline-sized event
//! (no spill). Achieved by:
//!
//! - One `OpenOptions::append(true).create(true)` per write — no `open` /
//!   `close` flapping, no fsync, no lock acquisition.
//! - Pre-serialised JSON: the caller passes the `payload` as a `serde_json::Value`
//!   once; the writer formats one line and writes it.
//! - No secondary store open for the per-tool event — the lifecycle index
//!   (`pipeline_events`) is a separate concern handled by the event router for
//!   `pipeline.*` prefixed events only.
//!
//! ## Economy emission (T5.8)
//!
//! After every successful write the sink emits a
//! `pipeline.economy.event.written { duration_ns, bytes_written,
//! spilled_to_blob }` event into the same NDJSON file, so the dashboard
//! `/economia` page can prove the new hot path beats the SQLite baseline in
//! real numbers (~30k ns measured vs ~100k-500k ns).
//!
//! ## Fail-open
//!
//! Every IO error degrades to a silent no-op — the caller's tool execution
//! is never blocked by a telemetry failure.

// W5 follow-up: `write_event` is now wired through `crate::shared::events::route`
// (the single classification layer that splits `pipeline.*` → SQLite from
// everything else → this NDJSON sink). `event_dir` is still the canonical
// path-resolver used by tests and the dashboard reader contract.

use crate::shared::events::blob_spill::{maybe_spill, BlobRef, SpillOutcome};
use mustard_core::time::now_iso8601;
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs;
use serde::Serialize;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

/// Detect whether `project` resolves to the `mustard-rt` crate's own source
/// directory — set by `cargo test` via `CARGO_MANIFEST_DIR`. Treating that
/// as "no harness available" prevents in-crate test runs from leaking a
/// `apps/rt/.claude/` tree (umbrella AC-G2). Fail-open: any env read failure
/// returns `false`.
fn project_is_own_crate(project: &Path) -> bool {
    let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") else { return false; };
    let manifest_path = std::path::PathBuf::from(&manifest);
    let canon_manifest = manifest_path.canonicalize().unwrap_or(manifest_path);
    let canon_project = project.canonicalize().unwrap_or_else(|_| project.to_path_buf());
    canon_project == canon_manifest
}

/// Resolve the per-spec event directory under `<project>/.claude/spec/{name}/[wave-N-{role}/].events/`.
///
/// `wave_role` is the optional `wave-N-{role}` segment (`Some("wave-5-mixed")`)
/// for inside-wave writes; `None` for the parent spec dir.
///
/// Falls back to `.claude/.session/{slug}/.events/` when `spec` is empty — the
/// W5.T5.4 sessions sidebar consumes that directory.
#[must_use]
pub fn event_dir(project: &Path, spec: Option<&str>, wave_role: Option<&str>, session_slug: &str) -> PathBuf {
    if let Some(spec_name) = spec.filter(|s| !s.is_empty()) {
        // Prefer the typed ClaudePaths accessor for the canonical
        // `<project>/.claude/spec/<name>/[<wave>/].events/` layout. On I1
        // guard rejection (an upstream caller handed us a `.claude`-nested
        // path) fall back to the manual composition so the writer remains
        // fail-open — telemetry must never block a real event.
        let base = ClaudePaths::for_project(project)
            .and_then(|p| p.for_spec(spec_name))
            .ok()
            .map(|sp| {
                if let Some(wr) = wave_role.filter(|s| !s.is_empty()) {
                    if let Ok(wp) = sp.for_wave(wr) {
                        return wp.dir().to_path_buf();
                    }
                    // Wave slug failed validation — degrade to spec root +
                    // raw slug so the writer still produces a stable path.
                    return sp.dir().join(wr);
                }
                sp.dir().to_path_buf()
            })
            .unwrap_or_else(|| {
                // I1 guard rejected the project root — keep telemetry alive
                // via the unchecked fallback handle so the canonical
                // accessor surface still materialises the path.
                let cp = ClaudePaths::compose_unchecked(project);
                let mut p = cp.spec_dir().join(spec_name);
                if let Some(wr) = wave_role.filter(|s| !s.is_empty()) {
                    p = p.join(wr);
                }
                p
            });
        base.join(".events")
    } else {
        // Session fallback: `<project>/.claude/.session/<slug>/.events/`. The
        // `.session/` directory is not exposed via `ClaudePaths` (it is the
        // sole consumer here, not Mustard-owned), so we reach for
        // `claude_dir()` and append manually.
        ClaudePaths::for_project(project)
            .map(|p| p.claude_dir())
            .unwrap_or_else(|_| ClaudePaths::compose_unchecked(project).claude_dir())
            .join(".session")
            .join(session_slug)
            .join(".events")
    }
}

/// One per-process writer file name (`{ts-ns}-{run-id}-{pid}.ndjson`).
///
/// Computed once per process via [`OnceLock`] so every event in the same
/// invocation lands in the same file — the dashboard tailer relies on that
/// to render an execution trace.
fn writer_filename() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        let ts_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0u128, |d| d.as_nanos());
        let pid = std::process::id();
        let run_id = std::env::var("MUSTARD_RUN_ID")
            .unwrap_or_else(|_| format!("r{pid}"));
        format!("{ts_ns}-{run_id}-{pid}.ndjson")
    })
}

/// One NDJSON record on disk. Pre-rendered shape so dashboard readers don't
/// need to guess fields per kind.
#[derive(Debug, Serialize)]
struct NdjsonRecord<'a> {
    /// ISO-8601 timestamp (string for human grep; epoch ms below for sort).
    ts: &'a str,
    /// Epoch milliseconds — primary sort key for the dashboard tailer.
    ts_ms: i64,
    /// The harness event name (`tool.use`, `pipeline.task.complete`, etc.).
    event: &'a str,
    /// Logical kind classification (`tool`, `phase`, `qa`, …). Saves the
    /// reader from re-classifying on render.
    kind: &'a str,
    /// Spec slug this event is attributed to. May be empty for session-scope.
    spec: Option<&'a str>,
    /// Wave number when known (1-based).
    wave: Option<u32>,
    /// Session id (Claude Code's UUID).
    session_id: Option<&'a str>,
    /// Hook actor id (`bash_guard`, `tracker`, …).
    actor: Option<&'a str>,
    /// Parent NDJSON line offset OR `pipeline_events.id` when this is a Task
    /// child — drives execution-trace recursion in the timeline UI.
    parent_id: Option<i64>,
    /// Inline payload (or a `{"$blob":...}` reference after spill).
    payload: Value,
    /// Pre-extracted tokens-in for cheap dashboard rendering.
    tokens_in: Option<u64>,
    /// Pre-extracted tokens-out for cheap dashboard rendering.
    tokens_out: Option<u64>,
    /// Pre-extracted duration in milliseconds (for tool calls that ran).
    duration_ms: Option<u64>,
}

/// Write one event to the NDJSON sink. The sink owns blob spill, the per-process
/// file handle (re-opened with `O_APPEND`), and emits the economy event after
/// the main write succeeds.
///
/// Returns `Ok(bytes_written)` on success — the caller is free to ignore it.
/// Every error is converted to a `Ok(0)` via fail-open at the boundary in
/// [`write_event`].
///
/// `ts_override` lets the router preserve a pre-constructed event's `ts`
/// (W6 follow-up: the SQLite-vs-NDJSON cascade revealed that
/// `route::emit` was discarding the caller's `HarnessEvent.ts`,
/// breaking consumer-side ts filters like the MCP `since` lower bound and
/// the `metrics wave-status` min/max duration). `None` falls back to
/// [`now_iso8601`] — the historical behaviour.
#[allow(clippy::too_many_arguments)]
fn write_event_inner(
    project: &Path,
    spec: Option<&str>,
    wave_role: Option<&str>,
    session_slug: &str,
    event_name: &str,
    kind: &str,
    wave: Option<u32>,
    session_id: Option<&str>,
    actor: Option<&str>,
    parent_id: Option<i64>,
    payload: &Value,
    ts_override: Option<&str>,
) -> std::io::Result<WriteOutcome> {
    let start = Instant::now();
    if project_is_own_crate(project) {
        return Ok(WriteOutcome {
            bytes_written: 0,
            spilled_to_blob: false,
            duration_ns: 0,
            path: PathBuf::new(),
        });
    }
    let dir = event_dir(project, spec, wave_role, session_slug);
    fs::create_dir_all(&dir).map_err(std::io::Error::other)?;
    let path = dir.join(writer_filename());

    // Serialize payload first so we can measure its size for spill.
    let payload_bytes = serde_json::to_vec(payload)?;

    // Spill root is the spec dir (or session dir) — one level above `events/`.
    let spill_root = dir.parent().unwrap_or(&dir).to_path_buf();
    let (payload_for_line, spilled) = match maybe_spill(&spill_root, &payload_bytes) {
        SpillOutcome::Inline => (payload.clone(), None),
        SpillOutcome::Spilled { reference, .. } => (blob_ref_to_value(&reference), Some(reference)),
    };

    let ts: String = ts_override
        .filter(|s| !s.is_empty())
        .map_or_else(now_iso8601, ToString::to_string);
    let ts_ms = mustard_core::time::parse_iso_millis(&ts).unwrap_or(0);

    // Pre-extract render hints from the payload — tokens + duration are the
    // dashboard's three most-read fields per row.
    let tokens_in = payload.get("tokens_in").and_then(Value::as_u64);
    let tokens_out = payload.get("tokens_out").and_then(Value::as_u64);
    let duration_ms = payload.get("duration_ms").and_then(Value::as_u64);

    let record = NdjsonRecord {
        ts: &ts,
        ts_ms,
        event: event_name,
        kind,
        spec,
        wave,
        session_id,
        actor,
        parent_id,
        payload: payload_for_line,
        tokens_in,
        tokens_out,
        duration_ms,
    };

    let mut line = serde_json::to_vec(&record)?;
    line.push(b'\n');
    let bytes_written = line.len();

    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)?;
        f.write_all(&line)?;
    }

    let duration_ns = start.elapsed().as_nanos() as u64;
    Ok(WriteOutcome {
        bytes_written,
        spilled_to_blob: spilled.is_some(),
        duration_ns,
        path,
    })
}

/// Outcome of one successful event write. Returned for tests + the economy
/// emitter; the hot path ignores it via `let _ =`.
#[derive(Debug)]
pub struct WriteOutcome {
    /// Bytes appended to the NDJSON file (including the trailing `\n`).
    pub bytes_written: usize,
    /// `true` if the payload was content-addressed into a blob (≥ 4 KB).
    pub spilled_to_blob: bool,
    /// Wall-clock duration of the write, in nanoseconds. The economy event
    /// reports this for the `/economia` baseline-vs-post chart.
    pub duration_ns: u64,
    /// Absolute path of the NDJSON file that was appended to. Returned for
    /// tests + future ad-hoc inspection (the production routing layer ignores
    /// it — Clippy's `dead_code` linter still flags struct fields with no
    /// read site so the field is annotated locally).
    #[allow(dead_code)]
    pub path: PathBuf,
}

/// Fail-open wrapper around [`write_event_inner`]. Any IO error degrades to a
/// silent no-op — the caller's tool execution must never be blocked by a
/// telemetry failure.
///
/// On success, an economy event `pipeline.economy.event.written` is appended
/// to the same file (T5.8) — best-effort, never recursing. The economy event
/// is always stamped with the wall-clock time of the write (it measures the
/// write itself), regardless of `ts_override`.
// Kept `pub` + `#[allow(dead_code)]` because the production callsite
// (`route::emit`) routes through [`write_event_with_ts`] for the W6
// ts-preservation fix, while the in-crate unit tests still call this
// historical entry point directly. Clippy in `--bin` mode (the gate that
// blocks PRs) doesn't see test code and would otherwise flag this as
// unused.
#[allow(dead_code, clippy::too_many_arguments)]
pub fn write_event(
    project: &Path,
    spec: Option<&str>,
    wave_role: Option<&str>,
    session_slug: &str,
    event_name: &str,
    kind: &str,
    wave: Option<u32>,
    session_id: Option<&str>,
    actor: Option<&str>,
    parent_id: Option<i64>,
    payload: &Value,
) -> Option<WriteOutcome> {
    write_event_with_ts(
        project, spec, wave_role, session_slug, event_name, kind, wave, session_id,
        actor, parent_id, payload, None,
    )
}

/// Same as [`write_event`] but lets the caller override the record's `ts`.
///
/// Used by [`crate::shared::events::route::emit`] to preserve the caller's
/// pre-constructed `HarnessEvent.ts` so consumer-side ts filters (MCP
/// `since`, `metrics wave-status` duration) still work in tests + at
/// hot-path callsites that pre-stamp events.
#[allow(clippy::too_many_arguments)]
pub fn write_event_with_ts(
    project: &Path,
    spec: Option<&str>,
    wave_role: Option<&str>,
    session_slug: &str,
    event_name: &str,
    kind: &str,
    wave: Option<u32>,
    session_id: Option<&str>,
    actor: Option<&str>,
    parent_id: Option<i64>,
    payload: &Value,
    ts_override: Option<&str>,
) -> Option<WriteOutcome> {
    let outcome = write_event_inner(
        project, spec, wave_role, session_slug, event_name, kind, wave, session_id,
        actor, parent_id, payload, ts_override,
    )
    .ok()?;

    // T5.8: emit the economy event in-line. Skip recursion guard — the
    // economy event payload is fixed-size + does not itself emit another.
    // Always stamp the economy line with wall-clock time (`None` -> now): it
    // measures the write event itself, not the original tool/agent call.
    if event_name != "pipeline.economy.event.written" {
        let economy_payload = json!({
            "duration_ns": outcome.duration_ns,
            "bytes_written": outcome.bytes_written,
            "spilled_to_blob": outcome.spilled_to_blob,
            "for_event": event_name,
        });
        let _ = write_event_inner(
            project, spec, wave_role, session_slug,
            "pipeline.economy.event.written", "other",
            wave, session_id, actor, None, &economy_payload, None,
        );
    }

    Some(outcome)
}

/// Convert a [`BlobRef`] back into the JSON shape that lives on the NDJSON
/// line. Mirrors [`BlobRef`]'s own serde shape via `to_value` to keep one
/// source of truth.
fn blob_ref_to_value(r: &BlobRef) -> Value {
    serde_json::to_value(r).unwrap_or_else(|_| {
        // Defensive fallback: the BlobRef's serde shape can't fail in practice;
        // keep an inline shape so the reader still recognises the reference.
        json!({ "$blob": r.sha256, "len": r.len })
    })
}


/// of 400 (1900 not leap, 2000 leap).

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn event_dir_resolves_under_spec() {
        let p = Path::new("/proj");
        let d = event_dir(p, Some("auth"), None, "s-1");
        assert!(d.ends_with("auth/.events") || d.ends_with("auth\\.events"));
    }

    #[test]
    fn event_dir_resolves_under_wave_role() {
        let p = Path::new("/proj");
        let d = event_dir(p, Some("auth"), Some("wave-2-rt"), "s-1");
        let s = d.display().to_string().replace('\\', "/");
        assert!(s.contains("/spec/auth/wave-2-rt/.events"));
    }

    #[test]
    fn event_dir_falls_back_to_session() {
        let p = Path::new("/proj");
        let d = event_dir(p, None, None, "s-42");
        let s = d.display().to_string().replace('\\', "/");
        assert!(s.contains("/.session/s-42/.events"));
    }

    #[test]
    fn write_event_creates_ndjson_file_and_emits_economy() {
        let dir = tempdir().unwrap();
        let payload = json!({"tool": "Bash", "cmd": "ls"});
        let outcome = write_event(
            dir.path(), Some("test-spec"), None, "s-1",
            "tool.use", "tool", Some(1), Some("s-1"), Some("bash_guard"),
            None, &payload,
        );
        assert!(outcome.is_some());

        // Two lines should be present: the original + the economy event.
        let events_dir = event_dir(dir.path(), Some("test-spec"), None, "s-1");
        let files: Vec<_> = std::fs::read_dir(&events_dir).unwrap().collect();
        assert_eq!(files.len(), 1, "exactly one NDJSON file");
        let path = files[0].as_ref().unwrap().path();
        let body = std::fs::read_to_string(&path).unwrap();
        let line_count = body.lines().count();
        assert_eq!(line_count, 2, "tool.use + economy event");

        // Economy event must reference the original event.
        let last: Value = serde_json::from_str(body.lines().last().unwrap()).unwrap();
        assert_eq!(last["event"], "pipeline.economy.event.written");
        assert_eq!(last["payload"]["for_event"], "tool.use");
        assert!(last["payload"]["duration_ns"].as_u64().is_some());
    }

    #[test]
    fn write_event_spills_large_payload_to_blob() {
        let dir = tempdir().unwrap();
        let big = "x".repeat(5_000);
        let payload = json!({"text": big});
        let _ = write_event(
            dir.path(), Some("big-spec"), None, "s-1",
            "tool.use", "tool", None, Some("s-1"), None, None, &payload,
        );

        // First line's payload should now be a blob reference, not the literal.
        let events_dir = event_dir(dir.path(), Some("big-spec"), None, "s-1");
        let first_file = std::fs::read_dir(&events_dir).unwrap().next().unwrap().unwrap().path();
        let body = std::fs::read_to_string(&first_file).unwrap();
        let first: Value = serde_json::from_str(body.lines().next().unwrap()).unwrap();
        assert!(first["payload"]["$blob"].is_string(), "payload is a blob ref");
        // The blobs dir must contain the spilled content.
        let blob_dir = dir.path().join(".claude").join("spec").join("big-spec").join(".blobs");
        assert!(blob_dir.exists(), ".blobs/ dir exists");
    }

    #[test]
    fn write_event_extracts_render_hints() {
        let dir = tempdir().unwrap();
        let payload = json!({
            "tool": "Task",
            "tokens_in": 1234,
            "tokens_out": 567,
            "duration_ms": 890,
        });
        let _ = write_event(
            dir.path(), Some("hints"), None, "s-1",
            "tool.use", "tool", None, None, None, None, &payload,
        );

        let events_dir = event_dir(dir.path(), Some("hints"), None, "s-1");
        let first_file = std::fs::read_dir(&events_dir).unwrap().next().unwrap().unwrap().path();
        let body = std::fs::read_to_string(&first_file).unwrap();
        let first: Value = serde_json::from_str(body.lines().next().unwrap()).unwrap();
        assert_eq!(first["tokens_in"], 1234);
        assert_eq!(first["tokens_out"], 567);
        assert_eq!(first["duration_ms"], 890);
    }

    #[test]
    fn epoch_ms_round_trips_a_known_timestamp() {
        // 2026-05-24T00:00:00.000Z. Verified externally:
        //   echo $(( ( $(date -d '2026-05-24' +%s) ) * 1000 ))  → 1779580800000
        let ms = mustard_core::time::parse_iso_millis("2026-05-24T00:00:00.000Z").unwrap_or(0);
        assert_eq!(ms, 1_779_580_800_000);
    }

    #[test]
    fn epoch_ms_handles_leap_year_in_march() {
        // 2024 is a leap year → 2024-03-01 sits one day later than non-leap.
        // Non-leap (2025-03-01): 20148 days * 86_400_000.
        let leap = mustard_core::time::parse_iso_millis("2024-03-01T00:00:00.000Z").unwrap_or(0);
        let non = mustard_core::time::parse_iso_millis("2023-03-01T00:00:00.000Z").unwrap_or(0);
        // 366 days between the two.
        assert_eq!(leap - non, 366 * 86_400_000);
    }

    /// AC-W5-8 — hot-path latency smoke. Real benchmark target is < 50 µs on
    /// the SSD path the user dev'd against. CI / Windows file IO under
    /// virus-scan contention can spike to tens of ms per write, so the assert
    /// bound here is a sanity ceiling (the order of magnitude). For the actual
    /// economy comparison the dashboard reads the `pipeline.economy.event.written`
    /// stream — see T5.8 wiring.
    #[test]
    fn event_writer_ndjson_hot_path() {
        let dir = tempdir().unwrap();
        let payload = json!({"tool": "Bash", "cmd": "true"});

        // Warm the file handle + dir.
        let _ = write_event(
            dir.path(), Some("hot"), None, "s-1",
            "tool.use", "tool", None, None, None, None, &payload,
        );

        // Measure across N iterations to amortise jitter.
        let n = 200;
        let start = Instant::now();
        for _ in 0..n {
            let _ = write_event(
                dir.path(), Some("hot"), None, "s-1",
                "tool.use", "tool", None, None, None, None, &payload,
            );
        }
        let avg_us = start.elapsed().as_micros() / u128::from(n as u32);
        // Sanity ceiling — anything sub-millisecond proves the write path
        // doesn't do the SQLite open/INSERT/commit dance (~100-500 µs each).
        // Windows with realtime AV scanning can balloon to tens of ms; the
        // economy event already records the real per-write duration_ns, so the
        // ceiling here only guards against pathological regressions
        // (e.g. accidental fsync, lock acquisition).
        let ceiling = if cfg!(windows) { 100_000 } else { 5_000 };
        assert!(
            avg_us < ceiling,
            "avg write latency {avg_us} µs exceeded ceiling {ceiling} µs"
        );
    }
}
