//! External cost adapters — populated in W3.
//!
//! Two submodules translate external telemetry formats into the W1 record
//! types defined in [`super::model`]. Each adapter is a *pure translator*: it
//! returns a `Vec<...>` and never touches `SQLite` directly. The caller (a hook
//! or `run` subcommand in `apps/rt`) opens a connection via
//! [`super::store::open_for`] and loops the appropriate writer function:
//!
//! ```ignore
//! let records = rtk::ingest(&ctx)?;
//! for r in records {
//!     // route one NDJSON economy event per record
//! }
//! ```
//!
//! Separating translation from persistence keeps each adapter testable in
//! isolation (no temp DB, no fixtures on disk) and lets the writer layer stay
//! the only place that knows about transactions.
//!
//! ## Submodules
//!
//! - [`otel`] — parses OTLP/JSON `traces` payloads into [`SpanRecord`]s.
//! - [`rtk`] — invokes the local `rtk` binary and maps `rtk gain --json` into
//!   [`SavingsRecord`]s.
//!
//! [`SpanRecord`]: super::model::SpanRecord
//! [`SavingsRecord`]: super::model::SavingsRecord

pub mod otel;
pub mod rtk;

/// Per-call context every adapter needs to attribute its records.
///
/// `project_path` is mandatory — it is the key the writer uses to bind the
/// record to the right `SQLite` database (and the dashboard uses to scope
/// queries). `session_id` is best-effort: Claude Code provides it in the
/// `CLAUDE_SESSION_ID` env var when a session is active; adapters set it to
/// the resolved value if available, `None` otherwise.
///
/// Kept deliberately small. Future ingest signals (wave id, spec id) would
/// extend the struct rather than thread a longer argument list through every
/// adapter signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestContext {
    /// Filesystem path of the project root the ingest is attributed to.
    pub project_path: String,
    /// Optional session id (the harness `events.session_id` correlate).
    pub session_id: Option<String>,
}

impl IngestContext {
    /// Build an [`IngestContext`] with no session id resolved.
    ///
    /// Convenience constructor for the common case where the caller has the
    /// project path but the session id is genuinely unknown (e.g. a manual
    /// `mustard-rt run rtk-gain` invocation outside a Claude session).
    #[must_use]
    pub fn for_project(project_path: impl Into<String>) -> Self {
        Self {
            project_path: project_path.into(),
            session_id: None,
        }
    }
}
