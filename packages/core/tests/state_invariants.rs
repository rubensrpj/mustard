//! Invariant tests for the canonical [`SpecState`] model introduced in
//! `spec-lifecycle-unification` Wave 1.
//!
//! Covers three things the unit tests in `model/view/spec.rs` complement:
//!
//! 1. The [`SpecState::new`] constructor rejects the three illegal
//!    `(stage, outcome, flags)` triples (the named ACs live here).
//! 2. The legacy [`SpecStatus`] ↔ [`SpecState`] conversions are consistent
//!    (active/terminal parity, exact round-trip for the unambiguous variants).
//! 3. The header parser derives the right state from both the new
//!    (`### Stage:` / `### Outcome:` / `### Flags:`) and the legacy
//!    (`### Status:`) formats — exercised through the public reader so the
//!    whole projection path is under test.

// The conversions and the derived `status` field are deprecated but
// intentionally exercised here during the W1→W7 migration window.
#![allow(deprecated)]

use mustard_core::{
    Flags, InMemorySpecReader, Outcome, SpecReader, SpecState, SpecStatus, Stage, StateError,
};
use std::io::Write;

// ---------------------------------------------------------------------------
// Constructor invariants
// ---------------------------------------------------------------------------

/// AC-W1-4: a terminal outcome paired with a non-Close stage is rejected.
#[test]
fn rejects_completed_with_active_stage() {
    let err = SpecState::new(Stage::Plan, Outcome::Completed, Flags::default());
    assert_eq!(err, Err(StateError::InvalidTerminalStage));

    // The legal sibling (Close) constructs fine.
    assert!(SpecState::new(Stage::Close, Outcome::Completed, Flags::default()).is_ok());
}

#[test]
fn rejects_followup_open_outside_close_active() {
    let followup = Flags {
        followup_open: true,
        ..Flags::default()
    };
    assert_eq!(
        SpecState::new(Stage::QaReview, Outcome::Active, followup.clone()),
        Err(StateError::InvalidFollowupContext)
    );
    assert!(SpecState::new(Stage::Close, Outcome::Active, followup).is_ok());
}

#[test]
fn rejects_wave_failed_outside_execute() {
    let wave_failed = Flags {
        wave_failed: true,
        ..Flags::default()
    };
    assert_eq!(
        SpecState::new(Stage::Plan, Outcome::Active, wave_failed.clone()),
        Err(StateError::InvalidWaveFailedContext)
    );
    assert!(SpecState::new(Stage::Execute, Outcome::Active, wave_failed).is_ok());
}

// ---------------------------------------------------------------------------
// Conversion idempotence / parity
// ---------------------------------------------------------------------------

#[test]
fn spec_state_status_round_trip_preserves_classification() {
    let statuses = [
        SpecStatus::Planning,
        SpecStatus::Implementing,
        SpecStatus::Reviewing,
        SpecStatus::Qa,
        SpecStatus::ClosedFollowup,
        SpecStatus::Completed,
        SpecStatus::Cancelled,
        SpecStatus::Abandoned,
        SpecStatus::Blocked,
        SpecStatus::WaveFailed,
    ];
    for status in statuses {
        let state: SpecState = status.into();
        // The active/terminal split must survive the lift, since list filters
        // depend on it.
        assert_eq!(state.is_active(), status.is_active(), "active parity {status:?}");
        assert_eq!(
            state.is_terminal(),
            status.is_terminal(),
            "terminal parity {status:?}"
        );
        // The back-projection is always a legal status.
        let _back = SpecStatus::try_from(state).expect("validated state projects to a status");
    }
}

#[test]
fn terminal_and_qualifier_statuses_round_trip_exactly() {
    // These variants are unambiguous in both directions.
    for status in [
        SpecStatus::Implementing,
        SpecStatus::ClosedFollowup,
        SpecStatus::Completed,
        SpecStatus::Cancelled,
        SpecStatus::Abandoned,
        SpecStatus::Blocked,
        SpecStatus::WaveFailed,
    ] {
        let state: SpecState = status.into();
        let back = SpecStatus::try_from(state).unwrap();
        assert_eq!(back, status, "exact round-trip for {status:?}");
    }
}

// ---------------------------------------------------------------------------
// Header parsing — through the public reader
// ---------------------------------------------------------------------------

/// Write a `spec.md` with `body` under `{root}/{spec}/spec.md`.
fn write_spec_md(root: &std::path::Path, spec: &str, body: &str) {
    let dir = root.join(spec);
    std::fs::create_dir_all(&dir).expect("create spec dir");
    let mut f = std::fs::File::create(dir.join("spec.md")).expect("create spec.md");
    f.write_all(body.as_bytes()).expect("write spec.md");
}

/// AC-W1-5: legacy `### Status: approved` derives Stage::Plan + Outcome::Active.
#[test]
fn parses_legacy_approved_as_plan_active() {
    let tmp = tempfile::tempdir().unwrap();
    write_spec_md(
        tmp.path(),
        "feat",
        "# Feature\n\n### Status: approved\n### Phase: plan\n\n## Resumo\n…",
    );
    let reader = InMemorySpecReader::new();
    reader.set_spec_md_root(tmp.path());

    let view = reader.spec_view("feat").unwrap().expect("view from header");
    assert_eq!(view.state.stage, Stage::Plan);
    assert_eq!(view.state.outcome, Outcome::Active);
    assert!(!view.state.flags.blocked);
    // Legacy field stays consistent.
    assert_eq!(view.status, SpecStatus::Planning);
}

/// AC-W1-6: the new header format parses into the matching state.
#[test]
fn parses_new_format() {
    let tmp = tempfile::tempdir().unwrap();
    write_spec_md(
        tmp.path(),
        "feat",
        "# Feature\n\n### Stage: Execute\n### Outcome: Active\n### Flags: blocked\n\n## Resumo\n…",
    );
    let reader = InMemorySpecReader::new();
    reader.set_spec_md_root(tmp.path());

    let view = reader.spec_view("feat").unwrap().expect("view from new header");
    assert_eq!(view.state.stage, Stage::Execute);
    assert_eq!(view.state.outcome, Outcome::Active);
    assert!(view.state.flags.blocked);
    // The blocked flag projects back to the legacy Blocked status.
    assert_eq!(view.status, SpecStatus::Blocked);
}

/// A terminal new-format header (Close + Completed) parses and projects.
#[test]
fn parses_new_format_terminal() {
    let tmp = tempfile::tempdir().unwrap();
    write_spec_md(
        tmp.path(),
        "done",
        "# Done\n\n### Stage: Close\n### Outcome: Completed\n\n## Resumo\n…",
    );
    let reader = InMemorySpecReader::new();
    reader.set_spec_md_root(tmp.path());

    let view = reader.spec_view("done").unwrap().expect("terminal view");
    assert_eq!(view.state.stage, Stage::Close);
    assert_eq!(view.state.outcome, Outcome::Completed);
    assert!(view.state.is_terminal());
    assert_eq!(view.status, SpecStatus::Completed);
}
