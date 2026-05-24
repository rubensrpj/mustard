//! [`SpecView`] — the rich per-spec `ViewModel` rendered in drill-down UIs, and
//! [`SpecSummary`] — the lean sibling used in list views.
//!
//! Both share [`SpecStatus`]. Notice the absence of an `Unknown` variant:
//! when an event stream cannot produce a status (a spec with zero events, or
//! one whose only events are the `__orphan__` backfill bucket), we resolve to
//! the explicit [`SpecStatus::NoEvents`] variant. UIs render the variant
//! deliberately; they don't paint a grey "UNKNOWN" badge by accident.

use super::{Phase, Scope};
use serde::{Deserialize, Serialize};

/// The canonical lifecycle position of a spec.
///
/// Replaces the flat [`SpecStatus`] enum: where `SpecStatus` conflated *where*
/// a spec is in the pipeline with *how* it ended and *what qualifier* applies,
/// [`SpecState`] factors those three concerns apart into [`Stage`] (position),
/// [`Outcome`] (terminal disposition) and [`Flags`] (orthogonal qualifiers).
///
/// Serialized as kebab-case so it round-trips with the new on-disk header
/// (`### Stage: qa-review`).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    /// ANALYZE — exploration before planning.
    Analyze,
    /// PLAN — drafting the spec and tasks (absorbs the legacy `draft`,
    /// `planning`, `approved` statuses).
    Plan,
    /// EXECUTE — running implementation waves.
    Execute,
    /// QA / REVIEW — review and acceptance agents are running. Absorbs the two
    /// legacy `reviewing` and `qa` statuses into one stage.
    QaReview,
    /// CLOSE — archival, registry sync, banner. A terminal [`Outcome`] only
    /// makes sense paired with this stage.
    Close,
}

impl Stage {
    /// Parse a free-form stage / legacy-status fragment into a [`Stage`].
    ///
    /// Case-insensitive. Accepts the canonical kebab-case spellings plus the
    /// legacy `### Status:` / `### Phase:` synonyms documented in the
    /// `spec-lifecycle-unification` mapping table (`approved` → `Plan`,
    /// `reviewing`/`qa` → `QaReview`, etc.). Returns `None` for unknown values
    /// so callers fail open.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "analyze" => Some(Self::Analyze),
            "plan" | "planning" | "draft" | "approved" => Some(Self::Plan),
            "execute" | "implementing" | "in-progress" | "in_progress" => Some(Self::Execute),
            "qa-review" | "qa_review" | "qareview" | "review" | "reviewing" | "qa" => {
                Some(Self::QaReview)
            }
            "close" => Some(Self::Close),
            _ => None,
        }
    }
}

/// The terminal disposition of a spec — *how* it ended, independent of *where*
/// it is ([`Stage`]).
///
/// `Active` is the non-terminal sentinel: a spec that is still running carries
/// `Outcome::Active` regardless of stage. The other three only ever pair with
/// [`Stage::Close`] (enforced by [`SpecState::new`]).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// The spec is still running — not yet terminal.
    Active,
    /// Finished successfully and archived.
    Completed,
    /// Deliberately aborted after a real pipeline ran (legacy `cancelled` /
    /// `superseded`).
    Cancelled,
    /// Abandoned without a real pipeline ever running — ghost noise (legacy
    /// `abandoned` / `orphan`).
    Abandoned,
}

impl Outcome {
    /// Parse a free-form outcome / legacy-status fragment into an [`Outcome`].
    ///
    /// Case-insensitive. Recognises the canonical spellings plus the legacy
    /// terminal-status synonyms. Returns `None` for unknown values.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "active" => Some(Self::Active),
            "completed" | "closed" | "done" => Some(Self::Completed),
            "cancelled" | "canceled" | "superseded" => Some(Self::Cancelled),
            "abandoned" | "orphan" => Some(Self::Abandoned),
            _ => None,
        }
    }
}

/// Orthogonal qualifiers that can apply to a spec at any stage.
///
/// Where the legacy [`SpecStatus`] crammed `blocked`, `wave-failed` and
/// `closed-followup` into the same flat enum as the lifecycle position, these
/// become independent booleans. A spec can be `Execute` *and* `blocked`, which
/// the old enum could not express without losing the stage.
///
/// Not [`Copy`] (the only field-bearing struct in this module that can grow
/// further flags without an API break).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Flags {
    /// Pipeline is paused — explicit user intervention required.
    #[serde(default)]
    pub blocked: bool,
    /// A wave failed twice in a row in the EXECUTE stage.
    #[serde(default)]
    pub wave_failed: bool,
    /// Pipeline finished but is in the follow-up window before archival.
    #[serde(default)]
    pub followup_open: bool,
}

impl Flags {
    /// Parse a free-form flags fragment — a comma- or whitespace-separated
    /// list of flag tokens, or a single legacy status that maps to a flag
    /// (`closed-followup` → `followup_open`, `blocked`/`paused` → `blocked`,
    /// `wave-failed` → `wave_failed`).
    ///
    /// Unknown tokens are ignored (fail-open). An empty input yields the
    /// all-false default.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        let mut flags = Self::default();
        for token in raw
            .split([',', ' ', '\t'])
            .map(|t| t.trim().to_ascii_lowercase())
            .filter(|t| !t.is_empty())
        {
            match token.as_str() {
                "blocked" | "paused" => flags.blocked = true,
                "wave-failed" | "wave_failed" => flags.wave_failed = true,
                "followup_open" | "followup-open" | "closed-followup" | "closed_followup" => {
                    flags.followup_open = true;
                }
                _ => {}
            }
        }
        flags
    }
}

/// Why [`SpecState::new`] rejected a `(stage, outcome, flags)` triple.
///
/// Kept local to the model layer — [`SpecState`] is pure (no I/O), so it does
/// not borrow the crate-wide [`Error`](crate::error::Error), which is reserved
/// for side-effecting operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StateError {
    /// A terminal [`Outcome`] was paired with a non-[`Stage::Close`] stage.
    /// `Completed`/`Cancelled`/`Abandoned` only make sense at CLOSE.
    #[error("terminal outcome requires Stage::Close")]
    InvalidTerminalStage,
    /// `followup_open` was set outside the close/active context. The follow-up
    /// window only exists for a closed-but-active spec.
    #[error("followup_open requires Stage::Close + Outcome::Active")]
    InvalidFollowupContext,
    /// `wave_failed` was set outside the EXECUTE stage. Wave failures are an
    /// EXECUTE-only condition.
    #[error("wave_failed requires Stage::Execute")]
    InvalidWaveFailedContext,
}

/// The canonical lifecycle state of a spec: `(stage, outcome, flags)`.
///
/// Always construct via [`Self::new`], which rejects the illegal combinations
/// the type system alone cannot express. `flags` defaults to all-false so a
/// header carrying only `### Stage:` / `### Outcome:` deserializes cleanly.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpecState {
    /// Where the spec is in the pipeline.
    pub stage: Stage,
    /// How the spec ended (or `Active` if still running).
    pub outcome: Outcome,
    /// Orthogonal qualifiers.
    #[serde(default)]
    pub flags: Flags,
}

impl SpecState {
    /// Construct a validated [`SpecState`], rejecting the three illegal
    /// `(stage, outcome, flags)` combinations.
    ///
    /// # Errors
    ///
    /// - [`StateError::InvalidTerminalStage`] when a terminal [`Outcome`]
    ///   (`Completed`/`Cancelled`/`Abandoned`) is paired with a stage other
    ///   than [`Stage::Close`].
    /// - [`StateError::InvalidFollowupContext`] when `flags.followup_open` is
    ///   set without `Stage::Close` + `Outcome::Active`.
    /// - [`StateError::InvalidWaveFailedContext`] when `flags.wave_failed` is
    ///   set outside [`Stage::Execute`].
    pub fn new(stage: Stage, outcome: Outcome, flags: Flags) -> Result<Self, StateError> {
        if outcome != Outcome::Active && stage != Stage::Close {
            return Err(StateError::InvalidTerminalStage);
        }
        if flags.followup_open && (stage != Stage::Close || outcome != Outcome::Active) {
            return Err(StateError::InvalidFollowupContext);
        }
        if flags.wave_failed && stage != Stage::Execute {
            return Err(StateError::InvalidWaveFailedContext);
        }
        Ok(Self {
            stage,
            outcome,
            flags,
        })
    }

    /// Whether this state counts as "active" for workspace and Specs filters —
    /// any non-terminal outcome. Mirrors the legacy
    /// [`SpecStatus::is_active`](SpecStatus::is_active) classification.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.outcome == Outcome::Active
    }

    /// Whether this state is terminal (the pipeline is done, success or not).
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        self.outcome != Outcome::Active
    }

    /// Canonical kebab-case string for the SQLite `specs.status` column the
    /// dashboard reads. Mirrors the legacy [`SpecStatus::as_kebab`] mapping
    /// exactly — qualifier flags win over the stage so the dashboard keeps the
    /// `blocked` / `wave-failed` / `closed-followup` signals.
    #[must_use]
    pub const fn status_kebab(&self) -> &'static str {
        match self.outcome {
            Outcome::Completed => "completed",
            Outcome::Cancelled => "cancelled",
            Outcome::Abandoned => "abandoned",
            Outcome::Active => {
                if self.flags.blocked {
                    "blocked"
                } else if self.flags.wave_failed {
                    "wave-failed"
                } else if self.flags.followup_open {
                    "closed-followup"
                } else {
                    match self.stage {
                        Stage::Analyze | Stage::Plan => "planning",
                        Stage::Execute => "implementing",
                        Stage::QaReview => "qa",
                        Stage::Close => "closed-followup",
                    }
                }
            }
        }
    }
}

#[allow(deprecated)] // bridge over the deprecated SpecStatus during W1→W7 migration.
impl From<SpecStatus> for SpecState {
    /// Lift a legacy [`SpecStatus`] into the canonical [`SpecState`].
    fn from(status: SpecStatus) -> Self {
        let (stage, outcome, flags) = match status {
            SpecStatus::NoEvents | SpecStatus::Planning => {
                (Stage::Plan, Outcome::Active, Flags::default())
            }
            SpecStatus::Implementing => (Stage::Execute, Outcome::Active, Flags::default()),
            SpecStatus::Reviewing | SpecStatus::Qa => {
                (Stage::QaReview, Outcome::Active, Flags::default())
            }
            SpecStatus::ClosedFollowup => (
                Stage::Close,
                Outcome::Active,
                Flags { followup_open: true, ..Flags::default() },
            ),
            SpecStatus::Completed => (Stage::Close, Outcome::Completed, Flags::default()),
            SpecStatus::Cancelled => (Stage::Close, Outcome::Cancelled, Flags::default()),
            SpecStatus::Abandoned => (Stage::Close, Outcome::Abandoned, Flags::default()),
            SpecStatus::Blocked => (
                Stage::Plan,
                Outcome::Active,
                Flags { blocked: true, ..Flags::default() },
            ),
            SpecStatus::WaveFailed => (
                Stage::Execute,
                Outcome::Active,
                Flags { wave_failed: true, ..Flags::default() },
            ),
        };
        Self::new(stage, outcome, flags).unwrap_or(Self {
            stage: Stage::Plan,
            outcome: Outcome::Active,
            flags: Flags::default(),
        })
    }
}

#[allow(deprecated)] // bridge over the deprecated SpecStatus during W1→W7 migration.
impl TryFrom<SpecState> for SpecStatus {
    type Error = StateError;

    /// Project a [`SpecState`] back to the closest legacy [`SpecStatus`].
    fn try_from(state: SpecState) -> Result<Self, Self::Error> {
        let status = match state.outcome {
            Outcome::Completed => Self::Completed,
            Outcome::Cancelled => Self::Cancelled,
            Outcome::Abandoned => Self::Abandoned,
            Outcome::Active => {
                if state.flags.blocked {
                    Self::Blocked
                } else if state.flags.wave_failed {
                    Self::WaveFailed
                } else if state.flags.followup_open {
                    Self::ClosedFollowup
                } else {
                    match state.stage {
                        Stage::Analyze | Stage::Plan => Self::Planning,
                        Stage::Execute => Self::Implementing,
                        Stage::QaReview => Self::Qa,
                        Stage::Close => Self::ClosedFollowup,
                    }
                }
            }
        };
        Ok(status)
    }
}

/// The lifecycle status of a single spec.
///
/// Legacy projection target — superseded by [`SpecState`] but kept on the wire
/// for the dashboard reader during the W1→W7 migration window. The serialized
/// form is kebab-case so it round-trips with `### Status: closed-followup`.
#[deprecated(note = "Use SpecState. Removed in spec-lifecycle-unification W7.")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpecStatus {
    /// Spec exists on disk but no harness events have arrived yet.
    NoEvents,
    /// Spec is in the PLAN phase but EXECUTE has not started.
    Planning,
    /// Pipeline is actively running waves.
    Implementing,
    /// Pipeline finished EXECUTE; REVIEW agents are running.
    Reviewing,
    /// Pipeline finished REVIEW; QA agents are running.
    Qa,
    /// Pipeline finished, dir still under the follow-up window.
    ClosedFollowup,
    /// Pipeline finished and archived.
    Completed,
    /// Pipeline was cancelled before completing.
    Cancelled,
    /// Spec was abandoned without a real pipeline ever running.
    Abandoned,
    /// Pipeline is paused — explicit user intervention required.
    Blocked,
    /// Wave plan reached a wave that failed twice in a row.
    WaveFailed,
}

#[allow(deprecated)] // SpecStatus is itself deprecated; its own impl is not a misuse.
impl SpecStatus {
    /// Whether this status counts as "active" for workspace and Specs filters.
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            Self::Planning | Self::Implementing | Self::Reviewing | Self::Qa
                | Self::ClosedFollowup | Self::Blocked | Self::WaveFailed
        )
    }

    /// Whether this status is terminal (the pipeline is done, success or not).
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Abandoned)
    }

    /// Parse the on-disk header string.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "no-events" => Some(Self::NoEvents),
            "planning" | "draft" | "approved" => Some(Self::Planning),
            "implementing" | "in-progress" | "in_progress" => Some(Self::Implementing),
            "reviewing" => Some(Self::Reviewing),
            "qa" => Some(Self::Qa),
            "closed-followup" | "closed_followup" => Some(Self::ClosedFollowup),
            "completed" | "closed" => Some(Self::Completed),
            "cancelled" | "canceled" | "superseded" => Some(Self::Cancelled),
            "abandoned" | "orphan" => Some(Self::Abandoned),
            "blocked" | "paused" => Some(Self::Blocked),
            "wave-failed" | "wave_failed" => Some(Self::WaveFailed),
            _ => None,
        }
    }

    /// The canonical kebab-case wire string.
    #[must_use]
    pub const fn as_kebab(self) -> &'static str {
        match self {
            Self::NoEvents => "no-events",
            Self::Planning => "planning",
            Self::Implementing => "implementing",
            Self::Reviewing => "reviewing",
            Self::Qa => "qa",
            Self::ClosedFollowup => "closed-followup",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Abandoned => "abandoned",
            Self::Blocked => "blocked",
            Self::WaveFailed => "wave-failed",
        }
    }
}

/// Rich per-spec view — the shape the dashboard drill-down renders.
///
/// Every field is `Option<…>` or a counter; absence is encoded as `None` or
/// zero, never a literal `"unknown"` string. Counters default to zero so an
/// empty event stream produces a coherent zeroed view rather than panicking
/// or returning an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(deprecated)] // carries the derived legacy `status` field during the W1→W7 migration.
pub struct SpecView {
    /// Spec name — the directory name under `.claude/spec/`.
    pub spec: String,
    /// Canonical lifecycle state, projected from `pipeline.scope` +
    /// `pipeline.status`. The source of truth — `status` is derived from it.
    pub state: SpecState,
    /// Legacy flat status, derived from [`Self::state`] for readers not yet
    /// migrated off [`SpecStatus`].
    #[deprecated(note = "Use SpecState (field `state`). Removed in spec-lifecycle-unification W7.")]
    pub status: SpecStatus,
    /// Latest phase from `pipeline.phase` events.
    pub phase: Option<Phase>,
    /// Scope from `pipeline.scope.payload.scope`.
    pub scope: Option<Scope>,
    /// Language tag from `pipeline.scope.payload.lang` (`"pt"` or `"en"`).
    pub lang: Option<String>,
    /// Model name from `pipeline.scope.payload.model`.
    pub model: Option<String>,
    /// ISO-8601 timestamp of the first event for this spec.
    pub started_at: Option<String>,
    /// ISO-8601 timestamp of the most recent event for this spec.
    pub last_event_at: Option<String>,
    /// Milliseconds between `started_at` and `last_event_at`. None until both
    /// timestamps are present.
    pub duration_ms: Option<i64>,
    /// Index of the current wave (`pipeline.wave.complete` max + 1, capped at
    /// `total_waves`). None for non-wave-plan specs.
    pub current_wave: Option<u32>,
    /// Total number of waves declared in the wave plan. None for single-spec
    /// pipelines.
    pub total_waves: Option<u32>,
    /// Waves the pipeline has finished, in order.
    pub completed_waves: Vec<u32>,
    /// Waves recorded as failed via `pipeline.wave.failed` or a fix-loop cap.
    pub failed_waves: Vec<u32>,
    /// Number of Acceptance Criteria that returned `pass` in the latest
    /// `qa.result` event.
    pub ac_passed: u32,
    /// Total Acceptance Criteria listed in the latest `qa.result` event.
    pub ac_total: u32,
    /// Number of Acceptance Criteria that returned `fail` or `error`.
    pub ac_failed: u32,
    /// Number of distinct files touched across all `tool.use` events scoped
    /// to this spec.
    pub files_touched: u32,
    /// Count of `tool.use` events for this spec.
    pub tools_used: u32,
    /// Count of `agent.start` events for this spec.
    pub agents_dispatched: u32,
    /// `true` when `pipeline.scope.payload.is_wave_plan` was set.
    pub is_wave_plan: bool,
}

impl SpecView {
    /// Construct an empty view for `spec` — the starting point for any fold
    /// over the event stream. State defaults to the earliest meaningful
    /// position (`Plan` + `Active`) and `status` to the legacy
    /// [`SpecStatus::NoEvents`] sentinel until evidence to the contrary lands.
    #[must_use]
    #[allow(deprecated)] // seeds the derived legacy `status` field.
    pub fn empty(spec: impl Into<String>) -> Self {
        Self {
            spec: spec.into(),
            state: SpecState {
                stage: Stage::Plan,
                outcome: Outcome::Active,
                flags: Flags::default(),
            },
            status: SpecStatus::NoEvents,
            phase: None,
            scope: None,
            lang: None,
            model: None,
            started_at: None,
            last_event_at: None,
            duration_ms: None,
            current_wave: None,
            total_waves: None,
            completed_waves: Vec::new(),
            failed_waves: Vec::new(),
            ac_passed: 0,
            ac_total: 0,
            ac_failed: 0,
            files_touched: 0,
            tools_used: 0,
            agents_dispatched: 0,
            is_wave_plan: false,
        }
    }
}

/// Lean per-spec view — the shape rendered in the Specs list, the workspace
/// `spec_tracks`, and the Topbar dropdown. Drops the heavy collections
/// (`completed_waves`, etc.) so a list of 100 specs stays light.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(deprecated)] // carries the derived legacy `status` field during the W1→W7 migration.
pub struct SpecSummary {
    /// Spec name.
    pub spec: String,
    /// Canonical lifecycle state — the source of truth.
    pub state: SpecState,
    /// Legacy flat status, derived from [`Self::state`].
    #[deprecated(note = "Use SpecState (field `state`). Removed in spec-lifecycle-unification W7.")]
    pub status: SpecStatus,
    /// Latest phase.
    pub phase: Option<Phase>,
    /// Scope tag.
    pub scope: Option<Scope>,
    /// ISO-8601 of the most recent event.
    pub last_event_at: Option<String>,
    /// ISO-8601 of the first event.
    pub started_at: Option<String>,
    /// Current wave (1-based) when this is a wave plan.
    pub current_wave: Option<u32>,
    /// Total waves declared.
    pub total_waves: Option<u32>,
    /// Acceptance Criteria pass count.
    pub ac_passed: u32,
    /// Acceptance Criteria total.
    pub ac_total: u32,
    /// Number of sub-specs linked to this spec via `spec.link` events.
    ///
    /// Populated by [`SpecReader::children_of`](crate::reader::SpecReader::children_of).
    /// Defaults to `0` so older clients (and rows produced before this field
    /// existed) deserialize cleanly.
    ///
    /// [`SpecReader::children_of`]: crate::reader::SpecReader::children_of
    #[serde(default)]
    pub children_count: u32,
}

impl From<&SpecView> for SpecSummary {
    /// Project a rich view into the lean summary shape. Useful when the same
    /// projection has already paid the cost of computing the rich view.
    ///
    /// `children_count` defaults to `0` — the rich view does not carry that
    /// information; callers that want it must populate it explicitly via
    /// [`SpecReader::children_of`](crate::reader::SpecReader::children_of).
    #[allow(deprecated)] // copies the derived legacy `status` field.
    fn from(view: &SpecView) -> Self {
        Self {
            spec: view.spec.clone(),
            state: view.state.clone(),
            status: view.status,
            phase: view.phase,
            scope: view.scope,
            last_event_at: view.last_event_at.clone(),
            started_at: view.started_at.clone(),
            current_wave: view.current_wave,
            total_waves: view.total_waves,
            ac_passed: view.ac_passed,
            ac_total: view.ac_total,
            children_count: 0,
        }
    }
}

/// One child spec linked to a parent via the `spec.link` event.
///
/// Produced by [`SpecReader::children_of`](crate::reader::SpecReader::children_of)
/// — the reader folds every `spec.link` event whose payload `parent` matches
/// the requested spec, deduplicates by child name, and resolves the child's
/// status by re-reading its own event stream.
///
/// Designed for the dashboard's "Sub-specs" tab: enough metadata to render a
/// row (name, status, started/completed timestamps, free-form reason) without
/// forcing the consumer to fan out an extra `spec_summary` call per row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(deprecated)] // carries the derived legacy `status` field during the W1→W7 migration.
pub struct SpecChild {
    /// Child spec name — the slug under `.claude/spec/`.
    pub spec: String,
    /// Canonical lifecycle state of the child, resolved from its own event
    /// stream.
    pub state: SpecState,
    /// Legacy flat status of the child, derived from [`Self::state`].
    #[deprecated(note = "Use SpecState (field `state`). Removed in spec-lifecycle-unification W7.")]
    pub status: SpecStatus,
    /// ISO-8601 timestamp of the child's first event, when known.
    pub started_at: Option<String>,
    /// ISO-8601 timestamp of the child's most recent terminal event, when known.
    pub completed_at: Option<String>,
    /// Free-form reason from the `spec.link` payload (e.g. `"tactical-fix"`).
    /// The first reason wins when the same parent→child pair is linked
    /// multiple times.
    pub reason: Option<String>,
}

#[cfg(test)]
#[allow(deprecated)] // these tests intentionally exercise the deprecated SpecStatus path.
mod tests {
    use super::*;

    #[test]
    fn empty_view_starts_at_no_events_with_zero_counters() {
        let view = SpecView::empty("feature-x");
        assert_eq!(view.spec, "feature-x");
        assert_eq!(view.status, SpecStatus::NoEvents);
        assert_eq!(view.state.stage, Stage::Plan);
        assert_eq!(view.state.outcome, Outcome::Active);
        assert_eq!(view.ac_total, 0);
        assert_eq!(view.tools_used, 0);
        assert!(view.completed_waves.is_empty());
        assert!(!view.is_wave_plan);
    }

    #[test]
    fn status_parse_accepts_canonical_and_synonyms() {
        assert_eq!(SpecStatus::parse("draft"), Some(SpecStatus::Planning));
        assert_eq!(SpecStatus::parse("approved"), Some(SpecStatus::Planning));
        assert_eq!(SpecStatus::parse("in_progress"), Some(SpecStatus::Implementing));
        assert_eq!(SpecStatus::parse("CLOSED-FOLLOWUP"), Some(SpecStatus::ClosedFollowup));
        assert_eq!(SpecStatus::parse("done"), None); // explicitly NOT "completed"
    }

    #[test]
    fn status_classification_buckets_match_pipeline_lifecycle() {
        assert!(SpecStatus::Implementing.is_active());
        assert!(SpecStatus::ClosedFollowup.is_active());
        assert!(!SpecStatus::Completed.is_active());
        assert!(SpecStatus::Completed.is_terminal());
        assert!(SpecStatus::Cancelled.is_terminal());
        assert!(SpecStatus::Abandoned.is_terminal());
        assert!(!SpecStatus::Abandoned.is_active());
        assert!(!SpecStatus::Implementing.is_terminal());
    }

    #[test]
    fn parse_recognises_abandoned_and_its_orphan_synonym() {
        assert_eq!(SpecStatus::parse("abandoned"), Some(SpecStatus::Abandoned));
        assert_eq!(SpecStatus::parse("ORPHAN"), Some(SpecStatus::Abandoned));
        assert_ne!(SpecStatus::parse("abandoned"), Some(SpecStatus::Cancelled));
    }

    #[test]
    fn spec_summary_from_view_preserves_identity_fields() {
        let mut view = SpecView::empty("auth");
        view.status = SpecStatus::Implementing;
        view.ac_passed = 3;
        view.ac_total = 5;
        view.current_wave = Some(2);

        let summary: SpecSummary = (&view).into();
        assert_eq!(summary.spec, "auth");
        assert_eq!(summary.status, SpecStatus::Implementing);
        assert_eq!(summary.ac_passed, 3);
        assert_eq!(summary.current_wave, Some(2));
    }

    #[test]
    fn state_new_rejects_terminal_outcome_off_close() {
        assert_eq!(
            SpecState::new(Stage::Plan, Outcome::Completed, Flags::default()),
            Err(StateError::InvalidTerminalStage)
        );
        // Same outcome at Close is legal.
        assert!(SpecState::new(Stage::Close, Outcome::Completed, Flags::default()).is_ok());
    }

    #[test]
    fn state_new_rejects_followup_outside_close_active() {
        let followup = Flags {
            followup_open: true,
            ..Flags::default()
        };
        assert_eq!(
            SpecState::new(Stage::Execute, Outcome::Active, followup.clone()),
            Err(StateError::InvalidFollowupContext)
        );
        assert!(SpecState::new(Stage::Close, Outcome::Active, followup).is_ok());
    }

    #[test]
    fn state_new_rejects_wave_failed_outside_execute() {
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

    #[test]
    fn stage_parse_accepts_legacy_synonyms() {
        assert_eq!(Stage::parse("approved"), Some(Stage::Plan));
        assert_eq!(Stage::parse("DRAFT"), Some(Stage::Plan));
        assert_eq!(Stage::parse("implementing"), Some(Stage::Execute));
        assert_eq!(Stage::parse("reviewing"), Some(Stage::QaReview));
        assert_eq!(Stage::parse("qa"), Some(Stage::QaReview));
        assert_eq!(Stage::parse("nonsense"), None);
    }

    #[test]
    fn outcome_and_flags_parse_legacy_forms() {
        assert_eq!(Outcome::parse("superseded"), Some(Outcome::Cancelled));
        assert_eq!(Outcome::parse("orphan"), Some(Outcome::Abandoned));
        assert!(Flags::parse("blocked").blocked);
        assert!(Flags::parse("closed-followup").followup_open);
        let multi = Flags::parse("blocked, wave-failed");
        assert!(multi.blocked && multi.wave_failed);
    }

    #[test]
    fn spec_status_round_trips_through_spec_state() {
        // Every legacy status lifts into SpecState and projects back to a
        // legacy status. The round-trip is not the identity (NoEvents and
        // Reviewing collapse), but the active/terminal classification holds.
        for status in [
            SpecStatus::Planning,
            SpecStatus::Implementing,
            SpecStatus::Qa,
            SpecStatus::ClosedFollowup,
            SpecStatus::Completed,
            SpecStatus::Cancelled,
            SpecStatus::Abandoned,
            SpecStatus::Blocked,
            SpecStatus::WaveFailed,
        ] {
            let state: SpecState = status.into();
            assert_eq!(state.is_active(), status.is_active(), "active parity for {status:?}");
            assert_eq!(
                state.is_terminal(),
                status.is_terminal(),
                "terminal parity for {status:?}"
            );
            let back = SpecStatus::try_from(state).unwrap();
            // Qualifier-bearing and terminal statuses round-trip exactly.
            if matches!(
                status,
                SpecStatus::Implementing
                    | SpecStatus::ClosedFollowup
                    | SpecStatus::Completed
                    | SpecStatus::Cancelled
                    | SpecStatus::Abandoned
                    | SpecStatus::Blocked
                    | SpecStatus::WaveFailed
            ) {
                assert_eq!(back, status, "exact round-trip for {status:?}");
            }
        }
    }
}
