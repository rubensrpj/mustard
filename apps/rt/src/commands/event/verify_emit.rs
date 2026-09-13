//! `verify_emit` — confirms that a spec's close reached its `spec.ndjson`.
//!
//! The close-orchestrate composite finalizes a spec in process and then asks
//! here whether the `state` with the phase `closed` really landed, instead of
//! trusting the finalize's fail-open writes blindly. The answer folds into its
//! report as `verified`.

/// The close of `spec` reached its `spec.ndjson`: a `state` with the phase
/// `closed`, recorded at `since_ms` (milliseconds since the epoch) or later.
/// The `state` time is in seconds, so the second of `since_ms` counts. `false`
/// with no event file or no such `state`. This is the check of the
/// `close-orchestrate` automatic close.
#[must_use]
pub fn closed_state_landed(cwd: &std::path::Path, spec: &str, since_ms: i64) -> bool {
    use mustard_core::domain::spec_state::SpecState as _;
    crate::shared::spec_state::DiskSpecState::new(cwd)
        .log(spec)
        .is_some_and(|log| {
            mustard_core::domain::spec_state::phase_recorded_since(&log, "closed", since_ms.div_euclid(1000))
        })
}
