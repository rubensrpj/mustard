//! The SQLite-backed harness store — `.claude/.harness/mustard.db`.
//!
//! [`SqliteEventStore`] is the single store the harness reads from and writes
//! to. It persists events in one `SQLite` database opened in **WAL mode**: WAL
//! lets a writer and any number of readers proceed concurrently, and a
//! per-connection `busy_timeout` makes a contended write *wait* instead of
//! erroring — the property the harness needs when several hooks fire in
//! parallel.
//!
//! The schema is the one shipped by the legacy TypeScript event store
//! (`apps/cli/src/runtime/schema.sql`): an append-only `events` table with an
//! `events_fts` FTS5 mirror, plus the denormalized `specs`, `metrics_projection`,
//! `knowledge` (+ standalone `knowledge_fts`), and `spans` projections. Every
//! `CREATE` is `IF NOT EXISTS`, so [`SqliteEventStore::new`] applies it on
//! every open without harm.
//!
//! This store implements the [`EventSink`](super::event_store::EventSink)
//! trait: `append` is INSERT-only, preserving the append-only / audit
//! semantics of the event log. The FTS mirror is kept in sync by the
//! `events_ai` trigger in the schema, not by application code.
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
    /// # Errors
    ///
    /// Same as [`SqliteEventStore::new`].
    pub fn for_project(project_dir: impl AsRef<Path>) -> Result<Self> {
        let env_override = std::env::var(DB_PATH_ENV).ok();
        Self::new(resolve_db_path(project_dir.as_ref(), env_override.as_deref()))
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

    /// Replay every event, oldest first (by insertion `id`).
    ///
    /// Fail-open: a row that cannot be decoded into a [`HarnessEvent`] is
    /// skipped rather than aborting the replay.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] only for a genuine query failure — never for
    /// an empty table or an individual bad row.
    pub fn replay(&self) -> Result<Vec<HarnessEvent>> {
        self.select_events("SELECT id, ts, session_id, wave, spec, event, \
             actor_kind, actor_id, payload FROM events ORDER BY id", [])
    }

    /// Replay events whose `ts` is `>= since_ts`, oldest first.
    ///
    /// `ts` is the ISO-8601 string column; the comparison is lexical, which is
    /// correct for the fixed-width UTC timestamps the harness emits. A `None`
    /// argument is equivalent to [`replay`](Self::replay) (no lower bound).
    ///
    /// Used by the workspace summary to avoid an unbounded full scan of the
    /// `events` table — only the recent window the dashboard renders is read.
    ///
    /// Fail-open: a row that cannot be decoded is skipped, not fatal.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] only for a genuine query failure.
    pub fn replay_since(&self, since_ts: Option<&str>) -> Result<Vec<HarnessEvent>> {
        match since_ts {
            Some(ts) => self.select_events(
                "SELECT id, ts, session_id, wave, spec, event, actor_kind, \
                 actor_id, payload FROM events WHERE ts >= ?1 ORDER BY id",
                params![ts],
            ),
            None => self.replay(),
        }
    }

    /// Delete `events` rows older than `cutoff_ts` (an ISO-8601 string).
    ///
    /// Retention helper for the append-only log: removes rows whose `ts` is
    /// strictly less than `cutoff_ts`. The `events_fts` external-content index
    /// is kept consistent by the `events_ad` AFTER DELETE trigger, which removes
    /// each pruned row's FTS entry incrementally — no orphaned index rows remain.
    /// Returns the number of rows removed.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a database failure.
    pub fn prune_events_older_than(&self, cutoff_ts: &str) -> Result<usize> {
        let removed = self
            .conn
            .execute("DELETE FROM events WHERE ts < ?1", params![cutoff_ts])?;
        Ok(removed)
    }

    /// Replay the events for a single spec, oldest first.
    ///
    /// A `None` argument matches events with no resolved spec (`spec IS NULL`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a query failure.
    pub fn query(&self, spec: Option<&str>) -> Result<Vec<HarnessEvent>> {
        match spec {
            Some(name) => self.select_events(
                "SELECT id, ts, session_id, wave, spec, event, actor_kind, \
                 actor_id, payload FROM events WHERE spec = ?1 ORDER BY id",
                params![name],
            ),
            None => self.select_events(
                "SELECT id, ts, session_id, wave, spec, event, actor_kind, \
                 actor_id, payload FROM events WHERE spec IS NULL ORDER BY id",
                [],
            ),
        }
    }

    /// Full-text search the `knowledge` table, ranked by FTS5 `bm25`.
    ///
    /// `query` is an FTS5 MATCH expression; results come back best-match
    /// first. `knowledge_fts` is a standalone (non-content) FTS5 table whose
    /// `id` column is `UNINDEXED`, so the match is joined back to `knowledge`
    /// on `id` to recover the full row.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a query failure (including a malformed
    /// MATCH expression).
    pub fn search(&self, query: &str) -> Result<Vec<KnowledgeRow>> {
        let sql = "SELECT k.id, k.type, k.name, k.description, k.confidence \
                   FROM knowledge_fts f \
                   JOIN knowledge k ON k.id = f.id \
                   WHERE knowledge_fts MATCH ?1 \
                   ORDER BY bm25(knowledge_fts)";
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params![query], |row| {
            Ok(KnowledgeRow {
                id: row.get(0)?,
                kind: row.get(1)?,
                name: row.get(2)?,
                description: row.get(3)?,
                confidence: row.get(4)?,
            })
        })?;
        Ok(rows.filter_map(std::result::Result::ok).collect())
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

    /// The `metrics_projection` row for `spec`, if one exists.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a query failure.
    pub fn metrics(&self, spec: &str) -> Result<Option<MetricsRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT spec, api_calls, retries, pass1, tool_breakdown, \
             dispatch_failures_by_phase, agent_count, updated_at \
             FROM metrics_projection WHERE spec = ?1",
        )?;
        let row = stmt
            .query_row(params![spec], |row| {
                Ok(MetricsRow {
                    spec: row.get(0)?,
                    api_calls: row.get(1)?,
                    retries: row.get(2)?,
                    pass1: row.get(3)?,
                    tool_breakdown: row.get(4)?,
                    dispatch_failures_by_phase: row.get(5)?,
                    agent_count: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })
            .optional()?;
        Ok(row)
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

    /// All distinct non-null spec names present in the `events` table, sorted
    /// alphabetically.
    ///
    /// Used by `archive_followups` to enumerate every spec the event store
    /// knows about without scanning the filesystem.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a query failure.
    pub fn distinct_specs(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT spec FROM events WHERE spec IS NOT NULL ORDER BY spec",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        Ok(rows.filter_map(std::result::Result::ok).collect())
    }

    /// `true` when at least one row in `events` has the given `event` name and
    /// `spec`. A cheap existence probe (`SELECT 1 ... LIMIT 1`) — the intended
    /// replacement for `replay()`-as-lookup, which scans and decodes the whole
    /// table just to test membership.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a genuine query failure. A missing match
    /// is `Ok(false)`, not an error.
    pub fn has_event_for_spec(&self, event: &str, spec: &str) -> Result<bool> {
        let found: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM events WHERE event = ?1 AND spec = ?2 LIMIT 1",
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
                "SELECT spec FROM events \
                 WHERE event = 'pipeline.scope' \
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

    /// Insert-or-replace a row in the denormalized `metrics_projection` table.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a database failure.
    pub fn upsert_metrics(&self, row: &MetricsRow) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO metrics_projection(\
             spec, api_calls, retries, pass1, tool_breakdown, \
             dispatch_failures_by_phase, agent_count, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                row.spec,
                row.api_calls,
                row.retries,
                row.pass1,
                row.tool_breakdown,
                row.dispatch_failures_by_phase,
                row.agent_count,
                row.updated_at,
            ],
        )?;
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

    /// Run an event-selecting query and decode each row into a [`HarnessEvent`].
    ///
    /// Shared by [`replay`](Self::replay) and [`query`](Self::query). The
    /// column order is fixed: `id, ts, session_id, wave, spec, event,
    /// actor_kind, actor_id, payload`. A row that fails to decode is skipped.
    fn select_events(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<HarnessEvent>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params, |row| {
            let actor_kind: Option<String> = row.get(6)?;
            let actor_id: Option<String> = row.get(7)?;
            let payload_text: Option<String> = row.get(8)?;
            Ok(HarnessEvent {
                v: SCHEMA_VERSION,
                ts: row.get(1)?,
                session_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                // cast_sign_loss/cast_possible_truncation: wave numbers are non-negative and small.
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                wave: row.get::<_, Option<i64>>(3)?.unwrap_or(0) as u32,
                actor: Actor {
                    kind: parse_actor_kind(actor_kind.as_deref()),
                    id: actor_id,
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

/// Decode the stored `actor_kind` string into an [`ActorKind`].
///
/// Falls back to [`ActorKind::Hook`] for an absent or unrecognised value —
/// the harness emits `"hook"` for the overwhelming majority of events, so it
/// is the safe default rather than an error.
fn parse_actor_kind(raw: Option<&str>) -> ActorKind {
    match raw {
        Some("agent") => ActorKind::Agent,
        Some("orchestrator") => ActorKind::Orchestrator,
        Some("cli") => ActorKind::Cli,
        _ => ActorKind::Hook,
    }
}

/// Render an [`ActorKind`] back to its lowercase wire string.
fn actor_kind_str(kind: ActorKind) -> &'static str {
    match kind {
        ActorKind::Hook => "hook",
        ActorKind::Agent => "agent",
        ActorKind::Orchestrator => "orchestrator",
        ActorKind::Cli => "cli",
    }
}

impl super::event_store::EventSink for SqliteEventStore {
    fn append(&self, event: &HarnessEvent) -> Result<()> {
        // INSERT-only: the events table is append-only. The `events_ai`
        // trigger in the schema mirrors the row into `events_fts`.
        let payload = serde_json::to_string(&event.payload)?;
        self.conn
            .execute(
                "INSERT INTO events \
                 (ts, session_id, wave, spec, event, actor_kind, actor_id, payload) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    event.ts,
                    event.session_id,
                    event.wave,
                    event.spec,
                    event.event,
                    actor_kind_str(event.actor.kind),
                    event.actor.id,
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

    fn sample_event(name: &str, spec: Option<&str>) -> HarnessEvent {
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

    #[test]
    fn second_open_takes_fast_path_and_preserves_user_version() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("mustard.db");

        // First open materializes the schema and stamps user_version.
        {
            let store = SqliteEventStore::new(&db).unwrap();
            let uv: i64 = store
                .conn
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert_eq!(uv, i64::from(crate::store::migrations::LATEST_VERSION));
        }

        // Drop a sentinel row into _mustard_meta. If the second open re-ran the
        // migration ladder it would touch this table; the fast-path must not.
        {
            let probe = Connection::open(&db).unwrap();
            probe
                .execute(
                    "INSERT OR REPLACE INTO _mustard_meta(key, value) \
                     VALUES('fast_path_probe', 'untouched')",
                    [],
                )
                .unwrap();
        }

        // Second open: user_version already at latest -> DDL + migrations skip.
        let store = SqliteEventStore::new(&db).unwrap();
        let uv: i64 = store
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(uv, i64::from(crate::store::migrations::LATEST_VERSION));
        let probe: String = store
            .conn
            .query_row(
                "SELECT value FROM _mustard_meta WHERE key = 'fast_path_probe'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(probe, "untouched");
    }

    #[test]
    fn prune_events_older_than_removes_expected_rows() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        for ts in ["2026-01-01T00:00:00Z", "2026-03-01T00:00:00Z", "2026-06-01T00:00:00Z"] {
            let mut ev = sample_event("tool.use", None);
            ev.ts = ts.to_string();
            store.append(&ev).unwrap();
        }
        let removed = store.prune_events_older_than("2026-04-01T00:00:00Z").unwrap();
        assert_eq!(removed, 2);
        assert_eq!(store.replay().unwrap().len(), 1);
        // replay_since with a lower bound matches only the surviving row.
        assert_eq!(
            store.replay_since(Some("2026-05-01T00:00:00Z")).unwrap().len(),
            1
        );
    }

    #[test]
    fn prune_events_older_than_clears_fts_index_for_pruned_rows() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());

        // A pruned row carries a unique event term; a surviving row carries another.
        let mut old_ev = sample_event("uniquepruned", None);
        old_ev.ts = "2026-01-01T00:00:00Z".to_string();
        store.append(&old_ev).unwrap();
        let mut new_ev = sample_event("uniquekept", None);
        new_ev.ts = "2026-06-01T00:00:00Z".to_string();
        store.append(&new_ev).unwrap();

        let fts_count = |term: &str| -> i64 {
            store
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM events_fts WHERE events_fts MATCH ?1",
                    params![term],
                    |r| r.get(0),
                )
                .unwrap()
        };
        assert_eq!(fts_count("uniquepruned"), 1);

        store.prune_events_older_than("2026-04-01T00:00:00Z").unwrap();

        // The pruned row's FTS entry is gone; the surviving row's remains.
        assert_eq!(fts_count("uniquepruned"), 0);
        assert_eq!(fts_count("uniquekept"), 1);
    }

    #[test]
    fn append_then_replay_round_trips() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        store.append(&sample_event("session.start", None)).unwrap();
        store.append(&sample_event("tool.use", None)).unwrap();

        let events = store.replay().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event, "session.start");
        assert_eq!(events[1].event, "tool.use");
        assert_eq!(events[1].payload, json!({"k": "v"}));
        assert_eq!(events[1].actor.kind, ActorKind::Hook);
        assert_eq!(events[1].actor.id.as_deref(), Some("sqlite-store-test"));
    }

    #[test]
    fn replay_fresh_db_is_empty_not_error() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        assert!(store.replay().unwrap().is_empty());
        assert!(store.query(Some("nope")).unwrap().is_empty());
        assert!(store.specs().unwrap().is_empty());
        assert!(store.metrics("nope").unwrap().is_none());
        assert!(store.runs_by_spec("nope").unwrap().is_empty());
        assert!(store.search("anything").unwrap().is_empty());
    }

    #[test]
    fn query_filters_by_spec() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        store.append(&sample_event("a", Some("spec-x"))).unwrap();
        store.append(&sample_event("b", Some("spec-y"))).unwrap();
        store.append(&sample_event("c", Some("spec-x"))).unwrap();
        store.append(&sample_event("d", None)).unwrap();

        let x = store.query(Some("spec-x")).unwrap();
        assert_eq!(x.len(), 2);
        assert_eq!(x[0].event, "a");
        assert_eq!(x[1].event, "c");

        let none = store.query(None).unwrap();
        assert_eq!(none.len(), 1);
        assert_eq!(none[0].event, "d");
    }

    #[test]
    fn search_ranks_knowledge_by_bm25() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        // Seed the knowledge table directly; the harness owns its rowid.
        store
            .conn
            .execute(
                "INSERT INTO knowledge (id, type, name, description, confidence) \
                 VALUES ('k1', 'pattern', 'event sink trait', \
                 'an interface for appending events', 0.9)",
                [],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO knowledge (id, type, name, description, confidence) \
                 VALUES ('k2', 'convention', 'naming', \
                 'use snake_case for files', 0.5)",
                [],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO knowledge_fts (id, name, description) \
                 SELECT id, name, description FROM knowledge",
                [],
            )
            .unwrap();

        let hits = store.search("event").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "k1");
        assert_eq!(hits[0].kind.as_deref(), Some("pattern"));

        // A term in both rows returns both.
        store
            .conn
            .execute(
                "INSERT INTO knowledge (id, type, name, description, confidence) \
                 VALUES ('k3', 'pattern', 'files', 'event log files', 0.7)",
                [],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO knowledge_fts (id, name, description) \
                 VALUES ('k3', 'files', 'event log files')",
                [],
            )
            .unwrap();
        assert_eq!(store.search("files").unwrap().len(), 2);
    }

    #[test]
    fn specs_metrics_and_spans_decode() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        store
            .conn
            .execute(
                "INSERT INTO specs (name, status, phase) \
                 VALUES ('2026-spec', 'active', 'EXECUTE')",
                [],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO metrics_projection (spec, api_calls, retries) \
                 VALUES ('2026-spec', 12, 3)",
                [],
            )
            .unwrap();
        // Wave 3: `runs_by_spec()` reads the sibling telemetry.db `run_usage`,
        // not the retired mustard.db `spans` table. Seed a run there via the
        // same sibling-path resolution `open_telemetry` uses.
        {
            let tele = crate::telemetry::TelemetryStore::new(
                store.path.with_file_name("telemetry.db"),
            )
            .unwrap();
            tele.conn()
                .execute(
                    "INSERT INTO run_usage (span_id, name, spec, phase, is_error) \
                     VALUES ('sp-1', 'plan', '2026-spec', 'PLAN', 0)",
                    [],
                )
                .unwrap();
        }

        let specs = store.specs().unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "2026-spec");
        assert_eq!(specs[0].status.as_deref(), Some("active"));

        let metrics = store.metrics("2026-spec").unwrap().unwrap();
        assert_eq!(metrics.api_calls, Some(12));
        assert_eq!(metrics.retries, Some(3));

        let runs = store.runs_by_spec("2026-spec").unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].span_id, "sp-1");
        assert!(!runs[0].is_error);
    }

    #[test]
    fn resolve_db_path_honors_env_override() {
        // A non-blank MUSTARD_DB_PATH wins verbatim, ignoring the project dir.
        let resolved =
            resolve_db_path(Path::new("/unused/project"), Some("/custom/my.db"));
        assert_eq!(resolved, PathBuf::from("/custom/my.db"));
    }

    #[test]
    fn resolve_db_path_falls_back_to_standard_path() {
        // No override (and a blank override) resolves the standard location.
        for env in [None, Some("   ")] {
            let resolved = resolve_db_path(Path::new("/proj"), env);
            assert!(resolved.ends_with("mustard.db"));
            assert!(
                resolved
                    .components()
                    .any(|c| c.as_os_str() == ".harness")
            );
        }
    }

    #[test]
    fn for_project_opens_a_usable_store() {
        // `for_project` reads the real env; with no override set it resolves
        // under the given dir. End-to-end: it must open and replay empty.
        let dir = tempdir().unwrap();
        let store = SqliteEventStore::for_project(dir.path()).unwrap();
        assert!(store.path().ends_with("mustard.db"));
        assert!(store.replay().unwrap().is_empty());
    }

    // -------------------------------------------------------------------------
    // Amendment-window writer round-trip tests
    // -------------------------------------------------------------------------

    mod amend_window_writers {
        use super::*;
        use crate::model::event::PipelineAmendOpenPayload;

        fn open_payload(spec_id: &str, session_id: &str) -> PipelineAmendOpenPayload {
            PipelineAmendOpenPayload {
                spec_id: spec_id.to_string(),
                session_id: session_id.to_string(),
                closed_at: "2026-05-20T00:00:00.000Z".to_string(),
                pipeline_file_set: vec![
                    "apps/rt/src/hooks/mod.rs".to_string(),
                    "apps/cli/src/main.rs".to_string(),
                ],
                subprojects: vec!["apps/rt".to_string(), "apps/cli".to_string()],
            }
        }

        #[test]
        fn open_amend_window_inserts_row() {
            let dir = tempdir().unwrap();
            let store = store_in(dir.path());
            let payload = open_payload("spec-1", "session-a");
            store.open_amend_window(&payload).unwrap();

            let window = store.amend_window_for_session("session-a").unwrap().unwrap();
            assert_eq!(window.spec_id, "spec-1");
            assert_eq!(window.status, "open");
            assert_eq!(window.pipeline_file_set, payload.pipeline_file_set);
            assert_eq!(window.subprojects, payload.subprojects);
        }

        #[test]
        fn open_amend_window_is_idempotent() {
            let dir = tempdir().unwrap();
            let store = store_in(dir.path());
            let payload = open_payload("spec-1", "session-a");
            store.open_amend_window(&payload).unwrap();
            // Second call must be a silent no-op (INSERT OR IGNORE).
            store.open_amend_window(&payload).unwrap();
            // Still exactly one window.
            let window = store.amend_window_for_session("session-a").unwrap();
            assert!(window.is_some());
        }

        #[test]
        fn record_amend_activity_stamps_last_activity() {
            let dir = tempdir().unwrap();
            let store = store_in(dir.path());
            store.open_amend_window(&open_payload("spec-1", "session-a")).unwrap();

            store
                .record_amend_activity("spec-1", "session-a", "2026-05-20T01:00:00.000Z")
                .unwrap();

            let window = store.amend_window_for_session("session-a").unwrap().unwrap();
            assert_eq!(
                window.last_activity_at.as_deref(),
                Some("2026-05-20T01:00:00.000Z")
            );
        }

        #[test]
        fn mark_amend_build_verde_stamps_build_verde_at() {
            let dir = tempdir().unwrap();
            let store = store_in(dir.path());
            store.open_amend_window(&open_payload("spec-1", "session-a")).unwrap();

            store
                .mark_amend_build_verde("session-a", "2026-05-20T02:00:00.000Z")
                .unwrap();

            let window = store.amend_window_for_session("session-a").unwrap().unwrap();
            assert_eq!(
                window.build_verde_at.as_deref(),
                Some("2026-05-20T02:00:00.000Z")
            );
        }

        #[test]
        fn add_amend_drift_path_accumulates_unique_paths() {
            let dir = tempdir().unwrap();
            let store = store_in(dir.path());
            store.open_amend_window(&open_payload("spec-1", "session-a")).unwrap();

            let len1 = store
                .add_amend_drift_path("spec-1", "session-a", "packages/core/src/lib.rs")
                .unwrap();
            assert_eq!(len1, 1);

            let len2 = store
                .add_amend_drift_path("spec-1", "session-a", "packages/core/src/error.rs")
                .unwrap();
            assert_eq!(len2, 2);

            // Duplicate — length must not grow.
            let len3 = store
                .add_amend_drift_path("spec-1", "session-a", "packages/core/src/lib.rs")
                .unwrap();
            assert_eq!(len3, 2);

            let window = store.amend_window_for_session("session-a").unwrap().unwrap();
            assert_eq!(window.drift_unrelated_paths.len(), 2);
        }

        #[test]
        fn mark_amend_drift_emitted_sets_flag() {
            let dir = tempdir().unwrap();
            let store = store_in(dir.path());
            store.open_amend_window(&open_payload("spec-1", "session-a")).unwrap();

            let window_before = store.amend_window_for_session("session-a").unwrap().unwrap();
            assert!(!window_before.drift_emitted);

            store.mark_amend_drift_emitted("spec-1", "session-a").unwrap();

            let window_after = store.amend_window_for_session("session-a").unwrap().unwrap();
            assert!(window_after.drift_emitted);
        }

        #[test]
        fn close_amend_window_transitions_status() {
            let dir = tempdir().unwrap();
            let store = store_in(dir.path());
            store.open_amend_window(&open_payload("spec-1", "session-a")).unwrap();

            store.close_amend_window("spec-1", "session-a", "resolved").unwrap();

            // After close, amend_window_for_session (which filters on
            // status IN ('open','amending')) must return None.
            let window = store.amend_window_for_session("session-a").unwrap();
            assert!(window.is_none());
        }
    }
}
