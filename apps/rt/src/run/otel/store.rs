//! SQLite store for the OTEL ports — a `rusqlite` handle on the harness
//! `mustard.db`.
//!
//! The JS `otel-collector.js` / `diagnose-otel.js` reached the database
//! through `_lib/event-store.js`, the CJS wrapper around the compiled
//! `EventStore` class. The Rust ports cannot load that class, so this module
//! opens the same `.claude/.harness/mustard.db` file directly with `rusqlite`.
//!
//! The schema MUST stay byte-identical to `SCHEMA_SQL` in
//! `packages/cli/src/runtime/event-store.ts` (and its mirror
//! `runtime/schema.sql`) so a database created by either runtime is
//! interchangeable — same columns, same types, and the same composite
//! `PRIMARY KEY (ts_bucket, metric, session_id, model, token_type)`, which is
//! the `ON CONFLICT` target the UPSERTs depend on.

use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

/// The `claude_code_otel` table plus its three indexes, copied verbatim from
/// `event-store.ts`'s `SCHEMA_SQL`. `IF NOT EXISTS` makes it idempotent: when
/// the JS `EventStore` already created the table this is a no-op, and when it
/// has not, the columns match exactly so either runtime can populate it.
const OTEL_SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS claude_code_otel (
  ts_bucket INTEGER NOT NULL,
  signal TEXT NOT NULL,
  metric TEXT NOT NULL,
  session_id TEXT,
  model TEXT,
  token_type TEXT,
  sum REAL DEFAULT 0,
  count INTEGER DEFAULT 0,
  attrs TEXT,
  PRIMARY KEY (ts_bucket, metric, session_id, model, token_type)
);
CREATE INDEX IF NOT EXISTS idx_cco_metric ON claude_code_otel(metric);
CREATE INDEX IF NOT EXISTS idx_cco_session ON claude_code_otel(session_id);
CREATE INDEX IF NOT EXISTS idx_cco_bucket ON claude_code_otel(ts_bucket);
";

/// One projected metric datapoint — the argument bundle for [`Store::upsert_metric`].
#[derive(Debug, Clone, PartialEq)]
pub struct MetricRow {
    /// Minute-floored ms-epoch bucket.
    pub ts_bucket: i64,
    /// OTLP metric name, e.g. `claude_code.token.usage`.
    pub metric: String,
    /// `session.id` attribute, if present.
    pub session_id: Option<String>,
    /// `model` attribute, if present.
    pub model: Option<String>,
    /// `type` attribute (only on token.usage), if present.
    pub token_type: Option<String>,
    /// The datapoint's numeric value.
    pub sum: f64,
    /// JSON of the remaining (non-projected) attributes.
    pub attrs: String,
}

/// One projected log record — the argument bundle for [`Store::upsert_log`].
#[derive(Debug, Clone, PartialEq)]
pub struct LogRow {
    /// Minute-floored ms-epoch bucket.
    pub ts_bucket: i64,
    /// The log's `body.stringValue`, or `"log"` when absent.
    pub metric: String,
    /// `session.id` attribute, if present.
    pub session_id: Option<String>,
    /// `model` attribute, if present.
    pub model: Option<String>,
    /// JSON of all the log's attributes.
    pub attrs: String,
}

/// A `rusqlite` handle on the harness `mustard.db`.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (creating if absent) `<claude_dir>/.harness/mustard.db` and ensure
    /// the `claude_code_otel` schema exists.
    ///
    /// # Errors
    /// Returns the `rusqlite` error when the directory cannot be created, the
    /// database cannot be opened, or the schema DDL fails.
    pub fn open(claude_dir: &Path) -> rusqlite::Result<Self> {
        let harness_dir = claude_dir.join(".harness");
        // A directory-create failure is surfaced as a generic SQLite error so
        // the caller has a single error type to fail open against.
        if std::fs::create_dir_all(&harness_dir).is_err() {
            return Err(rusqlite::Error::InvalidPath(harness_dir));
        }
        let db_path = harness_dir.join("mustard.db");
        let conn = Connection::open(&db_path)?;
        conn.execute_batch(OTEL_SCHEMA_SQL)?;
        Ok(Self { conn })
    }

    /// Open against an explicit database path — used by the inline tests with
    /// an in-memory or `tempfile` database.
    ///
    /// # Errors
    /// Returns the `rusqlite` error when the database cannot be opened or the
    /// schema DDL fails.
    #[cfg(test)]
    pub fn open_at(db_path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(OTEL_SCHEMA_SQL)?;
        Ok(Self { conn })
    }

    /// UPSERT one metric datapoint into the 1-minute bucket.
    ///
    /// On a `(ts_bucket, metric, session_id, model, token_type)` collision the
    /// `sum` and `count` are accumulated — identical to the JS
    /// `upsertMetricStmt` (`sum = sum + excluded.sum`,
    /// `count = count + excluded.count`).
    ///
    /// # Errors
    /// Returns the `rusqlite` error when the statement fails.
    pub fn upsert_metric(&self, row: &MetricRow) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO claude_code_otel
               (ts_bucket, signal, metric, session_id, model, token_type, sum, count, attrs)
             VALUES (?1, 'metric', ?2, ?3, ?4, ?5, ?6, 1, ?7)
             ON CONFLICT(ts_bucket, metric, session_id, model, token_type)
             DO UPDATE SET sum = sum + excluded.sum, count = count + excluded.count",
            params![
                row.ts_bucket,
                row.metric,
                row.session_id,
                row.model,
                row.token_type,
                row.sum,
                row.attrs,
            ],
        )?;
        Ok(())
    }

    /// UPSERT one log record into the 1-minute bucket.
    ///
    /// On a key collision only `count` is incremented (logs carry no `sum`) —
    /// identical to the JS `upsertLogStmt`.
    ///
    /// # Errors
    /// Returns the `rusqlite` error when the statement fails.
    pub fn upsert_log(&self, row: &LogRow) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO claude_code_otel
               (ts_bucket, signal, metric, session_id, model, token_type, sum, count, attrs)
             VALUES (?1, 'log', ?2, ?3, ?4, NULL, 0, 1, ?5)
             ON CONFLICT(ts_bucket, metric, session_id, model, token_type)
             DO UPDATE SET count = count + 1",
            params![
                row.ts_bucket,
                row.metric,
                row.session_id,
                row.model,
                row.attrs,
            ],
        )?;
        Ok(())
    }

    /// Total `claude_code_otel` row count.
    ///
    /// # Errors
    /// Returns the `rusqlite` error when the query fails.
    pub fn otel_row_count(&self) -> rusqlite::Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM claude_code_otel", [], |r| r.get(0))
    }

    /// `MAX(ts_bucket)` — the latest minute that has rows, or `None` when the
    /// table is empty.
    ///
    /// # Errors
    /// Returns the `rusqlite` error when the query fails.
    pub fn otel_last_bucket(&self) -> rusqlite::Result<Option<i64>> {
        self.conn
            .query_row("SELECT MAX(ts_bucket) FROM claude_code_otel", [], |r| {
                r.get(0)
            })
    }

    /// The latest five rows, newest first — the diagnose `[data]` sample.
    ///
    /// # Errors
    /// Returns the `rusqlite` error when the query fails.
    pub fn otel_sample(&self) -> rusqlite::Result<Vec<SampleRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT ts_bucket, metric, session_id, model, token_type, sum, count
             FROM claude_code_otel ORDER BY ts_bucket DESC LIMIT 5",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(SampleRow {
                ts_bucket: r.get(0)?,
                metric: r.get(1)?,
                session_id: r.get(2)?,
                model: r.get(3)?,
                token_type: r.get(4)?,
                sum: r.get(5)?,
                count: r.get(6)?,
            })
        })?;
        rows.collect()
    }

    /// Count `events` rows with `event = 'mustard.subtraction.applied'` whose
    /// `ts` is newer than the ISO-8601 `since` timestamp.
    ///
    /// Mirrors the JS `checkSubtractions`. The `events` table is part of the
    /// shared `EventStore` schema; this query is read-only and tolerant — if
    /// the table is missing (a database the JS store never initialised) the
    /// `rusqlite` error propagates and the caller reports it fail-open.
    ///
    /// # Errors
    /// Returns the `rusqlite` error when the query fails (e.g. no `events`
    /// table).
    pub fn subtractions_since(&self, since_iso: &str) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT COUNT(*) FROM events
             WHERE event = 'mustard.subtraction.applied' AND ts > ?1",
            params![since_iso],
            |r| r.get(0),
        )
    }
}

/// A single `[data]` sample row, mirroring the JS diagnose sample shape.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleRow {
    /// Minute-floored ms-epoch bucket.
    pub ts_bucket: i64,
    /// OTLP metric name (or log body).
    pub metric: String,
    /// `session.id` attribute.
    pub session_id: Option<String>,
    /// `model` attribute.
    pub model: Option<String>,
    /// `type` attribute (token.usage only).
    pub token_type: Option<String>,
    /// Aggregated value within the bucket.
    pub sum: f64,
    /// Number of datapoints summed.
    pub count: i64,
}

/// Resolve `<project>/.claude/.harness/mustard.db`'s parent `.claude` dir.
#[must_use]
pub fn claude_dir() -> PathBuf {
    PathBuf::from(crate::run::env::project_dir()).join(".claude")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_store() -> Store {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(OTEL_SCHEMA_SQL).unwrap();
        Store { conn }
    }

    fn metric(bucket: i64, sum: f64) -> MetricRow {
        MetricRow {
            ts_bucket: bucket,
            metric: "claude_code.token.usage".to_string(),
            session_id: Some("s1".to_string()),
            model: Some("opus".to_string()),
            token_type: Some("input".to_string()),
            sum,
            attrs: "{}".to_string(),
        }
    }

    #[test]
    fn metric_upsert_aggregates_within_bucket() {
        let store = mem_store();
        store.upsert_metric(&metric(60_000, 10.0)).unwrap();
        store.upsert_metric(&metric(60_000, 5.0)).unwrap();
        // Same composite key → one row, sum accumulated, count incremented.
        assert_eq!(store.otel_row_count().unwrap(), 1);
        let sample = store.otel_sample().unwrap();
        assert_eq!(sample.len(), 1);
        assert!((sample[0].sum - 15.0).abs() < f64::EPSILON);
        assert_eq!(sample[0].count, 2);
    }

    #[test]
    fn metric_distinct_buckets_are_separate_rows() {
        let store = mem_store();
        store.upsert_metric(&metric(60_000, 10.0)).unwrap();
        store.upsert_metric(&metric(120_000, 7.0)).unwrap();
        assert_eq!(store.otel_row_count().unwrap(), 2);
        assert_eq!(store.otel_last_bucket().unwrap(), Some(120_000));
    }

    #[test]
    fn log_upsert_inserts_zero_sum_rows() {
        // A log row carries `sum = 0` and `count = 1`. Note: `upsert_log`
        // always writes `token_type = NULL`, and SQLite treats NULL as
        // distinct inside a UNIQUE/PRIMARY KEY — so the log `ON CONFLICT`
        // never actually fires and every log is a fresh row. This is
        // faithful to the JS `upsertLogStmt`, which has the identical PK and
        // the identical effective behaviour. The aggregating `count = count
        // + 1` path therefore only matters for a hypothetical non-NULL
        // `token_type` log; see the metric path for the real aggregation.
        let store = mem_store();
        let log = LogRow {
            ts_bucket: 60_000,
            metric: "claude_code.api_request".to_string(),
            session_id: Some("s1".to_string()),
            model: Some("opus".to_string()),
            attrs: "{}".to_string(),
        };
        store.upsert_log(&log).unwrap();
        store.upsert_log(&log).unwrap();
        let sample = store.otel_sample().unwrap();
        assert_eq!(sample.len(), 2);
        for row in &sample {
            assert_eq!(row.count, 1);
            assert!(row.sum.abs() < f64::EPSILON);
        }
    }

    #[test]
    fn empty_table_reports_zero_and_no_bucket() {
        let store = mem_store();
        assert_eq!(store.otel_row_count().unwrap(), 0);
        assert_eq!(store.otel_last_bucket().unwrap(), None);
        assert!(store.otel_sample().unwrap().is_empty());
    }

    #[test]
    fn open_at_creates_interchangeable_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("mustard.db");
        {
            let store = Store::open_at(&db).unwrap();
            store.upsert_metric(&metric(60_000, 3.0)).unwrap();
        }
        // Re-open the same file: schema is idempotent, data persists.
        let reopened = Store::open_at(&db).unwrap();
        assert_eq!(reopened.otel_row_count().unwrap(), 1);
    }
}
