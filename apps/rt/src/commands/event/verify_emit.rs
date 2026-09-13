//! `verify_emit` — confirms that a spec's close reached its `spec.ndjson`.
//!
//! The close-orchestrate composite finalizes a spec in process and then asks
//! here whether the `state` with the phase `closed` really landed, instead of
//! trusting the finalize's fail-open writes blindly. The answer folds into its
//! report as `verified`.

/// O fechamento da spec `spec` chegou ao `spec.ndjson` dela: um `state` com a
/// fase `closed`, gravado em `since_ms` (milissegundos desde a época) ou
/// depois. A hora do `state` vem em segundos, então conta o segundo de
/// `since_ms`. `false` sem arquivo de eventos ou sem esse `state`. É a
/// conferência do fechamento automático do `close-orchestrate`.
#[must_use]
pub fn closed_state_landed(cwd: &std::path::Path, spec: &str, since_ms: i64) -> bool {
    use mustard_core::domain::spec_state::SpecState as _;
    crate::shared::spec_state::DiskSpecState::new(cwd)
        .log(spec)
        .is_some_and(|log| {
            mustard_core::domain::spec_state::phase_recorded_since(&log, "closed", since_ms.div_euclid(1000))
        })
}
