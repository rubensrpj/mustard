//! Telemetry aggregation functions.
//!
//! Wave 6A of [[2026-05-26-no-sqlite-git-source-of-truth]] retired the
//! SQLite query plane that fed these aggregations. The public function
//! signatures are preserved so call sites in `lib.rs` continue to type-check,
//! but the bodies fail-open to empty / zero-valued payloads. Faithful
//! NDJSON-backed reimplementations are tracked in the W6B sub-spec
//! (wave-20-dashboard) where the dedicated telemetry rewrite lives.

use crate::db::Connection;
use serde::{Deserialize, Serialize};

// ── Shapes ──────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct PhaseSummary {
    pub phase: String,
    pub events_count: i64,
    pub last_event_at: Option<String>,
    /// Event counts per day, last 7 days (oldest first, 7 slots).
    pub sparkline: Vec<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct TimelineEvent {
    pub id: String,
    pub ts: String,
    pub phase: Option<String>,
    pub spec: Option<String>,
    pub agent: Option<String>,
    pub summary: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct HeatmapCell {
    /// 0 = Sunday … 6 = Saturday.
    pub day_of_week: i64,
    /// 0–23
    pub hour: i64,
    pub event_count: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct HistoryEntry {
    pub spec: String,
    pub status: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    /// phase label → cumulative event count for that phase
    pub duration_per_phase: std::collections::HashMap<String, i64>,
    pub ac_passed: i64,
    pub ac_total: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct AcceptanceCriterion {
    pub spec: String,
    pub id: String,
    pub status: String,
    pub last_run_at: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct FileCount {
    pub path: String,
    pub count: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct ToolUseCount {
    pub name: String,
    pub count: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct PhaseEventCount {
    pub phase: String,
    pub duration_ms: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct AgentTypeCount {
    pub agent_type: String,
    pub count: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct EffortBreakdown {
    pub top_files: Vec<FileCount>,
    pub top_tools: Vec<ToolUseCount>,
    pub top_phases: Vec<PhaseEventCount>,
    pub top_agents: Vec<AgentTypeCount>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct AgentDispatch {
    pub subagent_type: String,
    pub count: i64,
    pub error_count: i64,
    pub avg_duration_ms: i64,
    pub last_dispatched_at: Option<String>,
}

// ── Stubs (closures unreachable post-Wave-6A) ───────────────────────────────

pub fn telemetry_phases(
    _conn: &Connection,
    _time_range: &str,
) -> Result<Vec<PhaseSummary>, String> {
    Ok(Vec::new())
}

pub fn telemetry_timeline(
    _conn: &Connection,
    _time_range: &str,
    _limit: usize,
) -> Result<Vec<TimelineEvent>, String> {
    Ok(Vec::new())
}

pub fn telemetry_heatmap(
    _conn: &Connection,
    _time_range: &str,
) -> Result<Vec<HeatmapCell>, String> {
    Ok(Vec::new())
}

pub fn telemetry_history(
    _conn: &Connection,
    _time_range: &str,
    _limit: usize,
) -> Result<Vec<HistoryEntry>, String> {
    Ok(Vec::new())
}

pub fn telemetry_criteria(
    _conn: &Connection,
    _time_range: &str,
) -> Result<Vec<AcceptanceCriterion>, String> {
    Ok(Vec::new())
}

pub fn telemetry_effort(
    _conn: &Connection,
    _time_range: &str,
) -> Result<EffortBreakdown, String> {
    Ok(EffortBreakdown::default())
}

pub fn telemetry_agents(
    _conn: &Connection,
    _time_range: &str,
) -> Result<Vec<AgentDispatch>, String> {
    Ok(Vec::new())
}
