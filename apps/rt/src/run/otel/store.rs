//! SQLite store for the OTEL ports.
//!
//! As of Wave 2 (telemetry-separation) the collector writes telemetry into the
//! dedicated `.claude/.harness/telemetry.db` (the `mustard_core::telemetry`
//! domain), **not** the hot `mustard.db` the hooks open on every tool use.
//! Metrics and logs are reduced onto `usage_totals` (one row per
//! `(metric, model, session_id)`, accumulating `sum`); the legacy per-minute
//! `claude_code_otel` bucket / `token_type` / `attrs` / `count` / `signal`
//! columns are dropped.
//!
//! `subtractions_since` still reads the `events` table, which lives in
//! `mustard.db`; that one query opens the harness store directly.

use mustard_core::fs;
use mustard_core::telemetry::model::UsageMetric;
use mustard_core::telemetry::{writer as telemetry_writer, TelemetryStore};
use mustard_core::ClaudePaths;
use std::path::{Path, PathBuf};

/// One projected metric datapoint — the argument bundle for [`Store::upsert_metric`].
///
/// Reduced for Wave 2: only `(metric, model, session_id, sum)` survive plus the
/// `ts_bucket` (now used solely as the `updated_at` freshness signal, not a
/// primary-key bucket). `token_type` / `attrs` are gone.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricRow {
    /// Minute-floored ms-epoch — now the `updated_at` freshness signal.
    pub ts_bucket: i64,
    /// OTLP metric name, e.g. `claude_code.token.usage`.
    pub metric: String,
    /// `session.id` attribute, if present.
    pub session_id: Option<String>,
    /// `model` attribute, if present.
    pub model: Option<String>,
    /// `type` attribute (only on token.usage), if present. Retained on the
    /// projection struct for the `project` walker's compatibility, but no
    /// longer persisted (the reduced schema drops it).
    pub token_type: Option<String>,
    /// The datapoint's numeric value.
    pub sum: f64,
    /// JSON of the remaining (non-projected) attributes. Retained on the
    /// projection struct but no longer persisted.
    pub attrs: String,
}

/// A handle on the dedicated `telemetry.db` (`usage_totals`), plus the
/// project root for the on-demand `mustard.db` open `subtractions_since` needs.
pub struct Store {
    telemetry: TelemetryStore,
    /// Project's `.claude` dir. Retained for the future
    /// `subtractions_since` re-implementation that will walk the per-spec
    /// NDJSON sink (today the method is a W5 stub returning 0 — see
    /// [`Self::subtractions_since`]).
    #[allow(dead_code)]
    claude_dir: PathBuf,
}

impl Store {
    /// Open (creating if absent) the telemetry store under `<claude_dir>`.
    ///
    /// # Errors
    /// Returns the `rusqlite` error when the directory cannot be created, the
    /// database cannot be opened, or the schema DDL fails.
    pub fn open(claude_dir: &Path) -> rusqlite::Result<Self> {
        let harness_dir = claude_dir.join(".harness");
        // A directory-create failure is surfaced as a generic SQLite error so
        // the caller has a single error type to fail open against.
        if fs::create_dir_all(&harness_dir).is_err() {
            return Err(rusqlite::Error::InvalidPath(harness_dir));
        }
        let telemetry = telemetry_open(claude_dir)?;
        Ok(Self {
            telemetry,
            claude_dir: claude_dir.to_path_buf(),
        })
    }

    /// Open against an explicit project's `.harness` directory — used by the
    /// inline tests with a `tempfile` database. `db_path` points at the legacy
    /// `mustard.db` slot; the telemetry store is opened beside it.
    ///
    /// # Errors
    /// Returns the `rusqlite` error when the database cannot be opened or the
    /// schema DDL fails.
    #[cfg(test)]
    pub fn open_at(db_path: &Path) -> rusqlite::Result<Self> {
        // `db_path` is `<harness>/mustard.db`; the telemetry db sits beside it.
        let harness_dir = db_path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let telemetry = TelemetryStore::new(harness_dir.join("telemetry.db"))
            .map_err(|_| rusqlite::Error::InvalidPath(harness_dir.clone()))?;
        Ok(Self {
            telemetry,
            // `claude_dir` is the parent of `.harness`; tests that exercise
            // `subtractions_since` pass a real `.harness/mustard.db` path.
            claude_dir: harness_dir
                .parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
        })
    }

    /// UPSERT one metric datapoint into `usage_totals` (reduced schema).
    ///
    /// On a `(metric, model, session_id)` collision `sum` is accumulated and
    /// `updated_at` advances to the latest contributing datapoint.
    ///
    /// # Errors
    /// Returns the `rusqlite` error when the statement fails.
    pub fn upsert_metric(&self, row: &MetricRow) -> rusqlite::Result<()> {
        // Ingestion filter: persist only the metrics the dashboard actually
        // reads (`telemetry::CONSUMED_METRICS`). Every other Claude Code OTEL
        // metric is dropped here — silently `Ok`, never written — so the ~9
        // unread metric types stop accumulating dead rows in `usage_totals`.
        if !mustard_core::telemetry::CONSUMED_METRICS.contains(&row.metric.as_str()) {
            return Ok(());
        }
        telemetry_writer::upsert_usage_metric(
            self.telemetry.conn(),
            &UsageMetric {
                metric: row.metric.clone(),
                model: row.model.clone(),
                session_id: row.session_id.clone(),
                sum: row.sum,
                updated_at: Some(row.ts_bucket),
            },
        )
        .map_err(to_sqlite_err)
    }

    /// One-time cleanup: delete every `usage_totals` row whose `metric` is not in
    /// `telemetry::CONSUMED_METRICS`. Purges the ~120 already-written irrelevant
    /// rows from before the ingestion filter existed. Idempotent — re-running
    /// after the filter is in place deletes nothing. Shares the allowlist with
    /// [`Store::upsert_metric`] so the filter and the purge can never drift.
    ///
    /// # Errors
    /// Returns the `rusqlite` error when the statement fails.
    pub fn purge_unconsumed_metrics(&self) -> rusqlite::Result<usize> {
        // Build the `NOT IN (?, ?, …)` placeholder list from the shared
        // allowlist so adding/removing a consumed metric needs no edit here.
        let allow = mustard_core::telemetry::CONSUMED_METRICS;
        let placeholders = vec!["?"; allow.len()].join(", ");
        let sql = format!("DELETE FROM usage_totals WHERE metric NOT IN ({placeholders})");
        let params: Vec<&dyn rusqlite::ToSql> =
            allow.iter().map(|m| m as &dyn rusqlite::ToSql).collect();
        self.telemetry
            .conn()
            .execute(&sql, params.as_slice())
    }

    /// Total `usage_totals` row count.
    ///
    /// # Errors
    /// Returns the `rusqlite` error when the query fails.
    pub fn otel_row_count(&self) -> rusqlite::Result<i64> {
        self.telemetry
            .conn()
            .query_row("SELECT COUNT(*) FROM usage_totals", [], |r| r.get(0))
    }

    /// `MAX(updated_at)` — the latest contributing datapoint, or `None` when the
    /// table is empty.
    ///
    /// # Errors
    /// Returns the `rusqlite` error when the query fails.
    pub fn otel_last_bucket(&self) -> rusqlite::Result<Option<i64>> {
        self.telemetry
            .conn()
            .query_row("SELECT MAX(updated_at) FROM usage_totals", [], |r| r.get(0))
    }

    /// The latest five rows by `updated_at`, newest first — the diagnose
    /// `[data]` sample, adjusted to the reduced schema.
    ///
    /// # Errors
    /// Returns the `rusqlite` error when the query fails.
    pub fn otel_sample(&self) -> rusqlite::Result<Vec<SampleRow>> {
        let conn = self.telemetry.conn();
        let mut stmt = conn.prepare(
            "SELECT metric, session_id, model, sum, updated_at \
             FROM usage_totals ORDER BY updated_at DESC LIMIT 5",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(SampleRow {
                metric: r.get(0)?,
                session_id: r.get(1)?,
                model: r.get(2)?,
                sum: r.get(3)?,
                updated_at: r.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// Count `mustard.subtraction.applied` events whose `ts` is newer than the
    /// ISO-8601 `since` timestamp.
    ///
    /// W5: the high-volume `events` table is retired. `mustard.subtraction.applied`
    /// is a non-pipeline event family that now lands in per-spec NDJSON files;
    /// the diagnose-otel summary that called this helper does not yet walk the
    /// NDJSON sink (counting subtractions cross-spec is out of scope of the OTEL
    /// health probe). Returns `Ok(0)` instead — the field stays in the JSON
    /// shape but always reads zero. A future probe that wants the count should
    /// walk `.claude/spec/<spec>/events/*.ndjson` directly.
    ///
    /// # Errors
    /// Never. Returns `Ok(0)` unconditionally — kept fallible for API stability
    /// with the pre-W5 signature so the caller's `match` arms (and the
    /// eventual NDJSON-walking re-implementation, which WILL be fallible)
    /// stay unchanged.
    #[allow(clippy::unnecessary_wraps)]
    pub fn subtractions_since(&self, since_iso: &str) -> rusqlite::Result<i64> {
        let _ = since_iso;
        Ok(0)
    }
}

/// Open the telemetry store for a `.claude` dir, mapping the core error onto a
/// `rusqlite` error so the OTEL ports keep their single error type.
fn telemetry_open(claude_dir: &Path) -> rusqlite::Result<TelemetryStore> {
    let path = claude_dir.join(".harness").join("telemetry.db");
    TelemetryStore::new(&path).map_err(|_| rusqlite::Error::InvalidPath(path))
}

/// Collapse a core telemetry error onto a `rusqlite` error. The OTEL ports fail
/// open on any store error, so the specific variant is not load-bearing.
fn to_sqlite_err(_e: mustard_core::error::Error) -> rusqlite::Error {
    rusqlite::Error::ExecuteReturnedResults
}

/// A single `[data]` sample row, adjusted to the reduced `usage_totals` schema.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleRow {
    /// OTLP metric name (or log body).
    pub metric: String,
    /// `session.id` attribute.
    pub session_id: Option<String>,
    /// `model` attribute.
    pub model: Option<String>,
    /// Aggregated value across every contributing datapoint.
    pub sum: f64,
    /// ms-epoch of the most recent contributing datapoint.
    pub updated_at: Option<i64>,
}

/// Resolve `<project>/.claude/.harness/...`'s parent `.claude` dir.
///
/// Routed through `ClaudePaths` so the I1 guard fires at the boundary; a
/// rejection collapses to an empty `PathBuf` (the caller's downstream IO will
/// then degrade gracefully — every harness path is a fail-open read).
#[must_use]
pub fn claude_dir() -> PathBuf {
    ClaudePaths::for_project(PathBuf::from(crate::run::env::project_dir()))
        .map(|p| p.claude_dir())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_store() -> Store {
        let tmp = tempfile::tempdir().unwrap();
        let harness = tmp.path().join(".harness");
        std::fs::create_dir_all(&harness).unwrap();
        let store = Store::open_at(&harness.join("mustard.db")).unwrap();
        // Keep the tempdir alive for the store's lifetime by leaking it — the
        // test process is short-lived and this avoids threading a guard out.
        std::mem::forget(tmp);
        store
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
    fn metric_upsert_accumulates_on_reduced_key() {
        let store = mem_store();
        store.upsert_metric(&metric(60_000, 10.0)).unwrap();
        store.upsert_metric(&metric(120_000, 5.0)).unwrap();
        // Same (metric, model, session_id) → one row, sum accumulated, the
        // freshness signal advances to the latest bucket.
        assert_eq!(store.otel_row_count().unwrap(), 1);
        let sample = store.otel_sample().unwrap();
        assert_eq!(sample.len(), 1);
        assert!((sample[0].sum - 15.0).abs() < f64::EPSILON);
        assert_eq!(store.otel_last_bucket().unwrap(), Some(120_000));
    }

    #[test]
    fn distinct_session_keys_are_separate_rows() {
        let store = mem_store();
        store.upsert_metric(&metric(60_000, 10.0)).unwrap();
        let mut other = metric(60_000, 7.0);
        other.session_id = Some("s2".to_string());
        store.upsert_metric(&other).unwrap();
        assert_eq!(store.otel_row_count().unwrap(), 2);
    }

    #[test]
    fn unconsumed_metric_is_dropped_on_ingestion() {
        let store = mem_store();
        let mut row = metric(60_000, 10.0);
        row.metric = "claude_code.hook_execution_start".to_string();
        // Outside CONSUMED_METRICS → silently dropped, no row written.
        store.upsert_metric(&row).unwrap();
        assert_eq!(store.otel_row_count().unwrap(), 0);
    }

    #[test]
    fn purge_removes_only_unconsumed_rows() {
        let store = mem_store();
        // A consumed metric survives; an unconsumed one is removed. We bypass
        // the ingestion filter for the unconsumed row by writing it directly,
        // simulating a pre-filter legacy row.
        store.upsert_metric(&metric(60_000, 10.0)).unwrap();
        telemetry_writer::upsert_usage_metric(
            store.telemetry.conn(),
            &UsageMetric {
                metric: "claude_code.api_request".to_string(),
                model: Some("opus".to_string()),
                session_id: Some("s9".to_string()),
                sum: 1.0,
                updated_at: Some(60_000),
            },
        )
        .unwrap();
        assert_eq!(store.otel_row_count().unwrap(), 2);
        let removed = store.purge_unconsumed_metrics().unwrap();
        assert_eq!(removed, 1);
        assert_eq!(store.otel_row_count().unwrap(), 1);
        // Idempotent: a second purge removes nothing.
        assert_eq!(store.purge_unconsumed_metrics().unwrap(), 0);
    }

    #[test]
    fn empty_table_reports_zero_and_no_bucket() {
        let store = mem_store();
        assert_eq!(store.otel_row_count().unwrap(), 0);
        assert_eq!(store.otel_last_bucket().unwrap(), None);
        assert!(store.otel_sample().unwrap().is_empty());
    }
}
