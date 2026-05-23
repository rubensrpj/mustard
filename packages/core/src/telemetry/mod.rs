//! The dedicated telemetry domain — owns `.harness/telemetry.db`, a database
//! **independent** of the hot `mustard.db` the hooks open on every tool use.
//!
//! Single responsibility: telemetry storage + the raw read/write API. The
//! module is trait-backed for Dependency Inversion — production code talks to
//! [`TelemetryWriter`] / [`TelemetryReader`], and tests swap in the in-memory
//! [`FakeTelemetry`]. The concrete SQLite implementation is
//! [`store::TelemetryStore`].
//!
//! Tables (see `schema.sql`):
//!
//! - `usage_totals` — aggregated Claude Code OTEL counters (reduced from the
//!   legacy `claude_code_otel`).
//! - `run_usage` — per-execution token usage + cost, with load-bearing
//!   `spec` / `wave_id` / `agent_id` (replaces the legacy `spans`).
//! - `run_attribution` — write-time spec/wave/agent stamp keyed on
//!   `(session_id, tool_use_id)`.
//!
//! The one-shot [`migrate`] step builds `telemetry.db` from the data still in
//! `mustard.db` (additive this wave — the legacy tables are left in place).

pub mod migrate;
pub mod model;
pub mod reader;
pub mod store;
pub mod writer;

use crate::error::Result;

/// The only `usage_totals` metric names the dashboard ever reads (see
/// [`reader::cost_total`] / [`reader::token_total`] / [`reader::session_count`] /
/// [`reader::active_time`]). Every other Claude Code OTEL metric (hook-execution,
/// tool_decision, api_request, lines_of_code, user_prompt, …) is dead weight:
/// never queried, so never worth persisting. The collector filters ingestion
/// against this list, and the one-time startup cleanup purges anything outside
/// it — keep both in sync by referencing this single source.
pub const CONSUMED_METRICS: &[&str] = &[
    "claude_code.cost.usage",
    "claude_code.session.count",
    "claude_code.active_time.total",
    "claude_code.token.usage",
];

pub use model::{RunAttribution, RunUsage, UsageMetric};
pub use reader::{
    ConsumptionGroup, CostGroup, DailyConsumption, DailyPoint, RunRow, RunTraceRow, SummaryRow,
    WaveCostGroup,
};
pub use store::TelemetryStore;

/// Write side of the telemetry store (Dependency Inversion seam).
///
/// Every method returns [`Result`] and never panics. The SQLite implementation
/// is [`TelemetryStore`]; [`FakeTelemetry`] is the in-memory test double.
pub trait TelemetryWriter {
    /// UPSERT one aggregated usage counter (accumulates `sum`).
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn upsert_usage_metric(&self, rec: &UsageMetric) -> Result<()>;

    /// Persist a per-execution usage record (idempotent by `span_id`).
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn record_run(&self, rec: &RunUsage) -> Result<()>;

    /// UPSERT one write-time attribution stamp.
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn upsert_attribution(&self, rec: &RunAttribution) -> Result<()>;

    /// Look up the attribution stamp for a `(session_id, tool_use_id)` pair.
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn lookup_attribution(
        &self,
        session_id: &str,
        tool_use_id: &str,
    ) -> Result<Option<RunAttribution>>;

    /// Session-only attribution fallback: the most-recent stamp for
    /// `session_id` at or before `before_ts` (ms-epoch), or — when `before_ts`
    /// is `None` — the single most-recent stamp for the session. Mirrors the
    /// legacy read-time session fallback for spans that carry no `tool_use_id`.
    ///
    /// Defaulted via [`lookup_attribution`](self::TelemetryWriter::lookup_attribution)'s
    /// sibling free function so existing implementors need not change.
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn lookup_attribution_by_session(
        &self,
        _session_id: &str,
        _before_ts: Option<i64>,
    ) -> Result<Option<RunAttribution>> {
        Ok(None)
    }
}

/// Read side of the telemetry store (Dependency Inversion seam).
///
/// Mirrors the aggregations the dashboard and economy readers perform today,
/// pointed at `run_usage` / `usage_totals`.
pub trait TelemetryReader {
    /// Total summed cost across `claude_code.cost.usage`.
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn cost_total(&self) -> Result<f64>;

    /// Cost summed per model, descending.
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn cost_by_model(&self) -> Result<Vec<(String, f64)>>;

    /// Cost summed per session, descending.
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn cost_by_session(&self) -> Result<Vec<(String, f64)>>;

    /// Lifetime session count.
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn session_count(&self) -> Result<f64>;

    /// Lifetime active time in seconds.
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn active_time(&self) -> Result<f64>;

    /// `MAX(updated_at)` across `usage_totals`, or `None` when empty.
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn freshness(&self) -> Result<Option<i64>>;

    /// Cost roll-up grouped by spec.
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn runs_by_spec(&self) -> Result<Vec<CostGroup>>;

    /// Cost roll-up grouped by `(spec, wave_id)`.
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn runs_by_wave(&self) -> Result<Vec<WaveCostGroup>>;

    /// Cost roll-up grouped by agent.
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn runs_by_agent(&self) -> Result<Vec<CostGroup>>;

    /// Cost roll-up grouped by model.
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn runs_by_model(&self) -> Result<Vec<CostGroup>>;

    /// Cost roll-up grouped by phase.
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn runs_by_phase(&self) -> Result<Vec<CostGroup>>;

    /// Daily cost/token series, ascending by day.
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn daily_series(&self) -> Result<Vec<DailyPoint>>;

    /// Cache-hit ratio in permille (cache-read over total input).
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn cache_hit_ratio_permille(&self) -> Result<i64>;

    /// Every run attributed to `spec`, ordered by start time.
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn trace_by_spec(&self, spec: &str) -> Result<Vec<RunTraceRow>>;

    /// Per-run rows for the MCP `get_span_summary` aggregation, optionally
    /// filtered to a spec and/or phase and capped to `limit` rows.
    ///
    /// Defaulted to an empty result so existing implementors need not change.
    ///
    /// # Errors
    /// Returns an error when the underlying store fails.
    fn runs_for_summary(
        &self,
        _spec: Option<&str>,
        _phase: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<SummaryRow>> {
        Ok(Vec::new())
    }
}

impl TelemetryWriter for TelemetryStore {
    fn upsert_usage_metric(&self, rec: &UsageMetric) -> Result<()> {
        writer::upsert_usage_metric(self.conn(), rec)
    }
    fn record_run(&self, rec: &RunUsage) -> Result<()> {
        writer::record_run(self.conn(), rec)
    }
    fn upsert_attribution(&self, rec: &RunAttribution) -> Result<()> {
        writer::upsert_attribution(self.conn(), rec)
    }
    fn lookup_attribution(
        &self,
        session_id: &str,
        tool_use_id: &str,
    ) -> Result<Option<RunAttribution>> {
        writer::lookup_attribution(self.conn(), session_id, tool_use_id)
    }
    fn lookup_attribution_by_session(
        &self,
        session_id: &str,
        before_ts: Option<i64>,
    ) -> Result<Option<RunAttribution>> {
        writer::lookup_attribution_by_session(self.conn(), session_id, before_ts)
    }
}

impl TelemetryReader for TelemetryStore {
    fn cost_total(&self) -> Result<f64> {
        reader::cost_total(self.conn())
    }
    fn cost_by_model(&self) -> Result<Vec<(String, f64)>> {
        reader::cost_by_model(self.conn())
    }
    fn cost_by_session(&self) -> Result<Vec<(String, f64)>> {
        reader::cost_by_session(self.conn())
    }
    fn session_count(&self) -> Result<f64> {
        reader::session_count(self.conn())
    }
    fn active_time(&self) -> Result<f64> {
        reader::active_time(self.conn())
    }
    fn freshness(&self) -> Result<Option<i64>> {
        reader::freshness(self.conn())
    }
    fn runs_by_spec(&self) -> Result<Vec<CostGroup>> {
        reader::runs_by_spec(self.conn())
    }
    fn runs_by_wave(&self) -> Result<Vec<WaveCostGroup>> {
        reader::runs_by_wave(self.conn())
    }
    fn runs_by_agent(&self) -> Result<Vec<CostGroup>> {
        reader::runs_by_agent(self.conn())
    }
    fn runs_by_model(&self) -> Result<Vec<CostGroup>> {
        reader::runs_by_model(self.conn())
    }
    fn runs_by_phase(&self) -> Result<Vec<CostGroup>> {
        reader::runs_by_phase(self.conn())
    }
    fn daily_series(&self) -> Result<Vec<DailyPoint>> {
        reader::daily_series(self.conn())
    }
    fn cache_hit_ratio_permille(&self) -> Result<i64> {
        reader::cache_hit_ratio_permille(self.conn())
    }
    fn trace_by_spec(&self, spec: &str) -> Result<Vec<RunTraceRow>> {
        reader::trace_by_spec(self.conn(), spec)
    }
    fn runs_for_summary(
        &self,
        spec: Option<&str>,
        phase: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SummaryRow>> {
        reader::runs_for_summary(self.conn(), spec, phase, limit)
    }
}

/// In-memory fake implementing both telemetry traits — the test double per the
/// `core-trait-backed-io` convention.
///
/// Single-threaded (`RefCell`); intended for `#[cfg(test)]` use. It mirrors the
/// store semantics closely enough to exercise reader/writer call sites without
/// a real database: `upsert_usage_metric` accumulates `sum` on the reduced key,
/// `record_run` is idempotent by `span_id`, and the reader aggregations group
/// the recorded rows the same way the SQL does.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct FakeTelemetry {
    usage: std::cell::RefCell<Vec<UsageMetric>>,
    runs: std::cell::RefCell<Vec<RunUsage>>,
    attribution: std::cell::RefCell<Vec<RunAttribution>>,
}

#[cfg(test)]
impl FakeTelemetry {
    /// A fresh, empty fake.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
impl TelemetryWriter for FakeTelemetry {
    fn upsert_usage_metric(&self, rec: &UsageMetric) -> Result<()> {
        let mut usage = self.usage.borrow_mut();
        if let Some(existing) = usage.iter_mut().find(|u| {
            u.metric == rec.metric && u.model == rec.model && u.session_id == rec.session_id
        }) {
            existing.sum += rec.sum;
            existing.updated_at = existing.updated_at.max(rec.updated_at);
        } else {
            usage.push(rec.clone());
        }
        Ok(())
    }

    fn record_run(&self, rec: &RunUsage) -> Result<()> {
        let mut runs = self.runs.borrow_mut();
        if let Some(slot) = runs.iter_mut().find(|r| r.span_id == rec.span_id) {
            *slot = rec.clone();
        } else {
            runs.push(rec.clone());
        }
        Ok(())
    }

    fn upsert_attribution(&self, rec: &RunAttribution) -> Result<()> {
        let mut attr = self.attribution.borrow_mut();
        if let Some(slot) = attr
            .iter_mut()
            .find(|a| a.session_id == rec.session_id && a.tool_use_id == rec.tool_use_id)
        {
            *slot = rec.clone();
        } else {
            attr.push(rec.clone());
        }
        Ok(())
    }

    fn lookup_attribution(
        &self,
        session_id: &str,
        tool_use_id: &str,
    ) -> Result<Option<RunAttribution>> {
        Ok(self
            .attribution
            .borrow()
            .iter()
            .find(|a| a.session_id == session_id && a.tool_use_id == tool_use_id)
            .cloned())
    }
}

#[cfg(test)]
impl TelemetryReader for FakeTelemetry {
    fn cost_total(&self) -> Result<f64> {
        Ok(self
            .usage
            .borrow()
            .iter()
            .filter(|u| u.metric == "claude_code.cost.usage")
            .map(|u| u.sum)
            .sum())
    }

    fn cost_by_model(&self) -> Result<Vec<(String, f64)>> {
        Ok(group_usage_sum(&self.usage.borrow(), |u| {
            u.model.clone().unwrap_or_default()
        }))
    }

    fn cost_by_session(&self) -> Result<Vec<(String, f64)>> {
        Ok(group_usage_sum(&self.usage.borrow(), |u| {
            u.session_id.clone().unwrap_or_default()
        }))
    }

    fn session_count(&self) -> Result<f64> {
        Ok(self
            .usage
            .borrow()
            .iter()
            .filter(|u| u.metric == "claude_code.session.count")
            .map(|u| u.sum)
            .sum())
    }

    fn active_time(&self) -> Result<f64> {
        Ok(self
            .usage
            .borrow()
            .iter()
            .filter(|u| u.metric == "claude_code.active_time.total")
            .map(|u| u.sum)
            .sum())
    }

    fn freshness(&self) -> Result<Option<i64>> {
        Ok(self.usage.borrow().iter().filter_map(|u| u.updated_at).max())
    }

    fn runs_by_spec(&self) -> Result<Vec<CostGroup>> {
        Ok(group_runs(&self.runs.borrow(), |r| {
            r.spec.clone().unwrap_or_default()
        }))
    }

    fn runs_by_wave(&self) -> Result<Vec<WaveCostGroup>> {
        let runs = self.runs.borrow();
        let mut acc: std::collections::HashMap<(String, String), (i64, i64, i64)> =
            std::collections::HashMap::new();
        for r in runs.iter() {
            let key = (
                r.spec.clone().unwrap_or_default(),
                r.wave_id.clone().unwrap_or_default(),
            );
            let e = acc.entry(key).or_default();
            e.0 += r.cost_usd_micros.unwrap_or(0);
            e.1 += r.input_tokens.unwrap_or(0) + r.output_tokens.unwrap_or(0);
            e.2 += 1;
        }
        let mut out: Vec<WaveCostGroup> = acc
            .into_iter()
            .map(|((spec, wave_id), (cost, tokens, n))| WaveCostGroup {
                spec,
                wave_id,
                cost_usd_micros: cost,
                tokens,
                run_count: n,
            })
            .collect();
        out.sort_by(|a, b| b.cost_usd_micros.cmp(&a.cost_usd_micros));
        Ok(out)
    }

    fn runs_by_agent(&self) -> Result<Vec<CostGroup>> {
        Ok(group_runs(&self.runs.borrow(), |r| {
            r.agent_id.clone().unwrap_or_default()
        }))
    }

    fn runs_by_model(&self) -> Result<Vec<CostGroup>> {
        Ok(group_runs(&self.runs.borrow(), |r| {
            r.model.clone().unwrap_or_default()
        }))
    }

    fn runs_by_phase(&self) -> Result<Vec<CostGroup>> {
        Ok(group_runs(&self.runs.borrow(), |r| {
            r.phase.clone().unwrap_or_default()
        }))
    }

    fn daily_series(&self) -> Result<Vec<DailyPoint>> {
        let runs = self.runs.borrow();
        let mut acc: std::collections::BTreeMap<String, (i64, i64)> =
            std::collections::BTreeMap::new();
        for r in runs.iter() {
            let Some(ts) = r.ts_iso.as_deref() else {
                continue;
            };
            if ts.len() < 10 {
                continue;
            }
            let e = acc.entry(ts[..10].to_string()).or_default();
            e.0 += r.cost_usd_micros.unwrap_or(0);
            e.1 += r.input_tokens.unwrap_or(0) + r.output_tokens.unwrap_or(0);
        }
        Ok(acc
            .into_iter()
            .map(|(day, (cost, tokens))| DailyPoint {
                day,
                cost_usd_micros: cost,
                tokens,
            })
            .collect())
    }

    fn cache_hit_ratio_permille(&self) -> Result<i64> {
        let runs = self.runs.borrow();
        let input: i64 = runs.iter().map(|r| r.input_tokens.unwrap_or(0)).sum();
        let cache: i64 = runs
            .iter()
            .map(|r| r.cache_read_input_tokens.unwrap_or(0))
            .sum();
        let den = input + cache;
        if den <= 0 {
            Ok(0)
        } else {
            #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
            Ok(((cache as f64) * 1000.0 / (den as f64)) as i64)
        }
    }

    fn trace_by_spec(&self, spec: &str) -> Result<Vec<RunTraceRow>> {
        let mut rows: Vec<RunUsage> = self
            .runs
            .borrow()
            .iter()
            .filter(|r| r.spec.as_deref() == Some(spec))
            .cloned()
            .collect();
        rows.sort_by_key(|r| r.started_at.unwrap_or(0));
        Ok(rows
            .into_iter()
            .map(|r| RunTraceRow {
                span_id: r.span_id,
                name: r.name,
                phase: r.phase,
                model: r.model,
                duration_ms: r.duration_ms,
                cost_usd_micros: r.cost_usd_micros,
            })
            .collect())
    }

    fn runs_for_summary(
        &self,
        spec: Option<&str>,
        phase: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SummaryRow>> {
        let mut rows: Vec<RunUsage> = self
            .runs
            .borrow()
            .iter()
            .filter(|r| spec.is_none_or(|s| r.spec.as_deref() == Some(s)))
            .filter(|r| phase.is_none_or(|p| r.phase.as_deref() == Some(p)))
            .cloned()
            .collect();
        rows.sort_by_key(|r| r.started_at.unwrap_or(0));
        rows.truncate(limit);
        Ok(rows
            .into_iter()
            .map(|r| SummaryRow {
                model: r.model,
                input_tokens: r.input_tokens,
                output_tokens: r.output_tokens,
                duration_ms: r.duration_ms,
            })
            .collect())
    }
}

/// Sum `usage.sum` grouped by a key extractor; used by the fake's per-model /
/// per-session readers. Only `claude_code.cost.usage` rows participate, to
/// match the SQL `WHERE metric = 'claude_code.cost.usage'`.
#[cfg(test)]
fn group_usage_sum(
    usage: &[UsageMetric],
    key: impl Fn(&UsageMetric) -> String,
) -> Vec<(String, f64)> {
    let mut acc: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for u in usage.iter().filter(|u| u.metric == "claude_code.cost.usage") {
        *acc.entry(key(u)).or_insert(0.0) += u.sum;
    }
    let mut out: Vec<(String, f64)> = acc.into_iter().collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Cost roll-up over `run_usage` grouped by a key extractor, descending by cost.
#[cfg(test)]
fn group_runs(runs: &[RunUsage], key: impl Fn(&RunUsage) -> String) -> Vec<CostGroup> {
    // (cost, tokens, count, MAX(started_at)) — the last slot mirrors the SQL
    // `MAX(started_at)` the real reader emits.
    let mut acc: std::collections::HashMap<String, (i64, i64, i64, Option<i64>)> =
        std::collections::HashMap::new();
    for r in runs.iter() {
        let e = acc.entry(key(r)).or_default();
        e.0 += r.cost_usd_micros.unwrap_or(0);
        e.1 += r.input_tokens.unwrap_or(0) + r.output_tokens.unwrap_or(0);
        e.2 += 1;
        e.3 = match (e.3, r.started_at) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
    }
    let mut out: Vec<CostGroup> = acc
        .into_iter()
        .map(|(key, (cost, tokens, n, last_started_at))| CostGroup {
            key,
            cost_usd_micros: cost,
            tokens,
            run_count: n,
            last_started_at,
        })
        .collect();
    out.sort_by(|a, b| b.cost_usd_micros.cmp(&a.cost_usd_micros));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(metric: &str, model: Option<&str>, session: Option<&str>, sum: f64, at: i64) -> UsageMetric {
        UsageMetric {
            metric: metric.into(),
            model: model.map(Into::into),
            session_id: session.map(Into::into),
            sum,
            updated_at: Some(at),
        }
    }

    fn run(span: &str, spec: &str, agent: &str, cost: i64, input: i64, cache: i64) -> RunUsage {
        RunUsage {
            trace_id: None,
            span_id: span.into(),
            parent_span_id: None,
            name: None,
            started_at: Some(0),
            ended_at: None,
            duration_ms: None,
            attributes: None,
            spec: Some(spec.into()),
            phase: Some("EXECUTE".into()),
            model: Some("opus".into()),
            input_tokens: Some(input),
            output_tokens: Some(0),
            cache_read_input_tokens: Some(cache),
            cache_creation_input_tokens: None,
            cost_usd_micros: Some(cost),
            is_error: false,
            project_path: None,
            ts_iso: Some("2026-05-22T00:00:00Z".into()),
            session_id: Some("s1".into()),
            wave_id: Some("w1".into()),
            tool_use_id: None,
            agent_id: Some(agent.into()),
        }
    }

    #[test]
    fn fake_writer_and_reader_round_trip() {
        let fake = FakeTelemetry::new();
        // usage_totals accumulation on the reduced key.
        fake.upsert_usage_metric(&usage("claude_code.cost.usage", Some("opus"), Some("s1"), 10.0, 100))
            .unwrap();
        fake.upsert_usage_metric(&usage("claude_code.cost.usage", Some("opus"), Some("s1"), 5.0, 200))
            .unwrap();
        fake.upsert_usage_metric(&usage("claude_code.session.count", None, None, 3.0, 50))
            .unwrap();

        assert!((fake.cost_total().unwrap() - 15.0).abs() < f64::EPSILON);
        assert!((fake.session_count().unwrap() - 3.0).abs() < f64::EPSILON);
        assert_eq!(fake.freshness().unwrap(), Some(200));
        assert_eq!(fake.cost_by_model().unwrap(), vec![("opus".into(), 15.0)]);

        // run_usage roll-ups.
        fake.record_run(&run("r1", "spec-A", "core-impl", 1000, 800, 200)).unwrap();
        fake.record_run(&run("r2", "spec-A", "explore", 2000, 400, 0)).unwrap();
        let by_spec = fake.runs_by_spec().unwrap();
        assert_eq!(by_spec.len(), 1);
        assert_eq!(by_spec[0].key, "spec-A");
        assert_eq!(by_spec[0].cost_usd_micros, 3000);
        assert_eq!(by_spec[0].run_count, 2);

        let by_agent = fake.runs_by_agent().unwrap();
        // Ordered by cost desc: explore (2000) before core-impl (1000).
        assert_eq!(by_agent[0].key, "explore");

        // cache ratio: cache 200 / (input 1200 + cache 200) = 142 permille.
        assert_eq!(fake.cache_hit_ratio_permille().unwrap(), 142);

        let trace = fake.trace_by_spec("spec-A").unwrap();
        assert_eq!(trace.len(), 2);
    }

    #[test]
    fn fake_record_run_is_idempotent_by_span_id() {
        let fake = FakeTelemetry::new();
        fake.record_run(&run("r1", "spec-A", "a", 1000, 100, 0)).unwrap();
        fake.record_run(&run("r1", "spec-A", "a", 5000, 100, 0)).unwrap();
        let by_spec = fake.runs_by_spec().unwrap();
        assert_eq!(by_spec[0].run_count, 1);
        assert_eq!(by_spec[0].cost_usd_micros, 5000);
    }

    #[test]
    fn fake_attribution_round_trip() {
        let fake = FakeTelemetry::new();
        fake.upsert_attribution(&RunAttribution {
            session_id: "s1".into(),
            tool_use_id: "tu1".into(),
            spec: Some("spec-A".into()),
            wave_id: Some("w1".into()),
            agent_id: Some("core-impl".into()),
            updated_at: Some(10),
        })
        .unwrap();
        let got = fake.lookup_attribution("s1", "tu1").unwrap().unwrap();
        assert_eq!(got.spec.as_deref(), Some("spec-A"));
        assert!(fake.lookup_attribution("s1", "missing").unwrap().is_none());
    }
}
