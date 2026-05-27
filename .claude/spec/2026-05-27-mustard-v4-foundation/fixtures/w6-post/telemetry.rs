//! Telemetry readers (Wave 6B).
//!
//! Wave 6B of [[2026-05-26-no-sqlite-git-source-of-truth]] retires the SQLite
//! read paths that backed every dashboard telemetry surface. The 21 public
//! functions keep their signatures so `lib.rs` continues to type-check, but
//! the bodies now derive their answers from the per-spec NDJSON channel at
//! `.claude/spec/*/.events/*.ndjson` via [`mustard_core::events::EventReader`].
//! Most readers fail-open to zero / empty payloads — the frontend keeps
//! shape-correct responses; richer NDJSON-backed aggregations land in a
//! follow-up sub-spec.
//!
//! ## W5#8 — attribution two-tier (BLOCKER absorbed here)
//!
//! The OTEL collector (W5A) writes `pipeline.telemetry.run` records carrying
//! the full [`mustard_core::economy::SpanRecord`] shape. Attribution lives
//! inside `SpanRecord.extra` as the JSON keys:
//!
//! - `extra["tool_use_id"]` — Anthropic `tool_use` block id
//! - `extra["session_id"]`  — Claude Code session id (also in `SpanRecord.session_id`)
//! - `extra["spec"]`        — pipeline spec the span belongs to (also in `SpanRecord.spec`)
//!
//! Resolution follows two tiers, mirroring the legacy SQLite cross-join that
//! lived in `packages/core/src/telemetry/writer.rs::lookup_attribution`:
//!
//! 1. **Primary** — exact match by `(session_id, tool_use_id)` against the
//!    NDJSON walk.
//! 2. **Fallback** — the last span seen for the same `session_id` whose
//!    `started_at` is strictly before the query timestamp. That carries the
//!    spec / wave attribution forward to anonymous spans that lost their
//!    `tool_use_id` (rare, but the legacy behaviour kept the UI honest).
//!
//! The helper is exposed as [`lookup_attribution_extra`] so MCP / dashboard
//! callers can re-derive attribution without touching SQLite. The internal
//! Tier 1/2 split is documented inline so a future read of the source surfaces
//! the contract without grepping commits.

use mustard_core::events::EventReader;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ── Shapes preserved from the legacy reader ─────────────────────────────────

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct RtkDaily {
    pub date: String,
    pub commands: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub saved_tokens: u64,
    pub savings_pct: f64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct RtkBlock {
    pub available: bool,
    pub total_commands: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub tokens_saved: Option<u64>,
    pub savings_pct: Option<f64>,
    pub total_exec_time_ms: Option<u64>,
    pub daily: Vec<RtkDaily>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct MeasuredBlock {
    pub tokens_total: u64,
    pub tokens_today: u64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct HookFireCount {
    pub hook: String,
    pub fires: u64,
    pub tokens_saved: u64,
    pub most_recent_ts: Option<String>,
    pub session_fires: u64,
    pub session_tokens_saved: u64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct RoutingByIntent {
    pub intent: String,
    pub blocks: u64,
    pub allows: u64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct RoutingByNote {
    pub note: String,
    pub count: u64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct RoutingBlock {
    pub blocks: u64,
    pub allows: u64,
    pub by_intent: Vec<RoutingByIntent>,
    pub by_note: Vec<RoutingByNote>,
    pub session_blocks: u64,
    pub session_allows: u64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct PhaseCount {
    pub phase: String,
    pub count: u64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowBlock {
    pub by_phase: Vec<PhaseCount>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct ToolCount {
    pub tool_name: String,
    pub count: u64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct AgentActivity {
    pub agent_type: String,
    pub starts: u64,
    pub stops: u64,
    pub errors: u64,
    pub avg_duration_ms: u64,
    pub last_ts: Option<String>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct AgentActivityBlock {
    pub total_dispatches: u64,
    pub total_errors: u64,
    pub agents: Vec<AgentActivity>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct TelemetrySummary {
    pub rtk: RtkBlock,
    pub measured: MeasuredBlock,
    pub prevention: Vec<HookFireCount>,
    pub routing: RoutingBlock,
    pub workflow: WorkflowBlock,
    pub tool_breakdown: Vec<ToolCount>,
    pub agent_activity: AgentActivityBlock,
    pub session_start_ts: Option<String>,
}

/// Per-event friction entry. Kept for the dashboard "Atrito" widget.
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct FrictionEntry {
    pub kind: String,
    pub count: u64,
    pub last_ts: Option<String>,
}

/// Live-activity envelope. Mirrors the legacy `LiveActivity` projection.
#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct LiveActivity {
    pub events_last_5m: u64,
    pub current_phase: Option<String>,
    pub current_spec: Option<String>,
    pub last_event_ts: Option<String>,
    pub session_id: Option<String>,
}

/// OTEL collector health snapshot. Read from canary lines written by
/// `apps/rt/src/run/otel/collector.rs` (W5A) into
/// `.claude/.harness/.otel/canary.ndjson`. Fields are conservative defaults.
#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct CollectorHealth {
    pub healthy: bool,
    pub last_canary_at: Option<String>,
    pub last_canary_level: Option<String>,
    pub last_canary_msg: Option<String>,
}

// ── Attribution (W5#8 absorbed) ─────────────────────────────────────────────

/// Resolved attribution carried by a `pipeline.telemetry.run` span.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Attribution {
    pub spec: Option<String>,
    pub session_id: Option<String>,
    pub tool_use_id: Option<String>,
}

/// Two-tier attribution lookup against the per-spec NDJSON `.events/*.ndjson`
/// channels. Implements W5#8 — the SQLite cross-join from
/// `packages/core/src/telemetry/writer.rs::lookup_attribution` was retired in
/// W5A; the equivalent lives here, sourced from `SpanRecord.extra`.
///
/// # Tiers
///
/// - **Tier 1 (primary):** exact match by `(session_id, tool_use_id)`. Walks
///   `pipeline.telemetry.run` events under `repo_path`, comparing the payload's
///   `extra.session_id` and `extra.tool_use_id` fields. Returns the matched
///   span's `extra.spec` (or top-level `spec`) attribution.
/// - **Tier 2 (fallback):** the **last** `pipeline.telemetry.run` span for
///   the same `session_id` whose `started_at` is strictly before
///   `started_at_ms`. Falls back to the top-level `ts` field when the
///   `started_at` extra is absent. Returns `None` when neither tier matches.
///
/// Every IO failure (missing dir, malformed JSON) is swallowed — the function
/// is fail-open by contract.
#[must_use]
pub fn lookup_attribution_extra(
    repo_path: &Path,
    session_id_filter: &str,
    tool_use_id: Option<&str>,
    started_at_ms: i64,
) -> Option<Attribution> {
    let spec_base = repo_path.join(".claude").join("spec");
    let Ok(spec_dirs) = std::fs::read_dir(&spec_base) else {
        return None;
    };

    let mut tier2_candidate: Option<(i64, Attribution)> = None;

    for spec_dir in spec_dirs.flatten() {
        let events_dir = spec_dir.path().join(".events");
        let Ok(files) = std::fs::read_dir(&events_dir) else {
            continue;
        };
        for ev_file in files.flatten() {
            let path = ev_file.path();
            if path.extension().and_then(|s| s.to_str()) != Some("ndjson") {
                continue;
            }
            for event in EventReader::stream(&path) {
                if event.kind != "pipeline.telemetry.run" {
                    continue;
                }
                let payload = &event.payload;
                let span_session = payload
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .or_else(|| payload.get("extra").and_then(|e| e.get("session_id")).and_then(|v| v.as_str()))
                    .unwrap_or("");
                if span_session != session_id_filter {
                    continue;
                }
                let extra_tool = payload
                    .get("extra")
                    .and_then(|e| e.get("tool_use_id"))
                    .and_then(|v| v.as_str());

                // Tier 1: exact (session_id, tool_use_id) match.
                if let (Some(needle), Some(haystack)) = (tool_use_id, extra_tool) {
                    if needle == haystack {
                        return Some(extract_attribution(payload, span_session));
                    }
                }

                // Tier 2: last span in session strictly before started_at_ms.
                let span_started = payload
                    .get("started_at")
                    .and_then(|v| v.as_i64())
                    .or_else(|| payload.get("ts").and_then(|v| iso_to_ms(v.as_str().unwrap_or(""))))
                    .unwrap_or(0);
                if span_started < started_at_ms {
                    if tier2_candidate.as_ref().map_or(true, |(prev, _)| span_started > *prev) {
                        tier2_candidate = Some((span_started, extract_attribution(payload, span_session)));
                    }
                }
            }
        }
    }

    tier2_candidate.map(|(_, attr)| attr)
}

/// Internal — pull `(spec, session_id, tool_use_id)` out of a SpanRecord
/// payload. Top-level fields win over `extra` keys (the W5A collector writes
/// both, but the typed fields are authoritative).
fn extract_attribution(payload: &serde_json::Value, session_id: &str) -> Attribution {
    let extra = payload.get("extra");
    let spec = payload
        .get("spec")
        .and_then(|v| v.as_str())
        .or_else(|| extra.and_then(|e| e.get("spec")).and_then(|v| v.as_str()))
        .map(str::to_string);
    let tool_use_id = extra
        .and_then(|e| e.get("tool_use_id"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Attribution {
        spec,
        session_id: Some(session_id.to_string()),
        tool_use_id,
    }
}

/// Parse an ISO-8601 timestamp into milliseconds since epoch. Returns `None`
/// on any parse failure.
fn iso_to_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

// ── Public readers (fail-open stubs) ────────────────────────────────────────

/// ISO-8601 cut-off marking the start of the current session, or `None` when
/// no NDJSON event with a `session_id` has been observed under `repo_path`.
/// Walks `.claude/spec/*/.events/*.ndjson` and returns the earliest `ts`
/// sharing the most-recently-seen `session_id`.
pub fn session_start_ts(repo_path: &Path) -> Option<String> {
    let spec_base = repo_path.join(".claude").join("spec");
    let spec_dirs = std::fs::read_dir(&spec_base).ok()?;

    // First pass: find the most-recent (latest ts) event that carries a
    // non-empty session_id.
    let mut latest: Option<(String, String)> = None; // (ts, session_id)
    for spec_dir in spec_dirs.flatten() {
        let events_dir = spec_dir.path().join(".events");
        let Ok(files) = std::fs::read_dir(&events_dir) else {
            continue;
        };
        for ev_file in files.flatten() {
            let path = ev_file.path();
            if path.extension().and_then(|s| s.to_str()) != Some("ndjson") {
                continue;
            }
            for event in EventReader::stream(&path) {
                let session = event
                    .payload
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if session.is_empty() {
                    continue;
                }
                let ts = event
                    .payload
                    .get("ts")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if ts.is_empty() {
                    continue;
                }
                let take = match latest.as_ref() {
                    None => true,
                    Some((prev, _)) => ts > prev.as_str(),
                };
                if take {
                    latest = Some((ts.to_string(), session.to_string()));
                }
            }
        }
    }

    let (_, target_session) = latest?;

    // Second pass: earliest ts sharing the target session_id.
    let mut earliest: Option<String> = None;
    let spec_dirs2 = std::fs::read_dir(&spec_base).ok()?;
    for spec_dir in spec_dirs2.flatten() {
        let events_dir = spec_dir.path().join(".events");
        let Ok(files) = std::fs::read_dir(&events_dir) else {
            continue;
        };
        for ev_file in files.flatten() {
            let path = ev_file.path();
            if path.extension().and_then(|s| s.to_str()) != Some("ndjson") {
                continue;
            }
            for event in EventReader::stream(&path) {
                let session = event
                    .payload
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if session != target_session {
                    continue;
                }
                let ts = event
                    .payload
                    .get("ts")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if ts.is_empty() {
                    continue;
                }
                let take = match earliest.as_ref() {
                    None => true,
                    Some(prev) => ts < prev.as_str(),
                };
                if take {
                    earliest = Some(ts.to_string());
                }
            }
        }
    }
    earliest
}

/// Per-project RTK summary block. Wave 6B fail-open: zero block until a
/// dedicated NDJSON reader lands; the dashboard stays shape-correct.
#[must_use]
pub fn rtk_summary(_repo_path: &Path) -> RtkBlock {
    RtkBlock::default()
}

/// Global RTK summary across every known workspace. Same fail-open shape.
#[must_use]
pub fn rtk_summary_global() -> RtkBlock {
    RtkBlock::default()
}

/// Per-hook fire counts for the prevention column. Empty vec until the NDJSON
/// reader lands — the UI tolerates an empty list (renders "—").
#[must_use]
pub fn hook_fire_counts(_repo_path: &Path, _since: Option<&str>) -> Vec<HookFireCount> {
    Vec::new()
}

/// Model-routing block. Zero counts until the NDJSON reader lands.
#[must_use]
pub fn routing_breakdown(_repo_path: &Path, _since: Option<&str>) -> RoutingBlock {
    RoutingBlock::default()
}

/// Workflow-by-phase block. Empty `by_phase` is shape-correct.
#[must_use]
pub fn workflow_by_phase(_repo_path: &Path) -> WorkflowBlock {
    WorkflowBlock::default()
}

/// Top-N tool breakdown. Empty vec is shape-correct.
#[must_use]
pub fn tool_breakdown(_repo_path: &Path) -> Vec<ToolCount> {
    Vec::new()
}

/// Agent activity block. Zero counts is shape-correct.
#[must_use]
pub fn agent_activity(_repo_path: &Path) -> AgentActivityBlock {
    AgentActivityBlock::default()
}

/// Public ISO→ms parser kept for callers that compose the value into other
/// payloads. Mirrors [`iso_to_ms`] without exposing the internal name.
#[must_use]
pub fn parse_iso_ms_pub(s: &str) -> Option<i64> {
    iso_to_ms(s)
}

/// Per-project measured-tokens block. Wave 6B fail-open: both counters zero.
#[must_use]
pub fn measured(_repo_path: &Path) -> MeasuredBlock {
    MeasuredBlock::default()
}

/// Live activity snapshot. The legacy reader joined `events.payload` against
/// the spans projection; Wave 6B reads only what NDJSON exposes directly.
/// We surface the most recent `pipeline.telemetry.run` span's session and
/// spec — that is the strongest live signal available without SQLite.
#[must_use]
pub fn live_activity(repo_path: &Path) -> LiveActivity {
    let spec_base = repo_path.join(".claude").join("spec");
    let Ok(spec_dirs) = std::fs::read_dir(&spec_base) else {
        return LiveActivity::default();
    };
    let mut latest_ts: Option<String> = None;
    let mut latest_payload: Option<serde_json::Value> = None;
    for spec_dir in spec_dirs.flatten() {
        let events_dir = spec_dir.path().join(".events");
        let Ok(files) = std::fs::read_dir(&events_dir) else {
            continue;
        };
        for ev_file in files.flatten() {
            let path = ev_file.path();
            if path.extension().and_then(|s| s.to_str()) != Some("ndjson") {
                continue;
            }
            for event in EventReader::stream(&path) {
                let ts = event
                    .payload
                    .get("ts")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if ts.is_empty() {
                    continue;
                }
                let take = match latest_ts.as_ref() {
                    None => true,
                    Some(prev) => ts > *prev,
                };
                if take {
                    latest_ts = Some(ts);
                    latest_payload = Some(event.payload.clone());
                }
            }
        }
    }
    let payload = latest_payload.unwrap_or_default();
    LiveActivity {
        events_last_5m: 0,
        current_phase: payload
            .get("phase")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        current_spec: payload
            .get("spec")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        last_event_ts: latest_ts,
        session_id: payload
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    }
}

/// Friction entries — read from `.claude/.metrics/friction.json` (legacy
/// channel that survived the SQLite migration). Empty vec when the file is
/// absent, which is the common case.
#[must_use]
pub fn friction_entries(repo_path: &Path) -> Vec<FrictionEntry> {
    let path = repo_path
        .join(".claude")
        .join(".metrics")
        .join("friction.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<FrictionEntry>>(&text).unwrap_or_default()
}

/// OTEL collector freshness check derived from a probe ISO-8601 timestamp.
/// Stable signature kept for callers that already hold a freshness sample.
#[must_use]
pub fn collector_health_from_freshness(last_canary_at: Option<String>) -> CollectorHealth {
    CollectorHealth {
        healthy: last_canary_at.is_some(),
        last_canary_at,
        last_canary_level: None,
        last_canary_msg: None,
    }
}

/// OTEL collector health. Reads the tail of
/// `.claude/.harness/.otel/canary.ndjson` (one line per collector
/// heartbeat). Fail-open: an absent file maps to `healthy = false`.
///
/// Exposed as a Tauri command — `lib.rs::tauri::generate_handler!` lists
/// `telemetry::collector_health` and the frontend invokes it directly.
#[tauri::command]
#[must_use]
pub fn collector_health(repo_path: String) -> CollectorHealth {
    let base = PathBuf::from(&repo_path);
    collector_health_impl(&base)
}

/// Implementation entry point with a `&Path` shape — kept for unit-test
/// access and any non-Tauri caller (legacy direct uses inside this module).
#[must_use]
pub fn collector_health_impl(repo_path: &Path) -> CollectorHealth {
    let canary = repo_path
        .join(".claude")
        .join(".harness")
        .join(".otel")
        .join("canary.ndjson");
    let Ok(text) = std::fs::read_to_string(&canary) else {
        return CollectorHealth::default();
    };
    let last = text.lines().rev().find(|l| !l.trim().is_empty());
    let Some(line) = last else {
        return CollectorHealth::default();
    };
    let parsed: serde_json::Value = serde_json::from_str(line).unwrap_or_default();
    CollectorHealth {
        healthy: true,
        last_canary_at: parsed
            .get("ts")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        last_canary_level: parsed
            .get("level")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        last_canary_msg: parsed
            .get("msg")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    }
}

// ── Tauri-command stubs (former SQLite economy / spec-trace readers) ────────
//
// All of the below preserve the `#[tauri::command]` shapes referenced by
// `lib.rs::tauri::generate_handler!`. Each one returns a shape-correct
// zero/empty body. The real NDJSON-backed economy readers live in
// `apps/dashboard/src-tauri/src/economy.rs` (W6A) and richer surfaces will
// be reintroduced once the W7 core-economy NDJSON layer is in.

#[tauri::command]
#[must_use]
pub fn dashboard_prompt_economy(_repo_path: String) -> serde_json::Value {
    serde_json::json!({ "by_role": [], "by_command": [] })
}

#[tauri::command]
#[must_use]
pub fn dashboard_economy_summary(_repo_path: String) -> serde_json::Value {
    serde_json::json!({
        "tokens_total": 0,
        "tokens_today": 0,
        "cost_usd": 0.0,
        "cost_usd_today": 0.0,
    })
}

#[tauri::command]
#[must_use]
pub fn dashboard_economy_savings_breakdown(_repo_path: String) -> serde_json::Value {
    serde_json::json!({ "by_source": [] })
}

#[tauri::command]
#[must_use]
pub fn dashboard_economy_context_routing(_repo_path: String) -> serde_json::Value {
    serde_json::json!({ "by_intent": [] })
}

#[tauri::command]
#[must_use]
pub fn dashboard_economy_per_spec_costs(_repo_path: String) -> serde_json::Value {
    serde_json::json!({ "per_spec": [] })
}

#[tauri::command]
#[must_use]
pub fn dashboard_economy_per_wave_costs(_repo_path: String) -> serde_json::Value {
    serde_json::json!({ "per_wave": [] })
}

/// Spec trace — formerly a recursive walk over `events` plus `spans`. Wave 6B
/// returns the resolved attribution for the (session_id, tool_use_id?) pair
/// so the dashboard at least exposes the W5#8 surface end-to-end. Callers
/// that hit this without a session id receive an empty object.
#[tauri::command]
#[must_use]
pub fn dashboard_spec_trace(
    repo_path: String,
    session_id: Option<String>,
    tool_use_id: Option<String>,
    started_at_ms: Option<i64>,
) -> serde_json::Value {
    let Some(sid) = session_id.filter(|s| !s.is_empty()) else {
        return serde_json::json!({ "attribution": null, "spans": [] });
    };
    let started = started_at_ms.unwrap_or(i64::MAX);
    let base = PathBuf::from(&repo_path);
    let attr = lookup_attribution_extra(&base, &sid, tool_use_id.as_deref(), started);
    serde_json::json!({
        "attribution": attr,
        "spans": [],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_event(dir: &Path, spec: &str, name: &str, body: &str) {
        let events_dir = dir
            .join(".claude")
            .join("spec")
            .join(spec)
            .join(".events");
        std::fs::create_dir_all(&events_dir).unwrap();
        std::fs::write(events_dir.join(name), body).unwrap();
    }

    fn span_line(session: &str, tool_use: Option<&str>, spec: &str, ts: &str) -> String {
        let mut payload = serde_json::json!({
            "kind": "pipeline.telemetry.run",
            "ts": ts,
            "session_id": session,
            "spec": spec,
            "extra": {
                "session_id": session,
                "spec": spec,
            }
        });
        if let Some(tu) = tool_use {
            payload["extra"]["tool_use_id"] = serde_json::Value::String(tu.to_string());
        }
        serde_json::to_string(&payload).unwrap()
    }

    #[test]
    fn attribution_tier1_matches_by_tool_use_id() {
        let tmp = TempDir::new().unwrap();
        let lines = format!(
            "{}\n{}\n",
            span_line("sess-A", Some("tu-1"), "spec-alpha", "2026-05-27T10:00:00.000Z"),
            span_line("sess-A", Some("tu-2"), "spec-beta", "2026-05-27T10:00:05.000Z"),
        );
        write_event(tmp.path(), "spec-alpha", "otel.ndjson", &lines);

        let attr = lookup_attribution_extra(tmp.path(), "sess-A", Some("tu-2"), 99_999_999_999_999)
            .expect("tier1 should hit");
        assert_eq!(attr.spec.as_deref(), Some("spec-beta"));
        assert_eq!(attr.session_id.as_deref(), Some("sess-A"));
        assert_eq!(attr.tool_use_id.as_deref(), Some("tu-2"));
    }

    #[test]
    fn attribution_tier2_picks_last_span_before_ts() {
        let tmp = TempDir::new().unwrap();
        // No tool_use_id match — fallback to last-before-ts.
        let lines = format!(
            "{}\n{}\n",
            span_line("sess-B", Some("tu-x"), "spec-old", "2026-05-27T09:00:00.000Z"),
            span_line("sess-B", Some("tu-y"), "spec-new", "2026-05-27T09:30:00.000Z"),
        );
        write_event(tmp.path(), "spec-old", "otel.ndjson", &lines);

        // Query at 10:00:00 with no tool_use_id → tier2, picks the latest span
        // strictly before the query (09:30 win over 09:00).
        let started_at_ms = iso_to_ms("2026-05-27T10:00:00.000Z").unwrap();
        let attr = lookup_attribution_extra(tmp.path(), "sess-B", None, started_at_ms)
            .expect("tier2 should hit");
        assert_eq!(attr.spec.as_deref(), Some("spec-new"));
    }

    #[test]
    fn attribution_returns_none_when_session_unknown() {
        let tmp = TempDir::new().unwrap();
        write_event(
            tmp.path(),
            "spec-z",
            "otel.ndjson",
            &format!("{}\n", span_line("sess-known", Some("tu"), "spec-z", "2026-05-27T10:00:00.000Z")),
        );
        let attr = lookup_attribution_extra(tmp.path(), "sess-other", Some("tu"), i64::MAX);
        assert!(attr.is_none());
    }

    #[test]
    fn live_activity_picks_latest_event() {
        let tmp = TempDir::new().unwrap();
        let lines = format!(
            "{}\n{}\n",
            r#"{"kind":"pipeline.phase","ts":"2026-05-27T09:00:00.000Z","session_id":"s","spec":"alpha","phase":"plan"}"#,
            r#"{"kind":"pipeline.phase","ts":"2026-05-27T09:05:00.000Z","session_id":"s","spec":"alpha","phase":"execute"}"#,
        );
        write_event(tmp.path(), "alpha", "events.ndjson", &lines);
        let live = live_activity(tmp.path());
        assert_eq!(live.current_phase.as_deref(), Some("execute"));
        assert_eq!(live.current_spec.as_deref(), Some("alpha"));
    }

    #[test]
    fn session_start_returns_earliest_ts_in_latest_session() {
        let tmp = TempDir::new().unwrap();
        // Two sessions interleaved; the most-recent event sits in session-2.
        let lines = format!(
            "{}\n{}\n{}\n{}\n",
            r#"{"kind":"k","ts":"2026-05-27T08:00:00.000Z","session_id":"sess-1","spec":"a"}"#,
            r#"{"kind":"k","ts":"2026-05-27T09:00:00.000Z","session_id":"sess-2","spec":"a"}"#,
            r#"{"kind":"k","ts":"2026-05-27T08:30:00.000Z","session_id":"sess-2","spec":"a"}"#,
            r#"{"kind":"k","ts":"2026-05-27T10:00:00.000Z","session_id":"sess-2","spec":"a"}"#,
        );
        write_event(tmp.path(), "a", "events.ndjson", &lines);
        let start = session_start_ts(tmp.path()).expect("should resolve");
        assert_eq!(start, "2026-05-27T08:30:00.000Z");
    }
}
