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




