#![allow(clippy::unwrap_used)]
//! Invariant tests for the canonical [`SpecState`] model introduced in
//! `spec-lifecycle-unification` Wave 1.
//!
//! Covers two things the unit tests in `model/view/spec.rs` complement:
//!
//! 1. The [`SpecState::new`] constructor rejects the three illegal
//!    `(stage, outcome, flags)` triples (the named ACs live here).
//! 2. The header parser derives the right state from both the new
//!    (`### Stage:` / `### Outcome:` / `### Flags:`) and the legacy
//!    (`### Status:`) formats — exercised through the public projection
//!    [`project_spec_view_with_header`] so the whole header → view path is
//!    under test.
//!
//! W8A-4 (no-sqlite Wave 8) deleted the `mustard_core::reader` layer
//! (`SpecReader` trait + `InMemorySpecReader` + `SqliteSpecReader`). The
//! header-parsing assertions now call the pure projection directly with the
//! on-disk `spec.md` path, exercising the same code path production readers
//! consume.

use mustard_core::{Flags, Outcome, SpecState, Stage, StateError};
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
// Classification invariants
// ---------------------------------------------------------------------------

/// The active/terminal split is a pure function of the outcome — list filters
/// depend on it staying that way regardless of stage or flags.
#[test]
fn active_terminal_split_follows_the_outcome() {
    let active = SpecState::new(Stage::Execute, Outcome::Active, Flags::default()).unwrap();
    assert!(active.is_active());
    assert!(!active.is_terminal());

    // A qualifier flag never flips the classification.
    let blocked = SpecState::new(
        Stage::Execute,
        Outcome::Active,
        Flags {
            blocked: true,
            ..Flags::default()
        },
    )
    .unwrap();
    assert!(blocked.is_active());

    let followup = SpecState::new(
        Stage::Close,
        Outcome::Active,
        Flags {
            followup_open: true,
            ..Flags::default()
        },
    )
    .unwrap();
    assert!(followup.is_active(), "the follow-up window is still active");

    for outcome in [
        Outcome::Completed,
        Outcome::Cancelled,
        Outcome::Abandoned,
        Outcome::Superseded,
        Outcome::Absorbed,
    ] {
        let state = SpecState::new(Stage::Close, outcome, Flags::default()).unwrap();
        assert!(state.is_terminal(), "terminal for {outcome:?}");
        assert!(!state.is_active(), "not active for {outcome:?}");
    }
}

// ---------------------------------------------------------------------------
// Header parsing — through the public projection
// ---------------------------------------------------------------------------

/// Write a `spec.md` with `body` under `{root}/{spec}/spec.md` and return its
/// full path.
fn write_spec_md(root: &std::path::Path, spec: &str, body: &str) -> std::path::PathBuf {
    let dir = root.join(spec);
    std::fs::create_dir_all(&dir).expect("create spec dir");
    let path = dir.join("spec.md");
    let mut f = std::fs::File::create(&path).expect("create spec.md");
    f.write_all(body.as_bytes()).expect("write spec.md");
    path
}

/// AC-W1-5: legacy `### Status: approved` derives Stage::Plan + Outcome::Active.
#[test]
fn parses_legacy_approved_as_plan_active() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_spec_md(
        tmp.path(),
        "feat",
        "# Feature\n\n### Status: approved\n### Phase: plan\n\n## Resumo\n…",
    );

    // Empty event slice + path → projection falls back to header parse.
    let view =
        mustard_core::view::projection::project_spec_view_with_header("feat", &[], Some(path.as_path()));
    assert_eq!(view.state.stage, Stage::Plan);
    assert_eq!(view.state.outcome, Outcome::Active);
    assert!(!view.state.flags.blocked);
}

/// AC-W1-6: the new header format parses into the matching state.
#[test]
fn parses_new_format() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_spec_md(
        tmp.path(),
        "feat",
        "# Feature\n\n### Stage: Execute\n### Outcome: Active\n### Flags: blocked\n\n## Resumo\n…",
    );

    let view =
        mustard_core::view::projection::project_spec_view_with_header("feat", &[], Some(path.as_path()));
    assert_eq!(view.state.stage, Stage::Execute);
    assert_eq!(view.state.outcome, Outcome::Active);
    assert!(view.state.flags.blocked);
}

/// A terminal new-format header (Close + Completed) parses and projects.
#[test]
fn parses_new_format_terminal() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_spec_md(
        tmp.path(),
        "done",
        "# Done\n\n### Stage: Close\n### Outcome: Completed\n\n## Resumo\n…",
    );

    let view =
        mustard_core::view::projection::project_spec_view_with_header("done", &[], Some(path.as_path()));
    assert_eq!(view.state.stage, Stage::Close);
    assert_eq!(view.state.outcome, Outcome::Completed);
    assert!(view.state.is_terminal());
}
