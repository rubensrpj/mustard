// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args
)]

//! Integration tests for `mustard-rt run migrate-spec-headers` — the seven
//! scenarios from `spec-lifecycle-unification` Wave 7.
//!
//! Each test drives the binary's `run migrate-spec-headers` subcommand against
//! a controlled temp-dir fixture (never the real repo). The audit log is
//! written into the same temp dir and parsed back to assert per-file actions.
//!
//! NOTE: the subcommand prints a JSON summary to stdout and writes the full
//! audit log to `--log`. We assert on disk state + the audit log, which is the
//! durable contract.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Path to the `mustard-rt` binary built by Cargo for this test run.
fn bin() -> PathBuf {
    // `CARGO_BIN_EXE_<name>` is set by Cargo for integration tests.
    PathBuf::from(env!("CARGO_BIN_EXE_mustard-rt"))
}

/// Seed `<root>/<rel>` with `body`, creating parent dirs. `body` is written
/// byte-for-byte (callers build CRLF fixtures with `.join("\r\n")`).
fn seed(root: &Path, rel: &str, body: &str) -> PathBuf {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, body).unwrap();
    path
}

/// Run `migrate-spec-headers` against `spec_root`, writing the audit log to a
/// known path inside `tmp`. Returns the parsed audit log JSON.
fn run_migration(tmp: &Path, spec_root: &Path, apply: bool) -> Value {
    let log_path = tmp.join("migration.log.json");
    let mut cmd = Command::new(bin());
    cmd.args(["run", "migrate-spec-headers"]);
    if apply {
        cmd.arg("--apply");
    } else {
        cmd.arg("--dry-run");
    }
    cmd.arg("--root").arg(spec_root);
    cmd.arg("--log").arg(&log_path);
    let out = cmd.output().expect("spawn mustard-rt");
    assert!(
        out.status.success(),
        "migrate-spec-headers exited non-zero: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let log_text = std::fs::read_to_string(&log_path).expect("audit log written");
    serde_json::from_str(&log_text).expect("audit log is valid JSON")
}

/// Find the audit record for the file whose path ends with `suffix`.
fn record_for<'a>(log: &'a Value, suffix: &str) -> &'a Value {
    log["files"]
        .as_array()
        .expect("files array")
        .iter()
        .find(|f| {
            f["path"]
                .as_str()
                .is_some_and(|p| p.replace('\\', "/").ends_with(suffix))
        })
        .unwrap_or_else(|| panic!("no audit record ending with {suffix}\nlog: {log:#}"))
}

// ---------------------------------------------------------------------------
// Scenario 1 — happy path
// ---------------------------------------------------------------------------

#[test]
fn scenario_1_happy_path_approved_execute() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("spec");
    let path = seed(
        &root,
        "feat-x/spec.md",
        "# Feat X\n### Status: approved\n### Phase: EXECUTE\n### Lang: pt\n\nbody\n",
    );

    let log = run_migration(tmp.path(), &root, true);
    assert_eq!(log["migrated"], 1);

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("### Stage: Execute"), "{after}");
    assert!(after.contains("### Outcome: Active"), "{after}");
    assert!(after.contains("### Flags:"), "{after}");
    assert!(!after.contains("### Status:"));
    assert!(!after.contains("### Phase:"));
    assert!(after.contains("### Lang: pt"));

    let rec = record_for(&log, "feat-x/spec.md");
    assert_eq!(rec["action"], "migrated");
    assert_eq!(rec["after"]["stage"], "Execute");
    assert_eq!(rec["after"]["outcome"], "Active");
}

// ---------------------------------------------------------------------------
// Scenario 2 — dry-run writes nothing to spec files
// ---------------------------------------------------------------------------

#[test]
fn scenario_2_dry_run_leaves_files_intact() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("spec");
    let body = "# Feat\n### Status: implementing\n### Phase: EXECUTE\n\nbody\n";
    let path = seed(&root, "feat/spec.md", body);

    let log = run_migration(tmp.path(), &root, false);
    assert_eq!(log["mode"], "dry-run");
    assert_eq!(log["migrated"], 1);

    // Spec file untouched.
    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(after, body, "dry-run must not modify the spec file");

    // But the audit log IS written (the review artifact).
    assert!(tmp.path().join("migration.log.json").exists());
    let rec = record_for(&log, "feat/spec.md");
    assert_eq!(rec["action"], "migrated");
    assert_eq!(rec["after"]["stage"], "Execute");
}

// ---------------------------------------------------------------------------
// Scenario 3 — idempotence: second --apply skips everything
// ---------------------------------------------------------------------------

#[test]
fn scenario_3_idempotent_second_apply_skips() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("spec");
    let path = seed(
        &root,
        "feat/spec.md",
        "# Feat\n### Status: approved\n### Phase: PLAN\n\nbody\n",
    );

    let first = run_migration(tmp.path(), &root, true);
    assert_eq!(first["migrated"], 1);
    let after_first = std::fs::read_to_string(&path).unwrap();

    let second = run_migration(tmp.path(), &root, true);
    assert_eq!(second["migrated"], 0, "second run migrates nothing");
    assert_eq!(second["skipped_already_migrated"], 1);

    // Content stable across the second run.
    let after_second = std::fs::read_to_string(&path).unwrap();
    assert_eq!(after_first, after_second, "idempotent: content unchanged");

    let rec = record_for(&second, "feat/spec.md");
    assert_eq!(rec["action"], "skipped");
    assert_eq!(rec["reason"], "already-migrated");
}

// ---------------------------------------------------------------------------
// Scenario 4 — atomicity: the write goes through tempfile+rename
// ---------------------------------------------------------------------------

#[test]
fn scenario_4_atomic_no_partial_file_and_no_leftover_temp() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("spec");
    let path = seed(
        &root,
        "feat/spec.md",
        "# Feat\n### Status: completed\n### Phase: CLOSE\n\nbody\n",
    );

    let log = run_migration(tmp.path(), &root, true);
    assert_eq!(log["migrated"], 1);
    assert_eq!(log["errors"], 0);

    // The file is fully written (not half) and parseable as the new format.
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("### Stage: Close"));
    assert!(after.contains("### Outcome: Completed"));
    // No `.spec.md.migrate.tmp` sidecar left behind after the rename.
    let leftover = root.join("feat").join(".spec.md.migrate.tmp");
    assert!(!leftover.exists(), "tempfile must be renamed, not left behind");
}

// ---------------------------------------------------------------------------
// Scenario 5 — terminal override: cancelled + PLAN -> Close/Cancelled
// ---------------------------------------------------------------------------

#[test]
fn scenario_5_cancelled_plan_terminal_overrides() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("spec");
    let path = seed(
        &root,
        "old/spec.md",
        "# Old\n### Status: cancelled\n### Phase: PLAN\n\nbody\n",
    );

    let log = run_migration(tmp.path(), &root, true);
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("### Stage: Close"), "{after}");
    assert!(after.contains("### Outcome: Cancelled"), "{after}");

    let rec = record_for(&log, "old/spec.md");
    assert_eq!(rec["after"]["stage"], "Close");
    assert_eq!(rec["after"]["outcome"], "Cancelled");
    assert!(
        rec["inferred_stage_override"].is_string(),
        "override note expected: {rec:#}"
    );
}

// ---------------------------------------------------------------------------
// Scenario 6 — flag mapping: closed-followup -> Flags: followup_open
// ---------------------------------------------------------------------------

#[test]
fn scenario_6_closed_followup_sets_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("spec");
    let path = seed(
        &root,
        "fu/spec.md",
        "# FU\n### Status: closed-followup\n\nbody\n",
    );

    let log = run_migration(tmp.path(), &root, true);
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("### Stage: Close"), "{after}");
    assert!(after.contains("### Outcome: Active"), "{after}");
    assert!(after.contains("### Flags: followup_open"), "{after}");

    let rec = record_for(&log, "fu/spec.md");
    assert_eq!(rec["after"]["flags"][0], "followup_open");
}

// ---------------------------------------------------------------------------
// Scenario 7 — malformed: no status header -> skip, no error
// ---------------------------------------------------------------------------

#[test]
fn scenario_7_no_status_header_skips_without_error() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("spec");
    let body = "# Readme\nThis file has no status header.\n";
    let path = seed(&root, "misc/README.md", body);

    let log = run_migration(tmp.path(), &root, true);
    assert_eq!(log["errors"], 0, "no error on a headerless file");
    assert_eq!(log["migrated"], 0);
    assert_eq!(log["skipped_malformed"], 1);

    // File untouched.
    assert_eq!(std::fs::read_to_string(&path).unwrap(), body);

    let rec = record_for(&log, "misc/README.md");
    assert_eq!(rec["action"], "skipped");
    assert_eq!(rec["reason"], "no-status-header");
}

// ---------------------------------------------------------------------------
// Scenario 8 — legacy bullet-list header form (`- **Status**: ...`)
// ---------------------------------------------------------------------------

#[test]
fn scenario_8_bullet_format_header_migrates_and_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("spec");
    // The older `# Mustard 2.0 — Phase 0` specs use a bullet list (with CRLF
    // and an accented title). Only Status+Phase become Stage/Outcome/Flags;
    // the other bullets must survive verbatim.
    let body = [
        "# Mustard 2.0 — Phase 0: Runtime Compatibility Layer",
        "",
        "- **Lang**: ptbr",
        "- **Status**: completed",
        "- **Phase**: CLOSE",
        "- **Checkpoint**: 2026-05-12T18:35:00Z",
        "- **Scope**: Full",
        "- **Type**: feature",
        "",
        "Justificativa: configuração não pronta — ção ó é.",
        "",
    ]
    .join("\r\n");
    let path = seed(&root, "phase0/spec.md", &body);

    let log = run_migration(tmp.path(), &root, true);
    assert_eq!(log["errors"], 0);
    assert_eq!(log["migrated"], 1);

    let after = std::fs::read_to_string(&path).unwrap();
    // Status+Phase replaced by the three canonical lines (CRLF preserved).
    assert!(after.contains("### Stage: Close\r\n"), "{after:?}");
    assert!(after.contains("### Outcome: Completed\r\n"), "{after:?}");
    assert!(after.contains("### Flags:"), "{after}");
    // The legacy bullet Status/Phase lines are gone.
    assert!(!after.contains("- **Status**:"), "{after}");
    assert!(!after.contains("- **Phase**:"), "{after}");
    // Every other bullet survives verbatim.
    assert!(after.contains("- **Lang**: ptbr"), "{after}");
    assert!(after.contains("- **Checkpoint**: 2026-05-12T18:35:00Z"), "{after}");
    assert!(after.contains("- **Scope**: Full"), "{after}");
    assert!(after.contains("- **Type**: feature"), "{after}");
    // Accented title + body intact.
    assert!(after.contains("Mustard 2.0 — Phase 0"), "{after}");
    assert!(after.contains("configuração não pronta — ção ó é."), "{after}");

    let rec = record_for(&log, "phase0/spec.md");
    assert_eq!(rec["action"], "migrated");
    assert_eq!(rec["after"]["stage"], "Close");
    assert_eq!(rec["after"]["outcome"], "Completed");
    // `before` captures the bullet status/phase values.
    assert_eq!(rec["before"]["status"], "completed");
    assert_eq!(rec["before"]["phase"], "CLOSE");

    // Idempotent: a second --apply skips it (now has `### Stage:`).
    let second = run_migration(tmp.path(), &root, true);
    assert_eq!(second["migrated"], 0, "second run migrates nothing");
    assert_eq!(second["skipped_already_migrated"], 1);
    let after_second = std::fs::read_to_string(&path).unwrap();
    assert_eq!(after, after_second, "idempotent: content unchanged");
    let rec2 = record_for(&second, "phase0/spec.md");
    assert_eq!(rec2["reason"], "already-migrated");
}

// ---------------------------------------------------------------------------
// Extra — CRLF + accented Portuguese byte-safety end-to-end
// ---------------------------------------------------------------------------

#[test]
fn crlf_accented_fixture_is_byte_safe() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("spec");
    // Build with explicit CRLF + accents (ção, —, ó) — never a raw literal.
    let body = [
        "# Especificação — fase ó",
        "### Status: implementing",
        "### Phase: EXECUTE",
        "### Lang: pt",
        "",
        "Justificativa: configuração não está pronta — ção ó é.",
        "",
    ]
    .join("\r\n");
    let path = seed(&root, "acc/spec.md", &body);

    let log = run_migration(tmp.path(), &root, true);
    assert_eq!(log["errors"], 0);
    assert_eq!(log["migrated"], 1);

    let after = std::fs::read_to_string(&path).unwrap();
    // CRLF terminators preserved on the rewritten header lines.
    assert!(after.contains("### Stage: Execute\r\n"), "{after:?}");
    assert!(after.contains("### Outcome: Active\r\n"), "{after:?}");
    // Accented body bytes intact.
    assert!(after.contains("configuração não está pronta — ção ó é."));
    assert!(after.contains("Especificação — fase ó"));
    assert!(!after.contains("### Status:"));
    assert!(!after.contains("### Phase:"));
}

// ---------------------------------------------------------------------------
// Scenario 9 — BUG 1: idempotence probe is header-scoped. A legacy HEADER whose
// BODY documents `### Stage:` (inside a `## Tarefas` section / code fence) must
// MIGRATE, and the body's `### Stage:` line stays byte-exact.
// ---------------------------------------------------------------------------

#[test]
fn scenario_9_body_mentions_stage_still_migrates() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("spec");
    let body = [
        "# Wave 7 Migration",
        "### Status: completed",
        "### Phase: CLOSE",
        "",
        "## Tarefas",
        "",
        "Rewrites every legacy header into:",
        "",
        "```text",
        "### Stage: Execute",
        "### Outcome: Active",
        "```",
        "",
        "Inline prose also names `### Stage: Plan` as the new shape.",
        "",
    ]
    .join("\n");
    let path = seed(&root, "wave-7-migration/spec.md", &body);

    let log = run_migration(tmp.path(), &root, true);
    assert_eq!(log["errors"], 0);
    assert_eq!(log["migrated"], 1, "header is legacy → must migrate, not skip");
    assert_eq!(log["skipped_already_migrated"], 0);

    let after = std::fs::read_to_string(&path).unwrap();
    // Header migrated.
    assert!(after.contains("### Stage: Close"), "{after}");
    assert!(after.contains("### Outcome: Completed"), "{after}");
    assert!(!after.contains("### Status:"), "{after}");
    assert!(!after.contains("### Phase:"), "{after}");
    // Body documentary lines untouched.
    assert!(after.contains("### Stage: Execute"), "{after}");
    assert!(after.contains("`### Stage: Plan`"), "{after}");

    let rec = record_for(&log, "wave-7-migration/spec.md");
    assert_eq!(rec["action"], "migrated");
    assert_eq!(rec["after"]["stage"], "Close");
}

// ---------------------------------------------------------------------------
// Scenario 10 — BUG 2: combined single-line status+phase(+scope).
// ---------------------------------------------------------------------------

#[test]
fn scenario_10_combined_pipe_line_migrates() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("spec");
    let body = "# Old\n### Status: completed | Phase: CLOSE | Scope: light\n\nbody\n";
    let path = seed(&root, "combined/spec.md", body);

    let log = run_migration(tmp.path(), &root, true);
    assert_eq!(log["errors"], 0);
    assert_eq!(log["migrated"], 1);

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("### Stage: Close"), "{after}");
    assert!(after.contains("### Outcome: Completed"), "{after}");
    assert!(after.contains("### Flags:"), "{after}");
    // The extra `Scope: light` segment survives as its own header line.
    assert!(after.contains("### Scope: light"), "{after}");
    assert!(!after.contains("| Phase:"), "{after}");
    assert!(!after.contains("### Status:"), "{after}");

    // Idempotent: a second --apply skips it.
    let second = run_migration(tmp.path(), &root, true);
    assert_eq!(second["migrated"], 0);
    assert_eq!(second["skipped_already_migrated"], 1);
    let after_second = std::fs::read_to_string(&path).unwrap();
    assert_eq!(after, after_second, "idempotent: content unchanged");
}

// ---------------------------------------------------------------------------
// Scenario 11 — BUG 3: sub-plan `queued` + parenthetical phase `QA (plano)`.
// Documented choice: a queued item has NOT started → Stage: Plan, Outcome: Active.
// ---------------------------------------------------------------------------

#[test]
fn scenario_11_queued_with_parenthetical_phase() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("spec");
    let body = "# Sub-plan\n### Status: queued\n### Phase: QA (plano)\n\nbody\n";
    let path = seed(&root, "feat/qa/spec.md", body);

    let log = run_migration(tmp.path(), &root, true);
    assert_eq!(log["errors"], 0);
    assert_eq!(log["migrated"], 1);

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("### Stage: Plan"), "{after}");
    assert!(after.contains("### Outcome: Active"), "{after}");
    assert!(!after.contains("(plano)"), "parenthetical dropped: {after}");

    let rec = record_for(&log, "feat/qa/spec.md");
    assert_eq!(rec["after"]["stage"], "Plan");
    assert_eq!(rec["after"]["outcome"], "Active");
}

// ---------------------------------------------------------------------------
// Scenario 12 — BUG 3 variant: `REVIEW (plano)` token maps to QaReview.
// ---------------------------------------------------------------------------

#[test]
fn scenario_12_review_parenthetical_maps_qareview() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("spec");
    // A non-queued status so the Phase decides the stage.
    let body = "# Sub-plan\n### Status: implementing\n### Phase: REVIEW (plano)\n\nbody\n";
    let path = seed(&root, "feat/review/spec.md", body);

    let log = run_migration(tmp.path(), &root, true);
    assert_eq!(log["migrated"], 1);

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("### Stage: QaReview"), "{after}");
}
