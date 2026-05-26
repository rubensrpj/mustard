//! The SQLite-backed harness store — `.claude/.harness/mustard.db`.
//!
//! [`SqliteEventStore`] is the single store the harness reads from and writes
//! to. It persists events in one `SQLite` database opened in **WAL mode**: WAL
//! lets a writer and any number of readers proceed concurrently, and a
//! per-connection `busy_timeout` makes a contended write *wait* instead of
//! erroring — the property the harness needs when several hooks fire in
//! parallel.
//!
//! ## W5 layout (2026-05-24-mustard-unification)
//!
//! The high-volume `events` table is gone. Tool / agent / qa / scope events now
//! live in per-spec NDJSON files under `.claude/spec/{name}/events/*.ndjson`
//! (written by `apps/rt/src/run/event_writer_ndjson.rs`). What remains in
//! SQLite is the **lifecycle index** — `pipeline_events` carries
//! `pipeline.scope`, `pipeline.status`, `pipeline.phase`, the
//! `pipeline.task.*` pair, `pipeline.wave.*` pair, and `pipeline.complete`.
//!
//! The legacy `replay`/`query`/`distinct_specs` methods are kept as
//! **compatibility shims**: they read from `pipeline_events` instead of the
//! retired `events` table. Tool-level event consumers must read NDJSON files
//! directly (see [`crate::projection::timeline`]).
//!
//! Every method is **fail-open**: it returns [`Result`] and never panics. A
//! database that cannot be opened, a query that fails, or a row that cannot be
//! decoded degrades to an [`Err`] (or, for replay-style reads, an empty `Vec`)
//! that a hook is free to ignore — telemetry is never load-bearing.

use crate::error::{Error, Result};
use crate::model::event::{
    Actor, ActorKind, HarnessEvent, PipelineAmendOpenPayload, PipelineTaskCompletePayload,
    SCHEMA_VERSION, EVENT_PIPELINE_TASK_COMPLETE,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Directory name of the harness store, under `.claude/`.
const HARNESS_DIR: &str = ".harness";

/// Default file name of the `SQLite` database.
const DB_FILE: &str = "mustard.db";

/// Environment variable overriding the resolved database path.
///
/// When set, [`SqliteEventStore::for_project`] uses its value verbatim instead
/// of the `{project}/.claude/.harness/mustard.db` default.
const DB_PATH_ENV: &str = "MUSTARD_DB_PATH";

/// How long a contended write waits for the lock before failing, in
/// milliseconds. Generous on purpose: a harness `INSERT` is sub-millisecond,
/// so five seconds covers any realistic pile-up of parallel hooks.
const BUSY_TIMEOUT_MS: u32 = 5_000;

/// The idempotent schema, sourced verbatim from the legacy TypeScript store
/// (`apps/cli/src/runtime/schema.sql`). Every `CREATE` is `IF NOT EXISTS`, so
/// applying it on every open is safe.
const SCHEMA_SQL: &str = include_str!("sqlite_schema.sql");

/// One spec row from the denormalized `specs` projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecRow {
    /// Spec name — the directory name under `.claude/spec/`.
    pub name: String,
    /// Lifecycle status (`active`, `closed`, …); `None` if unset.
    pub status: Option<String>,
    /// Current pipeline phase; `None` if unset.
    pub phase: Option<String>,
    /// ISO-8601 start timestamp; `None` if unset.
    pub started_at: Option<String>,
    /// ISO-8601 completion timestamp; `None` while still running.
    pub completed_at: Option<String>,
    /// Raw `affected_files` column (a JSON array string in the legacy schema).
    pub affected_files: Option<String>,
}

/// One row from the `metrics_projection` table — per-spec pipeline metrics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricsRow {
    /// Spec the metrics belong to.
    pub spec: String,
    /// Number of model API calls recorded for the spec.
    pub api_calls: Option<i64>,
    /// Number of retry attempts.
    pub retries: Option<i64>,
    /// First-attempt pass count.
    pub pass1: Option<i64>,
    /// Raw `tool_breakdown` column (a JSON object string).
    pub tool_breakdown: Option<String>,
    /// Raw `dispatch_failures_by_phase` column (a JSON object string).
    pub dispatch_failures_by_phase: Option<String>,
    /// Number of agents dispatched.
    pub agent_count: Option<i64>,
    /// ISO-8601 timestamp of the last projection update.
    pub updated_at: Option<String>,
}

/// One row from the `run_usage` projection — a single execution's usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    /// Trace the run belongs to.
    pub trace_id: Option<String>,
    /// Run identifier (the `span_id` primary key).
    pub span_id: String,
    /// Parent run, when this is a child run.
    pub parent_span_id: Option<String>,
    /// Human-readable run name.
    pub name: Option<String>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: Option<i64>,
    /// Spec the run is attributed to.
    pub spec: Option<String>,
    /// Pipeline phase the run occurred in.
    pub phase: Option<String>,
    /// Model in use during the run.
    pub model: Option<String>,
    /// Input token count, when the run carried token usage.
    pub input_tokens: Option<i64>,
    /// Output token count, when the run carried token usage.
    pub output_tokens: Option<i64>,
    /// Whether the run ended in an error.
    pub is_error: bool,
}

/// One knowledge entry, decoded from the `knowledge` table.
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeRow {
    /// Stable knowledge id (a `TEXT` key in the schema).
    pub id: String,
    /// Entry kind (`pattern`, `convention`, `entity`, …).
    pub kind: Option<String>,
    /// Short name of the pattern / convention.
    pub name: Option<String>,
    /// Free-form description.
    pub description: Option<String>,
    /// Confidence score in `[0, 1]`.
    pub confidence: Option<f64>,
}

/// One row from the `pipeline_amend_window` table.
///
/// Returned by [`SqliteEventStore::amend_window_for_session`].
#[derive(Debug, Clone, PartialEq)]
pub struct AmendWindow {
    /// Spec identifier that owns this window.
    pub spec_id: String,
    /// Session identifier this window belongs to.
    pub session_id: String,
    /// ISO-8601 timestamp at which the pipeline was originally closed.
    pub closed_at: String,
    /// File paths that form the allowed edit set for this amendment window.
    pub pipeline_file_set: Vec<String>,
    /// Subproject identifiers active during the original pipeline run.
    pub subprojects: Vec<String>,
    /// Window lifecycle status: `"open"`, `"amending"`, `"completed"`, etc.
    pub status: String,
    /// ISO-8601 timestamp of the most recent activity in this window.
    pub last_activity_at: Option<String>,
    /// ISO-8601 timestamp at which the build turned green.
    pub build_verde_at: Option<String>,
    /// Paths edited outside `pipeline_file_set` (drift candidates).
    pub drift_unrelated_paths: Vec<String>,
    /// Whether at least one drift event was emitted for this window.
    pub drift_emitted: bool,
}

/// SQLite-backed [`EventSink`](super::event_store::EventSink) over a single
/// `mustard.db` file.
///
/// Construct it with [`SqliteEventStore::new`] from an explicit path, or with
/// [`SqliteEventStore::for_project`] from a project root. Opening applies the
/// schema and switches the connection to WAL mode; the connection is held for
/// the lifetime of the store. It is **not** `Clone` — a [`Connection`] is a
/// single owned handle; share it behind an `Arc`/`Mutex` if a consumer needs
/// to.
#[derive(Debug)]
pub struct SqliteEventStore {
    /// The open `SQLite` connection. WAL mode + `busy_timeout` are set on open.
    conn: Connection,
    /// The database path, kept for diagnostics ([`SqliteEventStore::path`]).
    path: PathBuf,
}

impl SqliteEventStore {
    /// Open (creating if absent) a store backed by the database at `path`.
    ///
    /// On open the connection is switched to WAL journal mode, given a
    /// [`BUSY_TIMEOUT_MS`] busy timeout and `synchronous = NORMAL`. The
    /// idempotent [`SCHEMA_SQL`] and the [`migrations`](super::migrations)
    /// ladder are applied **only when the database is not already at the latest
    /// schema version** — gated by `SQLite`'s `PRAGMA user_version`, a header
    /// integer that costs nothing to read (no table access). The parent
    /// directory is created if it does not exist.
    ///
    /// # Why the fast-path
    ///
    /// The harness spawns a fresh process per hook event, so this constructor
    /// runs on every tool use. Running the full DDL plus the migration ladder
    /// every time was the dominant fixed cost. `user_version` lets a
    /// steady-state open skip both: it is `0` on a brand-new file and is
    /// stamped to [`LATEST_VERSION`](super::migrations::LATEST_VERSION) once the
    /// schema is materialized, so the gate is correct without a table read.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] if the database cannot be opened or the
    /// schema cannot be applied, and [`Error::Io`] if the parent directory
    /// cannot be created.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                crate::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(&path)?;
        // Per-connection pragmas — always set, they do not persist with the
        // database file. WAL: concurrent readers + a single writer, the harness
        // access shape. `query_row` because `journal_mode` returns the mode it
        // settled on.
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        // synchronous = NORMAL is safe under WAL (a crash can lose the tail of
        // the last transaction, never corrupt the database) and trades a full
        // fsync per commit for far fewer — telemetry writes are not durability
        // critical, so this is a clear win for the per-hook hot path.
        conn.execute_batch("PRAGMA synchronous = NORMAL")?;
        // A contended write waits up to BUSY_TIMEOUT_MS instead of erroring —
        // parallel hooks must not lose events to a transient lock.
        conn.busy_timeout(std::time::Duration::from_millis(u64::from(
            BUSY_TIMEOUT_MS,
        )))?;

        // Fast-path: skip DDL + migrations when the database is already
        // materialized at the latest version. `user_version` is a 32-bit
        // header field, so reading it touches no table.
        let user_version: i64 =
            conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if user_version != i64::from(super::migrations::LATEST_VERSION) {
            conn.execute_batch(SCHEMA_SQL)?;
            // Apply versioned data migrations after the shape is in place. The
            // v2 step backfills `events.spec` rows left NULL by the six
            // emitters identified in the 2026-05-20 attribution audit so
            // projections that filter by spec stop dropping those events. See
            // `migrations.rs` for the full ladder.
            let latest = super::migrations::apply(&conn)?;
            // Stamp the header so the next open takes the fast-path. Pragmas
            // do not accept bound parameters, so the value is formatted in —
            // it is a crate constant, never user input.
            conn.execute_batch(&format!("PRAGMA user_version = {latest}"))?;
        }
        Ok(Self { conn, path })
    }

    /// Open the standard harness database for a project.
    ///
    /// The path is resolved as: the value of the `MUSTARD_DB_PATH` environment
    /// variable if set, otherwise `{project_dir}/.claude/.harness/mustard.db`.
    ///
    /// **Self-crate guard:** when `CARGO_MANIFEST_DIR` is set (i.e. the binary
    /// is running under `cargo test` / `cargo run` from within a workspace
    /// crate) AND `project_dir` canonicalises to that same path, the store
    /// open is refused with [`Error::NotFound`]. Without this guard, in-crate
    /// test runs would leak `<crate>/.claude/.harness/mustard.db` on disk
    /// (umbrella AC-G2). Production callers pass the real workspace root
    /// from `MUSTARD_WORKSPACE_ROOT`, which never matches `CARGO_MANIFEST_DIR`
    /// at installed-binary runtime.
    ///
    /// # Errors
    ///
    /// Same as [`SqliteEventStore::new`], plus the self-crate refusal above.
    pub fn for_project(project_dir: impl AsRef<Path>) -> Result<Self> {
        let project = project_dir.as_ref();
        if project_is_own_crate(project) {
            return Err(Error::NotFound("self-crate store open refused".into()));
        }
        let env_override = std::env::var(DB_PATH_ENV).ok();
        Self::new(resolve_db_path(project, env_override.as_deref()))
    }

    /// The path of the backing database file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Borrow the store's open [`Connection`].
    ///
    /// Lets a caller reuse the single connection the constructor opened for the
    /// `writer` / `reader` free functions (which take `&Connection`) while still
    /// holding the store for its higher-level methods (`append`, `replay`, …) —
    /// so one open serves both the [`EventSink`](crate::store::event_store::EventSink)
    /// path and raw writer transactions within a single hook invocation.
    #[must_use]
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Consume the store, yielding its already-opened [`Connection`].
    ///
    /// Lets a caller that needs a bare `&Connection` (the `writer` / `reader`
    /// free functions) reuse the single connection the constructor opened —
    /// instead of constructing the store to apply schema, dropping it, then
    /// re-opening a second connection to the same file. The schema + migration
    /// work done in [`new`](Self::new) is already on this connection.
    #[must_use]
    pub fn into_connection(self) -> Connection {
        self.conn
    }

    /// Replay every lifecycle event, oldest first (by insertion `id`).
    ///
    /// W5 compatibility shim: the legacy `events` table is gone. This now
    /// reads from `pipeline_events` (lifecycle facts only — `pipeline.*`
    /// kinds). Tool / agent / qa events live in per-spec NDJSON files; see
    /// [`crate::projection::timeline`] for the NDJSON reader.
    ///
    /// Fail-open: a row that cannot be decoded into a [`HarnessEvent`] is
    /// skipped rather than aborting the replay.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] only for a genuine query failure — never for
    /// an empty table or an individual bad row.
    pub fn replay(&self) -> Result<Vec<HarnessEvent>> {
        self.select_pipeline_events(
            "SELECT id, ts, session_id, wave, spec, kind, payload \
             FROM pipeline_events ORDER BY id",
            [],
        )
    }

    /// Replay lifecycle events whose `ts` is `>= since_ts`, oldest first.
    ///
    /// W5 compatibility shim — reads from `pipeline_events`. See
    /// [`Self::replay`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] only for a genuine query failure.
    pub fn replay_since(&self, since_ts: Option<&str>) -> Result<Vec<HarnessEvent>> {
        match since_ts {
            Some(ts) => self.select_pipeline_events(
                "SELECT id, ts, session_id, wave, spec, kind, payload \
                 FROM pipeline_events WHERE ts >= ?1 ORDER BY id",
                params![ts],
            ),
            None => self.replay(),
        }
    }

    /// Delete `pipeline_events` rows older than `cutoff_ts` (an ISO-8601 string).
    ///
    /// W5 compatibility shim: prunes the lifecycle index. NDJSON files are
    /// pruned by `mustard-rt run spec-clear`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a database failure.
    pub fn prune_events_older_than(&self, cutoff_ts: &str) -> Result<usize> {
        let removed = self
            .conn
            .execute(
                "DELETE FROM pipeline_events WHERE ts < ?1",
                params![cutoff_ts],
            )?;
        Ok(removed)
    }

    /// Replay lifecycle events for a single spec, oldest first.
    ///
    /// W5 compatibility shim — reads from `pipeline_events`. A `None` argument
    /// matches events with no resolved spec (`spec IS NULL`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a query failure.
    pub fn query(&self, spec: Option<&str>) -> Result<Vec<HarnessEvent>> {
        match spec {
            Some(name) => self.select_pipeline_events(
                "SELECT id, ts, session_id, wave, spec, kind, payload \
                 FROM pipeline_events WHERE spec = ?1 ORDER BY id",
                params![name],
            ),
            None => self.select_pipeline_events(
                "SELECT id, ts, session_id, wave, spec, kind, payload \
                 FROM pipeline_events WHERE spec IS NULL ORDER BY id",
                [],
            ),
        }
    }

    /// W5 stub — the legacy `knowledge` FTS5 table is retired. Always returns
    /// an empty result. Callers should query `knowledge_patterns_fts` via
    /// dedicated readers (W6+).
    ///
    /// # Errors
    ///
    /// Never. Kept fallible for API compatibility with the pre-W5 signature.
    pub fn search(&self, _query: &str) -> Result<Vec<KnowledgeRow>> {
        Ok(Vec::new())
    }

    /// All rows of the `specs` projection.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a query failure.
    pub fn specs(&self) -> Result<Vec<SpecRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, status, phase, started_at, completed_at, \
             affected_files FROM specs ORDER BY name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SpecRow {
                name: row.get(0)?,
                status: row.get(1)?,
                phase: row.get(2)?,
                started_at: row.get(3)?,
                completed_at: row.get(4)?,
                affected_files: row.get(5)?,
            })
        })?;
        Ok(rows.filter_map(std::result::Result::ok).collect())
    }

    /// W5 stub — the `metrics_projection` table is retired (data duplicated
    /// `telemetry.db.run_usage`). Always returns `Ok(None)`. Callers that
    /// genuinely want metrics should query `telemetry::reader` against
    /// `run_usage` directly.
    ///
    /// # Errors
    ///
    /// Never. Kept fallible for API compatibility with the pre-W5 signature.
    pub fn metrics(&self, _spec: &str) -> Result<Option<MetricsRow>> {
        Ok(None)
    }

    /// The per-execution run rows for `spec`, ordered by start time.
    ///
    /// Wave 3 (telemetry-separation): this reads the self-attributed `run_usage`
    /// table in the dedicated `telemetry.db` (a sibling of this `mustard.db`),
    /// not the retired `spans` table — the returned [`RunRow`] shape is
    /// preserved for the MCP span summary consumer. The telemetry store is
    /// opened via [`crate::telemetry::TelemetryStore`], honouring its
    /// `MUSTARD_TELEMETRY_DB_PATH` env override; the sibling default sits next
    /// to this store's path.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a query failure.
    pub fn runs_by_spec(&self, spec: &str) -> Result<Vec<RunRow>> {
        let tele = self.open_telemetry()?;
        let rows = crate::telemetry::reader::runs_full_by_spec(tele.conn(), spec)?;
        Ok(rows
            .into_iter()
            .map(|r| RunRow {
                trace_id: r.trace_id,
                span_id: r.span_id,
                parent_span_id: r.parent_span_id,
                name: r.name,
                duration_ms: r.duration_ms,
                spec: r.spec,
                phase: r.phase,
                model: r.model,
                input_tokens: r.input_tokens,
                output_tokens: r.output_tokens,
                is_error: r.is_error,
            })
            .collect())
    }

    /// Open the dedicated telemetry store that sits beside this `mustard.db`.
    ///
    /// `MUSTARD_TELEMETRY_DB_PATH` wins when set (matching how the harness opens
    /// telemetry elsewhere); otherwise the path is the sibling `telemetry.db` in
    /// this store's own directory — robust regardless of where `mustard.db`
    /// lives (the standard layout puts both under `.claude/.harness/`).
    fn open_telemetry(&self) -> Result<crate::telemetry::TelemetryStore> {
        if let Ok(override_path) = std::env::var("MUSTARD_TELEMETRY_DB_PATH") {
            if !override_path.trim().is_empty() {
                return crate::telemetry::TelemetryStore::new(override_path);
            }
        }
        crate::telemetry::TelemetryStore::new(self.path.with_file_name("telemetry.db"))
    }

    /// All distinct non-null spec names present in `pipeline_events`, sorted
    /// alphabetically. W5 compatibility shim — reads from the lifecycle index.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a query failure.
    pub fn distinct_specs(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT spec FROM pipeline_events \
             WHERE spec IS NOT NULL ORDER BY spec",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        Ok(rows.filter_map(std::result::Result::ok).collect())
    }

    /// `true` when at least one row in `pipeline_events` has the given `kind`
    /// and `spec`. W5 compatibility shim (kind = the canonical event name).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a genuine query failure. A missing match
    /// is `Ok(false)`, not an error.
    pub fn has_event_for_spec(&self, event: &str, spec: &str) -> Result<bool> {
        let found: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM pipeline_events WHERE kind = ?1 AND spec = ?2 LIMIT 1",
                params![event, spec],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    /// Spec name of the most recent `pipeline.scope` event in `session_id`,
    /// excluding the `__orphan__` sentinel.
    ///
    /// Resolves the spec that is currently *active* for a session, which is the
    /// canonical input to attribution decisions (`current_spec` in `apps/rt`).
    /// Returns `None` when:
    ///
    /// - no `pipeline.scope` event has ever been emitted for `session_id`, or
    /// - every such event has `spec = NULL` / `spec = '__orphan__'`.
    ///
    /// Replaces the legacy mtime-of-`.pipeline-states/*.json` heuristic — that
    /// path went silent the moment `session_cleanup` removed the state file
    /// even while the Claude Code session continued working, which left every
    /// subsequent `tool.use` / `agent.start` event with `spec = NULL`. Reading
    /// from the event store is durable: the `pipeline.scope` event lives there
    /// until the database is wiped.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a genuine query failure. An empty result
    /// is `Ok(None)`, not an error.
    pub fn last_pipeline_scope_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<String>> {
        let spec: Option<String> = self
            .conn
            .query_row(
                "SELECT spec FROM pipeline_events \
                 WHERE kind = 'pipeline.scope' \
                   AND session_id = ?1 \
                   AND spec IS NOT NULL \
                   AND spec <> '__orphan__' \
                 ORDER BY id DESC \
                 LIMIT 1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(spec)
    }

    /// All amendment windows for `session_id`, ordered by `closed_at` descending.
    ///
    /// Returns all rows whose `session_id` matches, regardless of `status`.
    /// Used by `amend-finalize` to process every window at `SessionEnd`.
    ///
    /// JSON columns (`pipeline_file_set`, `subprojects`,
    /// `drift_unrelated_paths`) are decoded from their stored TEXT; a
    /// malformed array degrades to an empty `Vec` rather than an `Err`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a genuine query failure.
    pub fn amend_windows_by_session(&self, session_id: &str) -> Result<Vec<AmendWindow>> {
        let mut stmt = self.conn.prepare(
            "SELECT spec_id, session_id, closed_at, pipeline_file_set, subprojects, \
             status, last_activity_at, build_verde_at, drift_unrelated_paths, drift_emitted \
             FROM pipeline_amend_window \
             WHERE session_id = ?1 AND status IN ('open', 'amending') \
             ORDER BY closed_at DESC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            let pipeline_file_set_text: Option<String> = row.get(3)?;
            let subprojects_text: Option<String> = row.get(4)?;
            let drift_paths_text: Option<String> = row.get(8)?;
            let drift_emitted_raw: i64 = row.get::<_, Option<i64>>(9)?.unwrap_or(0);
            Ok(AmendWindow {
                spec_id: row.get(0)?,
                session_id: row.get(1)?,
                closed_at: row.get(2)?,
                pipeline_file_set: pipeline_file_set_text
                    .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
                    .unwrap_or_default(),
                subprojects: subprojects_text
                    .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
                    .unwrap_or_default(),
                status: row.get(5)?,
                last_activity_at: row.get(6)?,
                build_verde_at: row.get(7)?,
                drift_unrelated_paths: drift_paths_text
                    .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
                    .unwrap_or_default(),
                drift_emitted: drift_emitted_raw != 0,
            })
        })?;
        Ok(rows.filter_map(std::result::Result::ok).collect())
    }

    /// The open amendment window for `session_id`, if one exists.
    ///
    /// Queries `pipeline_amend_window` for a row whose `session_id` matches
    /// and whose `status` is `'open'` or `'amending'`, ordered by `closed_at`
    /// descending so the most recent window is returned. Returns `None` when no
    /// matching row exists — missing rows are not an error.
    ///
    /// JSON columns (`pipeline_file_set`, `subprojects`,
    /// `drift_unrelated_paths`) are decoded from their stored TEXT; a
    /// malformed array degrades to an empty `Vec` rather than an `Err`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a genuine query failure.
    pub fn amend_window_for_session(&self, session_id: &str) -> Result<Option<AmendWindow>> {
        let mut stmt = self.conn.prepare(
            "SELECT spec_id, session_id, closed_at, pipeline_file_set, subprojects, \
             status, last_activity_at, build_verde_at, drift_unrelated_paths, drift_emitted \
             FROM pipeline_amend_window \
             WHERE session_id = ?1 AND status IN ('open', 'amending') \
             ORDER BY closed_at DESC LIMIT 1",
        )?;
        let row = stmt
            .query_row(params![session_id], |row| {
                let pipeline_file_set_text: Option<String> = row.get(3)?;
                let subprojects_text: Option<String> = row.get(4)?;
                let drift_paths_text: Option<String> = row.get(8)?;
                let drift_emitted_raw: i64 = row.get::<_, Option<i64>>(9)?.unwrap_or(0);
                Ok(AmendWindow {
                    spec_id: row.get(0)?,
                    session_id: row.get(1)?,
                    closed_at: row.get(2)?,
                    pipeline_file_set: pipeline_file_set_text
                        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
                        .unwrap_or_default(),
                    subprojects: subprojects_text
                        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
                        .unwrap_or_default(),
                    status: row.get(5)?,
                    last_activity_at: row.get(6)?,
                    build_verde_at: row.get(7)?,
                    drift_unrelated_paths: drift_paths_text
                        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
                        .unwrap_or_default(),
                    drift_emitted: drift_emitted_raw != 0,
                })
            })
            .optional()?;
        Ok(row)
    }

    /// The union of all files modified across every `pipeline.task.complete`
    /// event for `spec_id`.
    ///
    /// Replays events scoped to `spec_id` (via [`Self::query`]), keeps only
    /// rows whose `event` field equals `"pipeline.task.complete"`, deserializes
    /// their `payload` as [`PipelineTaskCompletePayload`], and collects every
    /// path from `files_modified`. Paths are de-duplicated and returned sorted.
    ///
    /// A payload that fails to deserialize is skipped rather than aborting.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a query failure.
    pub fn amend_window_pipeline_file_set(&self, spec_id: &str) -> Result<Vec<String>> {
        let events = self.query(Some(spec_id))?;
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for ev in events {
            if ev.event != EVENT_PIPELINE_TASK_COMPLETE {
                continue;
            }
            let payload: PipelineTaskCompletePayload =
                match serde_json::from_value(ev.payload) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
            for path in payload.files_modified.unwrap_or_default() {
                seen.insert(path);
            }
        }
        Ok(seen.into_iter().collect())
    }

    // -------------------------------------------------------------------------
    // Projection-table write methods (specs, metrics_projection)
    //
    // The denormalized projections under `specs` and `metrics_projection` used
    // to be populated by the legacy JS harness. With the JS payload removed
    // the tables go stale unless a Rust writer keeps them alive — that writer
    // is `mustard-rt run rebuild-specs`, which uses these methods.
    //
    // Both methods do INSERT OR REPLACE on the primary key (spec name) so a
    // full re-materialization is safe to run any time.
    // -------------------------------------------------------------------------

    /// Insert-or-replace a row in the denormalized `specs` table.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a database failure.
    pub fn upsert_spec(&self, row: &SpecRow) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO specs(name, status, phase, started_at, completed_at, affected_files) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                row.name,
                row.status,
                row.phase,
                row.started_at,
                row.completed_at,
                row.affected_files,
            ],
        )?;
        Ok(())
    }

    /// W5 stub — the `metrics_projection` table is retired. This is a no-op
    /// preserved so existing callers (e.g. `mustard-rt run rebuild-specs`)
    /// compile while metrics flow through `telemetry::reader` against
    /// `telemetry.db.run_usage`.
    ///
    /// # Errors
    ///
    /// Never. Kept fallible for API compatibility.
    pub fn upsert_metrics(&self, _row: &MetricsRow) -> Result<()> {
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Amendment-window write methods (Wave 2 — session-bound amendments)
    // -------------------------------------------------------------------------

    /// Insert a new amendment window row.
    ///
    /// Uses `INSERT OR IGNORE` so a duplicate `(spec_id, session_id)` is
    /// silently skipped — idempotent by design.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a database failure.
    pub fn open_amend_window(&self, payload: &PipelineAmendOpenPayload) -> Result<()> {
        let file_set_json =
            serde_json::to_string(&payload.pipeline_file_set).map_err(Error::from)?;
        let subprojects_json =
            serde_json::to_string(&payload.subprojects).map_err(Error::from)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO pipeline_amend_window \
             (spec_id, session_id, closed_at, pipeline_file_set, subprojects) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                payload.spec_id,
                payload.session_id,
                payload.closed_at,
                file_set_json,
                subprojects_json,
            ],
        )?;
        Ok(())
    }

    /// Stamp the `last_activity_at` column for an open amendment window.
    ///
    /// Updates the row identified by `(spec_id, session_id)`. A missing row is
    /// a silent no-op — callers fail open.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a database failure.
    pub fn record_amend_activity(
        &self,
        spec_id: &str,
        session_id: &str,
        at: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE pipeline_amend_window \
             SET last_activity_at = ?3 \
             WHERE spec_id = ?1 AND session_id = ?2",
            params![spec_id, session_id, at],
        )?;
        Ok(())
    }

    /// Stamp `build_verde_at` for the open or amending window of `session_id`.
    ///
    /// Only updates rows whose `status` is `'open'` or `'amending'`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a database failure.
    pub fn mark_amend_build_verde(&self, session_id: &str, at: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE pipeline_amend_window \
             SET build_verde_at = ?2 \
             WHERE session_id = ?1 AND status IN ('open', 'amending')",
            params![session_id, at],
        )?;
        Ok(())
    }

    /// Append `file_path` to the `drift_unrelated_paths` JSON array for an
    /// open amendment window; skips duplicates.
    ///
    /// Reads the existing array, pushes `file_path` when absent, writes it
    /// back. Returns the new array length.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a database failure or
    /// [`Error::InvalidInput`] if the existing JSON array is irrecoverably
    /// malformed.
    pub fn add_amend_drift_path(
        &self,
        spec_id: &str,
        session_id: &str,
        file_path: &str,
    ) -> Result<usize> {
        // Read current array.
        let current_json: Option<String> = self
            .conn
            .query_row(
                "SELECT drift_unrelated_paths FROM pipeline_amend_window \
                 WHERE spec_id = ?1 AND session_id = ?2",
                params![spec_id, session_id],
                |row| row.get(0),
            )
            .optional()?;
        let mut paths: Vec<String> = current_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        if !paths.iter().any(|p| p == file_path) {
            paths.push(file_path.to_string());
        }
        let new_len = paths.len();
        let updated_json = serde_json::to_string(&paths).map_err(Error::from)?;
        self.conn.execute(
            "UPDATE pipeline_amend_window \
             SET drift_unrelated_paths = ?3 \
             WHERE spec_id = ?1 AND session_id = ?2",
            params![spec_id, session_id, updated_json],
        )?;
        Ok(new_len)
    }

    /// Set the `drift_emitted` flag to `1` for an amendment window.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a database failure.
    pub fn mark_amend_drift_emitted(&self, spec_id: &str, session_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE pipeline_amend_window \
             SET drift_emitted = 1 \
             WHERE spec_id = ?1 AND session_id = ?2",
            params![spec_id, session_id],
        )?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // W5 (2026-05-24-mustard-unification) — pipeline_events + sessions helpers.
    //
    // The hot-path event log moved to per-spec NDJSON files; SQLite keeps a
    // small `pipeline_events` table for the lifecycle events the dashboard
    // needs random-access reads for, plus a `sessions` registry for the
    // Sessions sidebar. The NDJSON writer (`apps/rt/src/run/event_writer_ndjson.rs`)
    // funnels the matching event kinds through `append_pipeline_event` so the
    // two stay in sync — NDJSON is the truth, SQLite is the index.
    // -------------------------------------------------------------------------

    /// Append one row to `pipeline_events`. `parent_id` ties Task children back
    /// to their dispatching event so the timeline UI can render execution
    /// trees (see W5.T5.3).
    ///
    /// `payload` is serialized once by the caller; passing it pre-serialized
    /// lets the hot path avoid a second `to_string` when it already holds the
    /// JSON text from the NDJSON write.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a database failure.
    pub fn append_pipeline_event(
        &self,
        ts: &str,
        session_id: Option<&str>,
        spec: Option<&str>,
        wave: Option<u32>,
        kind: &str,
        parent_id: Option<i64>,
        payload_json: Option<&str>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO pipeline_events \
             (ts, session_id, spec, wave, kind, parent_id, payload) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![ts, session_id, spec, wave, kind, parent_id, payload_json],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Insert or update one session row in the `sessions` registry.
    ///
    /// `INSERT OR REPLACE` keyed on `id`: the writer can call this on every
    /// `session.start` and every activity stamp without checking existence.
    /// `slug` is unique; conflicts on slug are rare in practice (timestamp +
    /// counter) and would return `Error::Sqlite` to the caller.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a database failure (including a slug
    /// collision).
    pub fn upsert_session(
        &self,
        id: &str,
        slug: &str,
        started_at: &str,
        last_activity_at: Option<&str>,
        last_spec: Option<&str>,
        cwd: Option<&str>,
        status: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions \
             (id, slug, started_at, last_activity_at, last_spec, cwd, status) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
             ON CONFLICT(id) DO UPDATE SET \
                last_activity_at = excluded.last_activity_at, \
                last_spec        = excluded.last_spec, \
                cwd              = excluded.cwd, \
                status           = excluded.status",
            params![id, slug, started_at, last_activity_at, last_spec, cwd, status],
        )?;
        Ok(())
    }

    /// Most-recent sessions, ordered by `last_activity_at DESC`. `limit` caps
    /// the result set; pass `None` for "all".
    ///
    /// Returns tuples of `(id, slug, started_at, last_activity_at, last_spec,
    /// cwd, status)` — kept as a tuple to avoid a public DTO until the
    /// dashboard reader stabilizes.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a database failure.
    #[allow(clippy::type_complexity)]
    pub fn recent_sessions(
        &self,
        limit: Option<u32>,
    ) -> Result<Vec<(String, String, String, Option<String>, Option<String>, Option<String>, String)>> {
        let sql = match limit {
            Some(_) => "SELECT id, slug, started_at, last_activity_at, last_spec, cwd, status \
                        FROM sessions ORDER BY COALESCE(last_activity_at, started_at) DESC LIMIT ?1",
            None => "SELECT id, slug, started_at, last_activity_at, last_spec, cwd, status \
                     FROM sessions ORDER BY COALESCE(last_activity_at, started_at) DESC",
        };
        let mut stmt = self.conn.prepare(sql)?;
        let row_map = |row: &rusqlite::Row<'_>| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
            ))
        };
        let rows = match limit {
            Some(n) => stmt.query_map(params![n], row_map)?.collect::<Vec<_>>(),
            None => stmt.query_map([], row_map)?.collect::<Vec<_>>(),
        };
        Ok(rows.into_iter().filter_map(std::result::Result::ok).collect())
    }

    /// Transition an amendment window to a terminal `status`.
    ///
    /// `status` should be one of `"resolved"`, `"pending"`, `"completed"`, or
    /// `"abandoned"`. The row is identified by `(spec_id, session_id)`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a database failure.
    pub fn close_amend_window(
        &self,
        spec_id: &str,
        session_id: &str,
        status: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE pipeline_amend_window \
             SET status = ?3 \
             WHERE spec_id = ?1 AND session_id = ?2",
            params![spec_id, session_id, status],
        )?;
        Ok(())
    }

    /// Run a `pipeline_events`-selecting query and decode each row into a
    /// [`HarnessEvent`].
    ///
    /// W5 helper: replaces the old `select_events` that read from the retired
    /// `events` table. Column order is fixed: `id, ts, session_id, wave, spec,
    /// kind, payload` — `pipeline_events` has no `actor_kind`/`actor_id` columns,
    /// so the decoded event always reports [`ActorKind::Hook`] with `id = None`.
    /// A row that fails to decode is skipped.
    fn select_pipeline_events(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<HarnessEvent>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params, |row| {
            let payload_text: Option<String> = row.get(6)?;
            Ok(HarnessEvent {
                v: SCHEMA_VERSION,
                ts: row.get(1)?,
                session_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                wave: row.get::<_, Option<i64>>(3)?.unwrap_or(0) as u32,
                actor: Actor {
                    kind: ActorKind::Hook,
                    id: None,
                    actor_type: None,
                },
                event: row.get(5)?,
                payload: payload_text
                    .and_then(|text| serde_json::from_str(&text).ok())
                    .unwrap_or(Value::Null),
                spec: row.get(4)?,
            })
        })?;
        Ok(rows.filter_map(std::result::Result::ok).collect())
    }
}

/// Resolve the harness database path for a project.
///
/// `env_override` is the raw value of `MUSTARD_DB_PATH` (the caller reads the
/// environment); when it is present and non-blank it wins verbatim. Otherwise
/// the path defaults to `{project_dir}/.claude/.harness/mustard.db`. Kept pure
/// — no environment access — so it is unit-testable without mutating process
/// state (`std::env::set_var` is `unsafe` under edition 2024 and this crate
/// forbids `unsafe`).
fn resolve_db_path(project_dir: &Path, env_override: Option<&str>) -> PathBuf {
    match env_override {
        Some(value) if !value.trim().is_empty() => PathBuf::from(value),
        _ => project_dir
            .join(".claude")
            .join(HARNESS_DIR)
            .join(DB_FILE),
    }
}

/// Self-crate guard used by [`SqliteEventStore::for_project`]: returns `true`
/// when `project_dir` canonicalises to `CARGO_MANIFEST_DIR`. That env var is
/// set by `cargo` only during build/test of a workspace crate, so it is a
/// reliable test-context fingerprint; at installed-binary runtime it is
/// absent and the guard never fires. Fail-open: any env read or canonicalise
/// failure returns `false` (the store opens as before).
fn project_is_own_crate(project_dir: &Path) -> bool {
    let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") else { return false; };
    let manifest_path = PathBuf::from(&manifest);
    let canon_manifest = manifest_path.canonicalize().unwrap_or(manifest_path);
    let canon_project = project_dir.canonicalize().unwrap_or_else(|_| project_dir.to_path_buf());
    canon_project == canon_manifest
}

// `parse_actor_kind` was used by the legacy `events`-table reader (which
// carried an `actor_kind` text column). `pipeline_events` does not.

// `actor_kind_str` was used by the legacy `events`-table writer (which carried
// `actor_kind` / `actor_id` columns). `pipeline_events` does not, so the
// W5-era `EventSink::append` no longer needs the helper.

impl super::event_store::EventSink for SqliteEventStore {
    /// Persist `event` according to the W5 split:
    ///
    /// - `pipeline.*` lifecycle events land in `pipeline_events` (the lean
    ///   in-database index the dashboard reads by spec).
    /// - Every other event kind (`tool.use`, `agent.start`, `qa.result`, …)
    ///   is a **no-op** at this sink: those events belong in the per-spec
    ///   NDJSON files (`apps/rt/src/run/event_writer_ndjson.rs`).
    ///
    /// The split keeps existing callers compiling without bypassing the W5
    /// hot-path split. Callers that want to make sure their event reaches the
    /// NDJSON sink must call `event_writer_ndjson::write_event` directly.
    fn append(&self, event: &HarnessEvent) -> Result<()> {
        if !event.event.starts_with("pipeline.") {
            return Ok(());
        }
        let payload = serde_json::to_string(&event.payload)?;
        let session_id: Option<&str> = if event.session_id.is_empty() {
            None
        } else {
            Some(event.session_id.as_str())
        };
        self.conn
            .execute(
                "INSERT INTO pipeline_events \
                 (ts, session_id, spec, wave, kind, payload) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    event.ts,
                    session_id,
                    event.spec,
                    event.wave,
                    event.event,
                    payload,
                ],
            )
            .map_err(Error::from)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::event_store::EventSink;
    use serde_json::json;
    use tempfile::tempdir;

    fn lifecycle_event(name: &str, spec: Option<&str>) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-19T00:00:00.000Z".to_string(),
            session_id: "s-test".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Hook,
                id: Some("sqlite-store-test".to_string()),
                actor_type: None,
            },
            event: name.to_string(),
            payload: json!({"k": "v"}),
            spec: spec.map(ToString::to_string),
        }
    }

    fn store_in(dir: &Path) -> SqliteEventStore {
        SqliteEventStore::new(dir.join("mustard.db")).unwrap()
    }

    /// W5 schema check: `events` and its FTS5 mirror must NOT be present
    /// after a fresh open at LATEST_VERSION.
    #[test]
    fn fresh_open_has_no_legacy_events_table() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        let exists = |name: &str| -> bool {
            store
                .conn
                .query_row(
                    "SELECT 1 FROM sqlite_master WHERE name = ?1 LIMIT 1",
                    params![name],
                    |r| r.get::<_, i64>(0),
                )
                .optional()
                .unwrap()
                .is_some()
        };
        for table in ["events", "events_fts", "knowledge", "knowledge_fts", "metrics_projection"] {
            assert!(!exists(table), "{table} must not exist in W5 schema");
        }
        for table in ["pipeline_events", "sessions", "specs", "pipeline_amend_window"] {
            assert!(exists(table), "{table} must exist in W5 schema");
        }
    }

    /// Append-then-replay round-trips a lifecycle event through pipeline_events.
    /// Filters out the `pipeline.economy.schema.shrunk` migration-emitted event
    /// the v9→v10 step writes when legacy intermediate tables were dropped.
    #[test]
    fn append_lifecycle_event_round_trips() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        store
            .append(&lifecycle_event("pipeline.scope", Some("spec-a")))
            .unwrap();
        store
            .append(&lifecycle_event("pipeline.phase", Some("spec-a")))
            .unwrap();
        let events: Vec<_> = store
            .replay()
            .unwrap()
            .into_iter()
            .filter(|e| e.event != "pipeline.economy.schema.shrunk")
            .collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event, "pipeline.scope");
        assert_eq!(events[1].event, "pipeline.phase");
        assert_eq!(events[0].spec.as_deref(), Some("spec-a"));
    }

    /// Non-pipeline events (tool.use, etc) are no-ops in the SQLite sink —
    /// the W5 contract moves them to per-spec NDJSON files. The migration
    /// `pipeline.economy.schema.shrunk` event (from v9→v10) is filtered out.
    #[test]
    fn append_non_pipeline_event_is_no_op() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        store.append(&lifecycle_event("tool.use", Some("spec-a"))).unwrap();
        store.append(&lifecycle_event("agent.start", Some("spec-a"))).unwrap();
        let events: Vec<_> = store
            .replay()
            .unwrap()
            .into_iter()
            .filter(|e| e.event != "pipeline.economy.schema.shrunk")
            .collect();
        assert!(events.is_empty());
    }

    /// `query` filters by spec on the pipeline_events table.
    #[test]
    fn query_filters_by_spec() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        store
            .append(&lifecycle_event("pipeline.scope", Some("spec-x")))
            .unwrap();
        store
            .append(&lifecycle_event("pipeline.scope", Some("spec-y")))
            .unwrap();
        store
            .append(&lifecycle_event("pipeline.phase", Some("spec-x")))
            .unwrap();
        assert_eq!(store.query(Some("spec-x")).unwrap().len(), 2);
        assert_eq!(store.query(Some("spec-y")).unwrap().len(), 1);
    }

    /// `distinct_specs` reads off pipeline_events and excludes NULLs.
    #[test]
    fn distinct_specs_lists_known_specs() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        for s in ["spec-a", "spec-b", "spec-a"] {
            store.append(&lifecycle_event("pipeline.scope", Some(s))).unwrap();
        }
        let specs = store.distinct_specs().unwrap();
        assert_eq!(specs, vec!["spec-a".to_string(), "spec-b".to_string()]);
    }

    /// W5 stubs: `search`, `metrics`, `upsert_metrics` are no-ops that compile.
    #[test]
    fn w5_stubs_compile_and_return_empty() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        assert!(store.search("anything").unwrap().is_empty());
        assert!(store.metrics("nope").unwrap().is_none());
        let row = MetricsRow {
            spec: "spec-a".into(),
            api_calls: None,
            retries: None,
            pass1: None,
            tool_breakdown: None,
            dispatch_failures_by_phase: None,
            agent_count: None,
            updated_at: None,
        };
        store.upsert_metrics(&row).unwrap();
    }

    #[test]
    fn resolve_db_path_honors_env_override() {
        let resolved = resolve_db_path(Path::new("/unused/project"), Some("/custom/my.db"));
        assert_eq!(resolved, PathBuf::from("/custom/my.db"));
    }

    #[test]
    fn resolve_db_path_falls_back_to_standard_path() {
        for env in [None, Some("   ")] {
            let resolved = resolve_db_path(Path::new("/proj"), env);
            assert!(resolved.ends_with("mustard.db"));
            assert!(resolved.components().any(|c| c.as_os_str() == ".harness"));
        }
    }

    #[test]
    fn for_project_opens_a_usable_store() {
        let dir = tempdir().unwrap();
        let store = SqliteEventStore::for_project(dir.path()).unwrap();
        assert!(store.path().ends_with("mustard.db"));
        // The v9→v10 migration emits a single `pipeline.economy.schema.shrunk`
        // event when intermediate ladder tables (savings_records, etc.) get
        // dropped on first open — filter it out for the empty-replay assertion.
        let events: Vec<_> = store
            .replay()
            .unwrap()
            .into_iter()
            .filter(|e| e.event != "pipeline.economy.schema.shrunk")
            .collect();
        assert!(events.is_empty());
    }

    /// AC-G3: an existing DB that still carries the legacy `events` table sees
    /// it dropped during the v9→v10 migration on the next open.
    #[test]
    fn ac_g3_legacy_events_dropped_on_migration() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("mustard.db");
        // Build a database pinned at v9 with the legacy `events` table present.
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE events (id INTEGER PRIMARY KEY, ts TEXT); \
                 CREATE TABLE knowledge (id TEXT PRIMARY KEY); \
                 CREATE TABLE metrics_projection (spec TEXT PRIMARY KEY); \
                 CREATE TABLE _mustard_meta (key TEXT PRIMARY KEY, value TEXT); \
                 INSERT INTO _mustard_meta(key, value) VALUES('schema_version', '9'); \
                 PRAGMA user_version = 0;",
            )
            .unwrap();
        }

        // Real open triggers migrate_v9_to_v10.
        let store = SqliteEventStore::new(&db).unwrap();
        let exists = |name: &str| -> bool {
            store
                .conn
                .query_row(
                    "SELECT 1 FROM sqlite_master WHERE name = ?1 LIMIT 1",
                    params![name],
                    |r| r.get::<_, i64>(0),
                )
                .optional()
                .unwrap()
                .is_some()
        };
        assert!(!exists("events"), "events must be dropped by v10 migration");
        assert!(!exists("knowledge"), "knowledge must be dropped by v10");
        assert!(
            !exists("metrics_projection"),
            "metrics_projection must be dropped by v10"
        );
    }

    /// Amendment-window writer round-trip — the core of the lean SQLite face.
    mod amend_window_writers {
        use super::*;
        use crate::model::event::PipelineAmendOpenPayload;

        fn open_payload(spec_id: &str, session_id: &str) -> PipelineAmendOpenPayload {
            PipelineAmendOpenPayload {
                spec_id: spec_id.to_string(),
                session_id: session_id.to_string(),
                closed_at: "2026-05-20T00:00:00.000Z".to_string(),
                pipeline_file_set: vec!["apps/rt/src/hooks/mod.rs".to_string()],
                subprojects: vec!["apps/rt".to_string()],
            }
        }

        #[test]
        fn open_then_close_amend_window_round_trips() {
            let dir = tempdir().unwrap();
            let store = store_in(dir.path());
            store.open_amend_window(&open_payload("spec-1", "session-a")).unwrap();

            let window = store.amend_window_for_session("session-a").unwrap().unwrap();
            assert_eq!(window.spec_id, "spec-1");
            assert_eq!(window.status, "open");

            store
                .close_amend_window("spec-1", "session-a", "resolved")
                .unwrap();
            assert!(store.amend_window_for_session("session-a").unwrap().is_none());
        }
    }
}
