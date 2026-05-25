//! Integration tests for the `spec_hygiene` SessionStart hook (Wave 5 of
//! `spec-lifecycle-unification`).
//!
//! The crate has no `lib.rs`, so each scenario drives the real binary:
//! `mustard-rt on SessionStart` with a harness-JSON stdin carrying the temp
//! project `cwd`. The hook runs inside the dispatcher exactly as it does in
//! production, shelling to its own `run verify-pipeline` / `run qa-run` faces
//! for the close-gate.
//!
//! Each scenario seeds an isolated tempdir (its own git repo + `mustard.json`
//! + `.claude/spec/<name>/spec.md`) so nothing depends on the live repo.
//!
//! Scenarios (from the spec):
//!   1. autoclose-green       — all AC `[x]`, recent commit, quiet, build green ⇒ `hygiene.autoclose`.
//!   2. build-red-skip        — same but build fails ⇒ `hygiene.skipped { blocker: build_red }`, no close.
//!   3. abandoned-suspect     — partial AC, very old last event ⇒ `hygiene.detected { abandoned_suspect }`.
//!   4. mode=off-silent       — `MUSTARD_HYGIENE_MODE=off` ⇒ no events.
//!   5. idempotence           — running twice on an already-closed spec emits nothing the 2nd time.

use mustard_core::model::event::HarnessEvent;
use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::model::event::{Actor, ActorKind, SCHEMA_VERSION};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// A temp project dir with `.claude/.harness/` created.
fn project_dir() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join(".claude").join(".harness")).expect("harness dir");
    dir
}

/// Init a git repo and make one commit touching the spec dir, so
/// `git log -1 -- <spec_dir>` yields a recent (`now`) commit timestamp.
fn git_commit_spec(project: &Path) {
    let run = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(project)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .expect("git");
    };
    run(&["init", "-q"]);
    run(&["add", "-A"]);
    run(&["commit", "-q", "-m", "seed"]);
}

/// Seed `.claude/spec/<name>/spec.md` with the given header + AC body.
fn seed_spec(project: &Path, name: &str, body: &str) {
    let spec_dir = project.join(".claude").join("spec").join(name);
    std::fs::create_dir_all(&spec_dir).expect("spec dir");
    std::fs::write(spec_dir.join("spec.md"), body).expect("write spec.md");
}

/// Write a `.claude/pipeline-config.md` with a single-row Build Command table.
///
/// This is the source `verify-pipeline` reads when `sync-detect` finds no
/// subprojects (the tempdir case) — it scans the `pipeline-config.md` table
/// for a `Build Command` column. A passing/failing `build_cmd` therefore makes
/// the hook's close-gate green/red deterministically.
fn write_pipeline_config(project: &Path, build_cmd: &str) {
    let body = format!(
        "# Pipeline Config\n\n\
         | Subproject | Build Command |\n\
         |---|---|\n\
         | app | {build_cmd} |\n"
    );
    std::fs::write(project.join(".claude").join("pipeline-config.md"), body)
        .expect("pipeline-config.md");
}

/// Append a synthetic prior event for `spec` at a chosen ISO timestamp, so the
/// hook's "last event age" reflects the scenario (quiet / stale / abandoned).
fn seed_event(project: &Path, spec: &str, ts_iso: &str) {
    let store = SqliteEventStore::for_project(project).expect("store");
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: ts_iso.to_string(),
        session_id: "seed".to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("seed".to_string()),
            actor_type: None,
        },
        event: "pipeline.scope".to_string(),
        payload: json!({ "scope": "small" }),
        spec: Some(spec.to_string()),
    };
    store.append(&ev).expect("append seed event");
}

/// Drive `mustard-rt on SessionStart` against `project`, with `MUSTARD_HYGIENE_MODE`
/// optionally overridden.
fn run_session_start(project: &Path, mode: Option<&str>) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_mustard-rt");
    let stdin_json = json!({ "cwd": project.to_string_lossy() }).to_string();
    let mut cmd = Command::new(bin);
    cmd.args(["on", "SessionStart"])
        .current_dir(project)
        .env("CLAUDE_PROJECT_DIR", project.to_string_lossy().as_ref())
        // Keep the close-gate deterministic / fast: only the build/test command
        // we set matters; never inherit a parent hygiene-mode.
        .env_remove("MUSTARD_HYGIENE_MODE");
    if let Some(m) = mode {
        cmd.env("MUSTARD_HYGIENE_MODE", m);
    }
    use std::io::Write;
    use std::process::Stdio;
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn mustard-rt");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin_json.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("wait")
}

/// Count events of `kind` attributed to `spec` across both stores.
///
/// W5 split: `pipeline.*` lives in SQLite (`pipeline_events`) and everything
/// else lives in `<project>/.claude/spec/<spec>/events/*.ndjson`. This helper
/// folds both sources so legacy callers can assert counts without caring which
/// store a kind landed in.
fn count_events(project: &Path, spec: &str, kind: &str) -> usize {
    let store = SqliteEventStore::for_project(project).expect("store");
    let sqlite_count = store
        .query(Some(spec))
        .expect("query")
        .iter()
        .filter(|e| e.event == kind)
        .count();
    let events_dir = project.join(".claude").join("spec").join(spec).join("events");
    let ndjson_count = mustard_core::projection::read_harness_events_from_ndjson_dir(
        &events_dir,
    )
    .iter()
    .filter(|e| e.event == kind)
    .count();
    sqlite_count + ndjson_count
}

/// `now - hours` as an ISO-8601 string the hook can parse.
fn hours_ago_iso(hours: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("epoch");
    let secs = now.as_secs().saturating_sub(hours * 3600);
    iso_from_epoch_secs(secs)
}

/// `now - days` as an ISO-8601 string.
fn days_ago_iso(days: u64) -> String {
    hours_ago_iso(days * 24)
}

/// Format epoch seconds as `YYYY-MM-DDThh:mm:ss.000Z` (Howard Hinnant civil).
fn iso_from_epoch_secs(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}.000Z")
}

// ---------------------------------------------------------------------------
// Scenario 1 — autoclose-green
// ---------------------------------------------------------------------------

#[test]
fn scenario_1_autoclose_green() {
    let tmp = project_dir();
    let project = tmp.path();
    let spec = "auto-green";

    seed_spec(
        project,
        spec,
        "# Auto Green\n### Outcome: Active\n\n## Acceptance Criteria\n\
         - [x] AC-1: ok — Command: `node -e \"process.exit(0)\"`\n",
    );
    // A passing build so verify-pipeline is green.
    write_pipeline_config(project, "node -e \"process.exit(0)\"");
    // Commit the spec so the last commit is recent (now).
    git_commit_spec(project);
    // Last event 8h ago → quiet enough (≥6h) to be a candidate.
    seed_event(project, spec, &hours_ago_iso(8));

    let out = run_session_start(project, None);
    assert!(
        out.status.success(),
        "SessionStart must exit 0 (fail-open). stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert_eq!(
        count_events(project, spec, "hygiene.autoclose"),
        1,
        "candidate with green gate must auto-close"
    );
    assert_eq!(
        count_events(project, spec, "pipeline.outcome"),
        1,
        "auto-close must emit pipeline.outcome: completed"
    );
    // The header was rewritten to terminal.
    let after = std::fs::read_to_string(
        project.join(".claude").join("spec").join(spec).join("spec.md"),
    )
    .expect("read spec");
    assert!(
        after.contains("### Outcome: Completed"),
        "header should be rewritten to Completed; got:\n{after}"
    );
}

// ---------------------------------------------------------------------------
// Scenario 2 — build-red-skip
// ---------------------------------------------------------------------------

#[test]
fn scenario_2_build_red_skips() {
    let tmp = project_dir();
    let project = tmp.path();
    let spec = "build-red";

    seed_spec(
        project,
        spec,
        "# Build Red\n### Outcome: Active\n\n## Acceptance Criteria\n\
         - [x] AC-1: ok — Command: `node -e \"process.exit(0)\"`\n",
    );
    // A failing build command ⇒ verify-pipeline red.
    write_pipeline_config(project, "node -e \"process.exit(1)\"");
    git_commit_spec(project);
    seed_event(project, spec, &hours_ago_iso(8));

    let out = run_session_start(project, None);
    assert!(out.status.success(), "stderr:\n{}", String::from_utf8_lossy(&out.stderr));

    assert_eq!(
        count_events(project, spec, "hygiene.autoclose"),
        0,
        "a red build must NOT auto-close"
    );
    assert_eq!(
        count_events(project, spec, "pipeline.outcome"),
        0,
        "a red build must NOT emit pipeline.outcome"
    );
    assert_eq!(
        count_events(project, spec, "hygiene.skipped"),
        1,
        "a red build must emit hygiene.skipped"
    );
    // The blocker is build_red, and the spec is still active.
    // W5/W6: `hygiene.*` is a non-pipeline family routed to per-spec NDJSON
    // (see `event_route::classify_kind`); read it from the NDJSON sink, not
    // from `SqliteEventStore::query` (which only sees `pipeline_events`).
    let events_dir = project.join(".claude").join("spec").join(spec).join("events");
    let skipped: Vec<_> = mustard_core::projection::read_harness_events_from_ndjson_dir(
        &events_dir,
    )
    .into_iter()
    .filter(|e| e.event == "hygiene.skipped")
    .collect();
    assert_eq!(skipped[0].payload["blocker"], Value::String("build_red".into()));
    let after = std::fs::read_to_string(
        project.join(".claude").join("spec").join(spec).join("spec.md"),
    )
    .expect("read spec");
    assert!(after.contains("### Outcome: Active"), "spec must stay active");
}

// ---------------------------------------------------------------------------
// Scenario 3 — abandoned-suspect detect
// ---------------------------------------------------------------------------

#[test]
fn scenario_3_abandoned_suspect_detect() {
    let tmp = project_dir();
    let project = tmp.path();
    let spec = "abandoned";

    seed_spec(
        project,
        spec,
        "# Abandoned\n### Outcome: Active\n\n## Acceptance Criteria\n\
         - [x] AC-1: done\n- [ ] AC-2: still open\n",
    );
    // Last event 60 days ago, partial AC ⇒ abandoned-suspect.
    seed_event(project, spec, &days_ago_iso(60));

    let out = run_session_start(project, None);
    assert!(out.status.success(), "stderr:\n{}", String::from_utf8_lossy(&out.stderr));

    // W5/W6: `hygiene.*` is non-pipeline → per-spec NDJSON. Reading via
    // `SqliteEventStore::query` would always return 0 for this family.
    let events_dir = project.join(".claude").join("spec").join(spec).join("events");
    let detected: Vec<_> = mustard_core::projection::read_harness_events_from_ndjson_dir(
        &events_dir,
    )
    .into_iter()
    .filter(|e| e.event == "hygiene.detected")
    .collect();
    assert_eq!(detected.len(), 1, "must emit exactly one hygiene.detected");
    assert_eq!(
        detected[0].payload["reason"],
        Value::String("abandoned_suspect".into())
    );
    // Detect-only: no auto-close.
    assert_eq!(count_events(project, spec, "hygiene.autoclose"), 0);
}

// ---------------------------------------------------------------------------
// Scenario 4 — mode=off silent
// ---------------------------------------------------------------------------

#[test]
fn scenario_4_mode_off_is_silent() {
    let tmp = project_dir();
    let project = tmp.path();
    let spec = "off-spec";

    seed_spec(
        project,
        spec,
        "# Off\n### Outcome: Active\n\n## Acceptance Criteria\n- [x] AC-1: ok\n",
    );
    write_pipeline_config(project, "node -e \"process.exit(0)\"");
    git_commit_spec(project);
    seed_event(project, spec, &hours_ago_iso(8));

    let out = run_session_start(project, Some("off"));
    assert!(out.status.success(), "stderr:\n{}", String::from_utf8_lossy(&out.stderr));

    assert_eq!(count_events(project, spec, "hygiene.autoclose"), 0);
    assert_eq!(count_events(project, spec, "hygiene.detected"), 0);
    assert_eq!(count_events(project, spec, "hygiene.skipped"), 0);
}

// ---------------------------------------------------------------------------
// Scenario 6 — CRLF + accented Portuguese must not crash the session
// ---------------------------------------------------------------------------

/// Regression for the byte-offset drift panic: on Windows CRLF specs with
/// accented Portuguese (`ó`, `—`, `ção`) the old `ac_section` sliced inside a
/// multibyte char and panicked, violating the fail-open contract. Drive the
/// real `mustard-rt on SessionStart` against such a spec and require exit 0.
#[test]
fn scenario_6_crlf_accents_does_not_panic() {
    let tmp = project_dir();
    let project = tmp.path();
    let spec = "crlf-accents";

    // Build the body with explicit `\r\n` — a raw literal may be saved as LF.
    let body = [
        "# Mustard 2.0 — Phase 3: MCP Memory Server",
        "### Outcome: Active",
        "",
        "Justificativa: a implementação não está pronta — revisão pendente.",
        "",
        "## Critérios de Aceitação",
        "- [x] AC-1: configuração validada — ó é ção — Command: `node -e \"process.exit(0)\"`",
        "- [ ] AC-2: ainda em revisão",
        "",
    ]
    .join("\r\n");

    seed_spec(project, spec, &body);
    write_pipeline_config(project, "node -e \"process.exit(0)\"");
    git_commit_spec(project);
    seed_event(project, spec, &hours_ago_iso(8));

    let out = run_session_start(project, None);
    assert!(
        out.status.success(),
        "SessionStart must exit 0 (fail-open) on CRLF+accented specs. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("char boundary"),
        "no char-boundary panic may appear on stderr. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ---------------------------------------------------------------------------
// Scenario 5 — idempotence on an already-closed spec
// ---------------------------------------------------------------------------

#[test]
fn scenario_5_idempotent_on_closed_spec() {
    let tmp = project_dir();
    let project = tmp.path();
    let spec = "auto-twice";

    seed_spec(
        project,
        spec,
        "# Twice\n### Outcome: Active\n\n## Acceptance Criteria\n\
         - [x] AC-1: ok — Command: `node -e \"process.exit(0)\"`\n",
    );
    write_pipeline_config(project, "node -e \"process.exit(0)\"");
    git_commit_spec(project);
    seed_event(project, spec, &hours_ago_iso(8));

    // First run closes the spec.
    let out1 = run_session_start(project, None);
    assert!(out1.status.success());
    assert_eq!(count_events(project, spec, "hygiene.autoclose"), 1, "first run closes");

    // Second run: the header is now terminal (Outcome: Completed) ⇒ the spec is
    // no longer active ⇒ the hook emits NOTHING new.
    let out2 = run_session_start(project, None);
    assert!(out2.status.success());
    assert_eq!(
        count_events(project, spec, "hygiene.autoclose"),
        1,
        "second run on a closed spec must NOT emit another autoclose"
    );
    assert_eq!(
        count_events(project, spec, "pipeline.outcome"),
        1,
        "no duplicate pipeline.outcome on the second run"
    );
}
