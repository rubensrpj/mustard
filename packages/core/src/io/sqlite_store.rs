//! The SQLite-backed harness store — `.claude/.harness/mustard.db`.
//!
//! [`SqliteEventStore`] is the single store the harness reads from and writes
//! to. It persists events in one SQLite database opened in **WAL mode**: WAL
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
use crate::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Directory name of the harness store, under `.claude/`.
const HARNESS_DIR: &str = ".harness";

/// Default file name of the SQLite database.
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

/// One row from the `spans` projection — a single OTEL-style span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanRow {
    /// Trace the span belongs to.
    pub trace_id: Option<String>,
    /// Span identifier (primary key).
    pub span_id: String,
    /// Parent span, when this is a child span.
    pub parent_span_id: Option<String>,
    /// Human-readable span name.
    pub name: Option<String>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: Option<i64>,
    /// Spec the span is attributed to.
    pub spec: Option<String>,
    /// Pipeline phase the span occurred in.
    pub phase: Option<String>,
    /// Model in use during the span.
    pub model: Option<String>,
    /// Input token count, when the span carried token usage.
    pub input_tokens: Option<i64>,
    /// Output token count, when the span carried token usage.
    pub output_tokens: Option<i64>,
    /// Whether the span ended in an error.
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
    /// The open SQLite connection. WAL mode + `busy_timeout` are set on open.
    conn: Connection,
    /// The database path, kept for diagnostics ([`SqliteEventStore::path`]).
    path: PathBuf,
}

impl SqliteEventStore {
    /// Open (creating if absent) a store backed by the database at `path`.
    ///
    /// On open the connection is switched to WAL journal mode, given a
    /// [`BUSY_TIMEOUT_MS`] busy timeout, and the idempotent [`SCHEMA_SQL`] is
    /// applied. The parent directory is created if it does not exist.
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
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(&path)?;
        // WAL: concurrent readers + a single writer, the harness access shape.
        // `query_row` because `journal_mode` returns the mode it settled on.
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        // A contended write waits up to BUSY_TIMEOUT_MS instead of erroring —
        // parallel hooks must not lose events to a transient lock.
        conn.busy_timeout(std::time::Duration::from_millis(u64::from(
            BUSY_TIMEOUT_MS,
        )))?;
        conn.execute_batch(SCHEMA_SQL)?;
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

    /// The `spans` projection rows for `spec`, ordered by start time.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`] for a query failure.
    pub fn spans(&self, spec: &str) -> Result<Vec<SpanRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT trace_id, span_id, parent_span_id, name, duration_ms, \
             spec, phase, model, input_tokens, output_tokens, is_error \
             FROM spans WHERE spec = ?1 ORDER BY started_at",
        )?;
        let rows = stmt.query_map(params![spec], |row| {
            Ok(SpanRow {
                trace_id: row.get(0)?,
                span_id: row.get(1)?,
                parent_span_id: row.get(2)?,
                name: row.get(3)?,
                duration_ms: row.get(4)?,
                spec: row.get(5)?,
                phase: row.get(6)?,
                model: row.get(7)?,
                input_tokens: row.get(8)?,
                output_tokens: row.get(9)?,
                // `is_error` is stored as 0/1; treat any non-zero as true.
                is_error: row.get::<_, Option<i64>>(10)?.unwrap_or(0) != 0,
            })
        })?;
        Ok(rows.filter_map(std::result::Result::ok).collect())
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
    use crate::io::event_store::EventSink;
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
        assert!(store.spans("nope").unwrap().is_empty());
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
        store
            .conn
            .execute(
                "INSERT INTO spans (span_id, name, spec, phase, is_error) \
                 VALUES ('sp-1', 'plan', '2026-spec', 'PLAN', 0)",
                [],
            )
            .unwrap();

        let specs = store.specs().unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "2026-spec");
        assert_eq!(specs[0].status.as_deref(), Some("active"));

        let metrics = store.metrics("2026-spec").unwrap().unwrap();
        assert_eq!(metrics.api_calls, Some(12));
        assert_eq!(metrics.retries, Some(3));

        let spans = store.spans("2026-spec").unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].span_id, "sp-1");
        assert!(!spans[0].is_error);
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
}
