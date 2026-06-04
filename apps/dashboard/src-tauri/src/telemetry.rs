//! Telemetry readers — restored by wave-21-dashboard-restore.
//!
//! Wave 6B (commit 723ad1a) of [[2026-05-26-no-sqlite-git-source-of-truth]]
//! retired the SQLite read paths that backed every dashboard telemetry
//! surface, but left ~8 public functions returning `Default::default()` /
//! `Vec::new()`. This file restores real readers for each, sourced from the
//! NDJSON per-spec event channels (`.claude/spec/*/.events/*.ndjson`) and the
//! legacy hook-metric JSONL shards (`.claude/.metrics/*.jsonl`) that
//! `mustard_core::platform::metrics::emit_metric` continues to write.
//!
//! ## Sources by reader
//!
//! | Reader | Source |
//! |---|---|
//! | `rtk_summary`, `rtk_summary_global` | subprocess `rtk gain -f json --daily` |
//! | `hook_fire_counts` | filesystem `.claude/.metrics/*.jsonl` |
//! | `routing_breakdown` | filesystem `.claude/.metrics/model-routing-gate.jsonl` |
//! | `workflow_by_phase` | NDJSON `event=="pipeline.phase"` |
//! | `tool_breakdown` | NDJSON `event=="tool.use"`, agg `payload.tool` |
//! | `agent_activity` | NDJSON `event=="agent.start"`/`"agent.stop"` |
//! | `measured` | NDJSON `event=="pipeline.telemetry.run"`, sum tokens |
//! | `dashboard_spec_trace` | NDJSON `event=="tool.use"` per spec (minimal) |
//!
//! Every reader is **fail-open** — a missing dir, malformed line, or absent
//! binary degrades to a `Default`/empty result so the frontend stays
//! shape-correct.
//!
//! ## NDJSON event vs kind
//!
//! The NDJSON record on disk carries both `"event"` (the harness event name,
//! e.g. `"tool.use"`) and `"kind"` (the dashboard's logical classification,
//! e.g. `"tool"`). [`mustard_core::io::events::reader::EventReader`] deserialises
//! the `"kind"` JSON field into `Event.kind`, so when filtering by event
//! **name** you must read `event.raw["event"]`, not `event.kind`. The one
//! exception is the OTEL collector, which writes `event_name == kind` (both
//! set to `"pipeline.telemetry.run"`), so the historical filter on
//! `event.kind == "pipeline.telemetry.run"` still works for that subset.
//!
//! ## W5#8 — attribution two-tier
//!
//! The OTEL collector (W5A) writes `pipeline.telemetry.run` records carrying
//! the full [`mustard_core::domain::economy::SpanRecord`] shape. Attribution lives
//! inside `SpanRecord.extra` as the JSON keys `tool_use_id`, `session_id`,
//! `spec`. Resolution follows two tiers — `Tier 1` is exact
//! `(session_id, tool_use_id)`, `Tier 2` is the last span in the same
//! `session_id` whose `started_at` is strictly before the query timestamp.
//!
//! ## Behavioral gaps (pending follow-up)
//!
//! The 6 `dashboard_economy_*` + `dashboard_prompt_economy` commands accept a
//! `EconomyScopeDto` argument (so the frontend's `invoke(..., { scope })` call
//! no longer panics on signature mismatch) but still return a default body.
//! Implementing them requires migrating `mustard_core::domain::economy::reader` off
//! SQLite, which is outside this restoration's scope. The doc-comments below
//! tag each one as a behavioural gap.

use mustard_core::ClaudePaths;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Stdio;

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

/// OTEL collector health snapshot.
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

/// One Claude Code session, aggregated from `.claude/.session/{id}/.events/`.
///
/// Mirrors the frontend `SessionRow` (`lib/dashboard.ts`); field names are
/// `snake_case` so the serde shape matches the TypeScript interface verbatim.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionRow {
    /// The session directory name (a UUID, or the literal `unknown` bucket).
    pub id: String,
    /// Human handle. No slug source exists yet, so the frontend falls back to
    /// `id`; kept for forward-compat with the schema.
    pub slug: String,
    /// Earliest event `ts` seen in the session (ISO-8601).
    pub started_at: String,
    /// Latest event `ts` seen (ISO-8601). `None` when the session has no
    /// timestamped events.
    pub last_activity_at: Option<String>,
    /// `spec` of the most-recent event that carried one. `None` when every
    /// event was spec-less (root-orchestrator turns).
    pub last_spec: Option<String>,
    /// Working directory from the `session.start` payload (or any event that
    /// carried one). `None` when unknown.
    pub cwd: Option<String>,
    /// `"open"` when the last activity is within [`SESSION_OPEN_WINDOW_MS`] of
    /// now, else `"closed"`. There is no session-end event, so recency is the
    /// only honest liveness signal.
    pub status: String,
    /// Number of parseable NDJSON event lines aggregated for this session.
    pub event_count: u64,
    /// `true` for the `unknown` attribution-leak bucket (events that landed in
    /// `.session/unknown/` because their `session_id` couldn't be resolved at
    /// emit time). Surfaced honestly rather than hidden so the leak stays
    /// visible; the row is labelled, not dropped.
    pub is_unknown_bucket: bool,
}

/// A session counts as `open` when its last activity is no older than this.
const SESSION_OPEN_WINDOW_MS: i64 = 15 * 60 * 1000;

/// Two-tier attribution lookup against the per-spec NDJSON `.events/*.ndjson`
/// channels (W5#8).
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

    let mut records: Vec<Value> = Vec::new();
    for spec_dir in spec_dirs.flatten() {
        // Reads raw `Value` lines — the typed `Event` reader can't be used here
        // because real span records (and the test fixtures) may omit the
        // required `payload` field, which makes serde drop the whole line.
        collect_one_dir(&spec_dir.path().join(".events"), &mut records);
    }

    for record in &records {
        // Match on the harness event NAME, not the logical `kind` class. On
        // disk a span carries `event == "pipeline.telemetry.run"` but
        // `kind == "pipeline"`; only the OTEL collector sets the two equal.
        // `event_name` reads `"event"` and falls back to `"kind"` for older
        // payloads (mirrors core/economy/reader.rs:82).
        if event_name(record) != "pipeline.telemetry.run" {
            continue;
        }
        // Real records carry `session_id`/`spec`/`extra` at the RECORD level,
        // not under `payload`; fall back to `payload` (and `payload.extra`)
        // for legacy / OTEL span shapes.
        let span_session = first_str(
            record,
            &[&["session_id"], &["extra", "session_id"], &["payload", "session_id"], &["payload", "extra", "session_id"]],
        )
        .unwrap_or("");
        if span_session != session_id_filter {
            continue;
        }
        let extra_tool = first_str(
            record,
            &[&["extra", "tool_use_id"], &["tool_use_id"], &["payload", "tool_use_id"], &["payload", "extra", "tool_use_id"]],
        );

        // Tier 1: exact (session_id, tool_use_id) match.
        if let (Some(needle), Some(haystack)) = (tool_use_id, extra_tool) {
            if needle == haystack {
                return Some(extract_attribution(record, span_session));
            }
        }

        // Tier 2: last span in session strictly before started_at_ms.
        let span_started = first_i64(record, &[&["started_at"], &["payload", "started_at"]])
            .or_else(|| first_str(record, &[&["ts"], &["payload", "ts"]]).and_then(iso_to_ms))
            .unwrap_or(0);
        if span_started < started_at_ms
            && tier2_candidate.as_ref().map_or(true, |(prev, _)| span_started > *prev)
        {
            tier2_candidate = Some((span_started, extract_attribution(record, span_session)));
        }
    }

    tier2_candidate.map(|(_, attr)| attr)
}

fn extract_attribution(record: &Value, session_id: &str) -> Attribution {
    // `spec`/`tool_use_id` live at the record level on real spans, inside
    // `extra` on OTEL `SpanRecord`s, or under `payload` on legacy shapes —
    // probe all three, record-level first.
    let spec = first_str(
        record,
        &[&["spec"], &["extra", "spec"], &["payload", "spec"], &["payload", "extra", "spec"]],
    )
    .map(str::to_string);
    let tool_use_id = first_str(
        record,
        &[&["extra", "tool_use_id"], &["tool_use_id"], &["payload", "tool_use_id"], &["payload", "extra", "tool_use_id"]],
    )
    .map(str::to_string);
    Attribution {
        spec,
        session_id: Some(session_id.to_string()),
        tool_use_id,
    }
}

/// Canonical harness event NAME for a raw NDJSON record. The writers emit a
/// top-level `"event"` field (e.g. `"tool.use"`, `"pipeline.telemetry.run"`)
/// distinct from the `"kind"` CLASS (`"tool"`, `"pipeline"`); when `"event"`
/// is absent (older payloads, OTEL collector) the `"kind"` discriminator holds
/// the same value. Mirrors `mustard_core::domain::economy::reader::event_name`.
fn event_name(record: &Value) -> &str {
    record
        .get("event")
        .and_then(Value::as_str)
        .or_else(|| record.get("kind").and_then(Value::as_str))
        .unwrap_or("")
}

/// First non-empty string found by probing `record` along each JSON path in
/// order. A path is a slice of keys; `["payload", "session_id"]` reads
/// `record["payload"]["session_id"]`, `["session_id"]` reads the record-level
/// field. Lets one call straddle the record level and the nested `payload`.
fn first_str<'r>(record: &'r Value, paths: &[&[&str]]) -> Option<&'r str> {
    paths
        .iter()
        .find_map(|path| dig(record, path).and_then(Value::as_str))
        .filter(|s| !s.is_empty())
}

/// `i64` counterpart of [`first_str`] — first integer found along the paths.
fn first_i64(record: &Value, paths: &[&[&str]]) -> Option<i64> {
    paths
        .iter()
        .find_map(|path| dig(record, path).and_then(Value::as_i64))
}

/// Resolve a key path against a raw record `Value`.
fn dig<'r>(record: &'r Value, path: &[&str]) -> Option<&'r Value> {
    let mut cur = record;
    for key in path {
        cur = cur.get(key)?;
    }
    Some(cur)
}

fn iso_to_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// `true` when `ts` is lexically `>= since`. ISO-8601 UTC strings sort
/// chronologically — a plain string compare is correct and avoids a date-parsing
/// dependency.
fn in_session(ts: Option<&str>, since: Option<&str>) -> bool {
    match (ts, since) {
        (Some(t), Some(s)) => t >= s,
        _ => false,
    }
}

/// Iterate every `<repo>/.claude/spec/*/.events/*.ndjson` file, yielding the
/// (event, JSON value of the line's full raw record) for every parseable line.
///
/// The returned `Value` has the full record fields available
/// (`"event"`, `"kind"`, `"ts"`, `"session_id"`, `"actor"`, `"spec"`,
/// `"wave"`, `"payload"`, `"tokens_in"`, `"tokens_out"`, `"duration_ms"`),
/// so callers can read `value["event"]` to match the harness event name.
fn for_each_ndjson_line<F>(repo_path: &Path, mut visit: F)
where
    F: FnMut(&Value),
{
    let spec_base = repo_path.join(".claude").join("spec");
    let Ok(spec_dirs) = std::fs::read_dir(&spec_base) else {
        return;
    };
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
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                visit(&v);
            }
        }
    }
}

// ── RTK ─────────────────────────────────────────────────────────────────────

fn rtk_unavailable() -> RtkBlock {
    RtkBlock::default()
}

/// Run `rtk gain -f json --daily`, optionally with `-p` and a chdir, and decode
/// the result. Returns `rtk_unavailable()` on any failure (binary missing,
/// non-zero exit, malformed JSON).
fn run_rtk_gain(repo_path: Option<&Path>) -> RtkBlock {
    let mut cmd = crate::process_util::no_window_command("rtk");
    cmd.arg("gain").arg("-f").arg("json").arg("--daily");
    if let Some(p) = repo_path {
        cmd.arg("-p").current_dir(p);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let output = match cmd.output() {
        Ok(o) => o,
        Err(_) => return rtk_unavailable(),
    };
    if !output.status.success() {
        return rtk_unavailable();
    }
    let stdout = match std::str::from_utf8(&output.stdout) {
        Ok(s) => s,
        Err(_) => return rtk_unavailable(),
    };
    let v: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(_) => return rtk_unavailable(),
    };

    let summary = v.get("summary");
    let total_commands = summary.and_then(|s| s.get("total_commands")).and_then(Value::as_u64);
    let input_tokens = summary.and_then(|s| s.get("total_input")).and_then(Value::as_u64);
    let output_tokens = summary.and_then(|s| s.get("total_output")).and_then(Value::as_u64);
    let tokens_saved = summary.and_then(|s| s.get("total_saved")).and_then(Value::as_u64);
    let savings_pct = summary.and_then(|s| s.get("avg_savings_pct")).and_then(Value::as_f64);
    let total_exec_time_ms = summary.and_then(|s| s.get("total_time_ms")).and_then(Value::as_u64);

    let daily: Vec<RtkDaily> = v
        .get("daily")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    let date = entry.get("date").and_then(Value::as_str)?.to_string();
                    Some(RtkDaily {
                        date,
                        commands: entry.get("commands").and_then(Value::as_u64).unwrap_or(0),
                        input_tokens: entry.get("input_tokens").and_then(Value::as_u64).unwrap_or(0),
                        output_tokens: entry.get("output_tokens").and_then(Value::as_u64).unwrap_or(0),
                        saved_tokens: entry.get("saved_tokens").and_then(Value::as_u64).unwrap_or(0),
                        savings_pct: entry.get("savings_pct").and_then(Value::as_f64).unwrap_or(0.0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    RtkBlock {
        available: true,
        total_commands,
        input_tokens,
        output_tokens,
        tokens_saved,
        savings_pct,
        total_exec_time_ms,
        daily,
    }
}

/// Per-project RTK summary. Shells `rtk gain -p` with cwd=repo so RTK filters
/// to commands that ran in this project tree.
#[must_use]
pub fn rtk_summary(repo_path: &Path) -> RtkBlock {
    run_rtk_gain(Some(repo_path))
}

/// Global RTK summary across every known workspace. Same shape, no `-p`.
#[must_use]
pub fn rtk_summary_global() -> RtkBlock {
    run_rtk_gain(None)
}

// ── Hook fire counts ─────────────────────────────────────────────────────────

const EXCLUDED_HOOKS: &[&str] = &["rtk-gain", "rtk-rewrite", "budget-observations"];

/// Aggregate per-hook fire counts + tokens saved from
/// `.claude/.metrics/*.jsonl`. Each `<event>.jsonl` shard is one hook; we sum
/// `tokens_saved` and bump `fires` per parseable line. `session_since` cuts
/// the lifetime totals down to "this session" via lexical ts compare.
#[must_use]
pub fn hook_fire_counts(repo_path: &Path, session_since: Option<&str>) -> Vec<HookFireCount> {
    let metrics_dir = repo_path.join(".claude").join(".metrics");
    let entries = match std::fs::read_dir(&metrics_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut results: Vec<HookFireCount> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        if EXCLUDED_HOOKS.contains(&stem.as_str()) {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mut fires: u64 = 0;
        let mut tokens_saved: u64 = 0;
        let mut session_fires: u64 = 0;
        let mut session_tokens_saved: u64 = 0;
        let mut most_recent_ts: Option<String> = None;
        for line in content.lines() {
            let v: Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            fires += 1;
            let saved = v["tokens_saved"].as_u64().unwrap_or(0);
            tokens_saved += saved;
            let ts = v["ts"].as_str();
            if let Some(ts) = ts {
                most_recent_ts = Some(ts.to_string());
            }
            if in_session(ts, session_since) {
                session_fires += 1;
                session_tokens_saved += saved;
            }
        }
        results.push(HookFireCount {
            hook: stem,
            fires,
            tokens_saved,
            most_recent_ts,
            session_fires,
            session_tokens_saved,
        });
    }

    results.sort_by(|a, b| b.tokens_saved.cmp(&a.tokens_saved).then(b.fires.cmp(&a.fires)));
    results
}

// ── Routing breakdown ────────────────────────────────────────────────────────

/// Aggregate `model-routing-gate.jsonl` lines into the routing breakdown the
/// dashboard surfaces. Groups by subagent_type / pipeline_type / intent, counts
/// blocks vs allows, and emits a per-note tally for the prevention-category
/// stack.
#[must_use]
pub fn routing_breakdown(repo_path: &Path, session_since: Option<&str>) -> RoutingBlock {
    let path = repo_path
        .join(".claude")
        .join(".metrics")
        .join("model-routing-gate.jsonl");
    if !path.exists() {
        return RoutingBlock::default();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return RoutingBlock::default(),
    };

    let mut total_blocks: u64 = 0;
    let mut total_allows: u64 = 0;
    let mut session_blocks: u64 = 0;
    let mut session_allows: u64 = 0;
    let mut grouped: HashMap<String, (u64, u64)> = HashMap::new();
    let mut by_note_map: HashMap<String, u64> = HashMap::new();

    for line in content.lines() {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let note = v["note"]
            .as_str()
            .or_else(|| v.get("payload").and_then(|p| p.get("note")).and_then(Value::as_str))
            .unwrap_or("");
        // Block notes: explicit upgrade-attempt + Explorer no-model + sonnet-rule.
        let is_block = matches!(
            note,
            "violation" | "no-model-denied" | "no-model-denied-sonnet" | "blocked"
        );
        let is_allow =
            note == "passed" || note == "no-model-advisory" || note.starts_with("allow");
        if !is_block && !is_allow {
            continue;
        }

        *by_note_map.entry(note.to_string()).or_insert(0) += 1;

        let key = extract_routing_key(&v);
        let entry = grouped.entry(key).or_insert((0, 0));
        let session = in_session(v.get("ts").and_then(Value::as_str), session_since);
        if is_block {
            total_blocks += 1;
            entry.0 += 1;
            if session {
                session_blocks += 1;
            }
        } else {
            total_allows += 1;
            entry.1 += 1;
            if session {
                session_allows += 1;
            }
        }
    }

    let mut intent_vec: Vec<RoutingByIntent> = grouped
        .into_iter()
        .map(|(intent, (blocks, allows))| RoutingByIntent { intent, blocks, allows })
        .collect();
    intent_vec.sort_by(|a, b| {
        b.blocks
            .cmp(&a.blocks)
            .then((b.blocks + b.allows).cmp(&(a.blocks + a.allows)))
    });
    intent_vec.truncate(6);

    let mut by_note: Vec<RoutingByNote> = by_note_map
        .into_iter()
        .map(|(note, count)| RoutingByNote { note, count })
        .collect();
    by_note.sort_by_key(|a| std::cmp::Reverse(a.count));

    RoutingBlock {
        blocks: total_blocks,
        allows: total_allows,
        by_intent: intent_vec,
        by_note,
        session_blocks,
        session_allows,
    }
}

/// Pull the most useful grouping key out of a `model-routing-gate` event:
/// subagent_type → pipeline_type → legacy intent → "outros".
fn extract_routing_key(v: &Value) -> String {
    let extras = v.get("payload").and_then(|p| p.get("extras"));
    let lookup = |k: &str| -> Option<String> {
        v.get(k)
            .and_then(Value::as_str)
            .or_else(|| extras.and_then(|e| e.get(k)).and_then(Value::as_str))
            .filter(|s| !s.is_empty() && *s != "unknown" && *s != "none")
            .map(str::to_string)
    };
    if let Some(s) = lookup("subagent_type") {
        return s;
    }
    if let Some(s) = lookup("pipeline_type") {
        return s;
    }
    if let Some(s) = v
        .get("payload")
        .and_then(|p| p.get("intent"))
        .and_then(Value::as_str)
    {
        if !s.is_empty() {
            return s.to_string();
        }
    }
    "outros".to_string()
}

// ── Workflow by phase ────────────────────────────────────────────────────────

/// Count `pipeline.phase` events across every per-spec NDJSON channel.
///
/// The emit-phase writer keeps the target phase under `payload.to`; we group
/// by that. Returns phases ordered by count desc.
#[must_use]
pub fn workflow_by_phase(repo_path: &Path) -> WorkflowBlock {
    let mut by_phase: HashMap<String, u64> = HashMap::new();
    for_each_ndjson_line(repo_path, |v| {
        // Match by the harness event name (the NDJSON record's `event` field).
        if v.get("event").and_then(Value::as_str) != Some("pipeline.phase") {
            return;
        }
        let payload = match v.get("payload") {
            Some(p) => p,
            None => return,
        };
        // emit-phase writes `{ to: "<PHASE>" }`; legacy rows used `phase`.
        let phase = payload
            .get("to")
            .or_else(|| payload.get("phase"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if phase.is_empty() {
            return;
        }
        *by_phase.entry(phase.to_string()).or_insert(0) += 1;
    });

    let mut rows: Vec<PhaseCount> = by_phase
        .into_iter()
        .map(|(phase, count)| PhaseCount { phase, count })
        .collect();
    rows.sort_by(|a, b| b.count.cmp(&a.count).then(a.phase.cmp(&b.phase)));
    WorkflowBlock { by_phase: rows }
}

// ── Tool breakdown ───────────────────────────────────────────────────────────

/// Top-N tool breakdown — counts every `tool.use` event across all per-spec
/// NDJSON channels, grouped by `payload.tool`. Returns up to 15 entries
/// ordered by count desc.
#[must_use]
pub fn tool_breakdown(repo_path: &Path) -> Vec<ToolCount> {
    const LIMIT: usize = 15;
    let mut by_tool: HashMap<String, u64> = HashMap::new();
    for_each_ndjson_line(repo_path, |v| {
        if v.get("event").and_then(Value::as_str) != Some("tool.use") {
            return;
        }
        let payload = match v.get("payload") {
            Some(p) => p,
            None => return,
        };
        // `tracker` writes `{ tool: "<Name>" }`; legacy rows used `tool_name`.
        let tool = payload
            .get("tool")
            .or_else(|| payload.get("tool_name"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if tool.is_empty() {
            return;
        }
        *by_tool.entry(tool.to_string()).or_insert(0) += 1;
    });

    let mut rows: Vec<ToolCount> = by_tool
        .into_iter()
        .map(|(tool_name, count)| ToolCount { tool_name, count })
        .collect();
    rows.sort_by(|a, b| b.count.cmp(&a.count).then(a.tool_name.cmp(&b.tool_name)));
    rows.truncate(LIMIT);
    rows
}

// ── Agent activity ───────────────────────────────────────────────────────────

/// Aggregate `agent.start` / `agent.stop` pairs by agent_type. Tokens are
/// omitted (they live in `pipeline.telemetry.run` spans, not in agent events);
/// duration is start→stop on `(session_id, actor)`. Errors come from
/// `agent.stop` payloads' `isError` field.
#[must_use]
pub fn agent_activity(repo_path: &Path) -> AgentActivityBlock {
    struct Acc {
        starts: u64,
        stops: u64,
        errors: u64,
        durations_ms: Vec<u64>,
        last_ts: Option<String>,
    }
    let mut acc: HashMap<String, Acc> = HashMap::new();
    // `(session_id|actor)` → start ts. Used to derive duration on the
    // matching `agent.stop`.
    let mut pending: HashMap<String, String> = HashMap::new();

    for_each_ndjson_line(repo_path, |v| {
        let event = v.get("event").and_then(Value::as_str).unwrap_or("");
        if event != "agent.start" && event != "agent.stop" {
            return;
        }
        // `agent_type` lives in the `payload` (tracker writes `subagentType`
        // for starts; falls back to actor for stops).
        let payload = v.get("payload");
        let agent_type = payload
            .and_then(|p| p.get("subagentType"))
            .or_else(|| payload.and_then(|p| p.get("agent_type")))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                v.get("actor")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "unknown".to_string());
        let ts = v.get("ts").and_then(Value::as_str).map(str::to_string);
        let session_id = v.get("session_id").and_then(Value::as_str).unwrap_or("");
        let actor = v.get("actor").and_then(Value::as_str).unwrap_or("");
        let pair_key = format!("{session_id}|{actor}");

        let entry = acc.entry(agent_type.clone()).or_insert(Acc {
            starts: 0,
            stops: 0,
            errors: 0,
            durations_ms: vec![],
            last_ts: None,
        });
        if let Some(ref t) = ts {
            if entry.last_ts.as_ref().is_none_or(|cur| t > cur) {
                entry.last_ts = Some(t.clone());
            }
        }
        if event == "agent.start" {
            entry.starts += 1;
            if let Some(t) = ts {
                pending.insert(pair_key, t);
            }
        } else {
            // agent.stop
            entry.stops += 1;
            let is_error = payload
                .and_then(|p| p.get("isError"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if is_error {
                entry.errors += 1;
            }
            if let (Some(t1_str), Some(t0_str)) = (ts.as_ref(), pending.remove(&pair_key)) {
                if let (Some(t0), Some(t1)) = (iso_to_ms(&t0_str), iso_to_ms(t1_str)) {
                    if t1 >= t0 {
                        entry.durations_ms.push((t1 - t0) as u64);
                    }
                }
            }
        }
    });

    let mut total_dispatches: u64 = 0;
    let mut total_errors: u64 = 0;
    let mut agents: Vec<AgentActivity> = acc
        .into_iter()
        .map(|(agent_type, a)| {
            total_dispatches += a.starts;
            total_errors += a.errors;
            let avg_duration_ms = if a.durations_ms.is_empty() {
                0
            } else {
                let sum: u64 = a.durations_ms.iter().sum();
                sum / a.durations_ms.len() as u64
            };
            AgentActivity {
                agent_type,
                starts: a.starts,
                stops: a.stops,
                errors: a.errors,
                avg_duration_ms,
                last_ts: a.last_ts,
            }
        })
        .collect();
    agents.sort_by(|a, b| b.starts.cmp(&a.starts).then_with(|| b.last_ts.cmp(&a.last_ts)));
    agents.truncate(10);

    AgentActivityBlock {
        total_dispatches,
        total_errors,
        agents,
    }
}

// ── Measured tokens ──────────────────────────────────────────────────────────

/// Sum `tokens_in` + `tokens_out` (the writer pre-extracts these onto the
/// NDJSON record) across every `pipeline.telemetry.run` event. `tokens_today`
/// filters by `ts >= today (UTC midnight)`.
#[must_use]
pub fn measured(repo_path: &Path) -> MeasuredBlock {
    let today_prefix = today_iso_prefix();
    let mut tokens_total: u64 = 0;
    let mut tokens_today: u64 = 0;
    for_each_ndjson_line(repo_path, |v| {
        if v.get("event").and_then(Value::as_str) != Some("pipeline.telemetry.run") {
            return;
        }
        // Prefer the record-level pre-extracted hints (the writer fills these
        // when the payload carries `tokens_in`/`tokens_out`). Fall back to
        // payload-level fields the OTEL collector writes
        // (`extra.input_tokens` / `extra.output_tokens`), then to the bare
        // top-level keys.
        let tin = v.get("tokens_in").and_then(Value::as_u64).unwrap_or_else(|| {
            v.get("payload")
                .and_then(|p| p.get("extra"))
                .and_then(|e| e.get("input_tokens"))
                .and_then(Value::as_u64)
                .or_else(|| v.get("payload").and_then(|p| p.get("input_tokens")).and_then(Value::as_u64))
                .unwrap_or(0)
        });
        let tout = v.get("tokens_out").and_then(Value::as_u64).unwrap_or_else(|| {
            v.get("payload")
                .and_then(|p| p.get("extra"))
                .and_then(|e| e.get("output_tokens"))
                .and_then(Value::as_u64)
                .or_else(|| v.get("payload").and_then(|p| p.get("output_tokens")).and_then(Value::as_u64))
                .unwrap_or(0)
        });
        let row_tokens = tin + tout;
        tokens_total += row_tokens;
        let ts = v.get("ts").and_then(Value::as_str).unwrap_or("");
        if !today_prefix.is_empty() && ts.starts_with(&today_prefix) {
            tokens_today += row_tokens;
        }
    });
    MeasuredBlock {
        tokens_total,
        tokens_today,
    }
}

/// `YYYY-MM-DD` of "today UTC" — used as a string-prefix filter on ISO-8601
/// timestamps. Empty string on any clock failure.
fn today_iso_prefix() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

// ── Session start + live activity ────────────────────────────────────────────

/// ISO-8601 cut-off marking the start of the current session, or `None` when
/// no NDJSON event with a `session_id` has been observed under `repo_path`.
#[must_use]
pub fn session_start_ts(repo_path: &Path) -> Option<String> {
    let spec_base = repo_path.join(".claude").join("spec");
    let _ = std::fs::read_dir(&spec_base).ok()?;

    // First pass: find the most-recent (latest ts) event that carries a
    // non-empty session_id.
    let mut latest: Option<(String, String)> = None;
    for_each_ndjson_line(repo_path, |v| {
        let session = v.get("session_id").and_then(Value::as_str).unwrap_or("");
        if session.is_empty() {
            return;
        }
        let ts = v.get("ts").and_then(Value::as_str).unwrap_or("");
        if ts.is_empty() {
            return;
        }
        let take = match latest.as_ref() {
            None => true,
            Some((prev, _)) => ts > prev.as_str(),
        };
        if take {
            latest = Some((ts.to_string(), session.to_string()));
        }
    });

    let (_, target_session) = latest?;

    // Second pass: earliest ts sharing the target session_id.
    let mut earliest: Option<String> = None;
    for_each_ndjson_line(repo_path, |v| {
        let session = v.get("session_id").and_then(Value::as_str).unwrap_or("");
        if session != target_session {
            return;
        }
        let ts = v.get("ts").and_then(Value::as_str).unwrap_or("");
        if ts.is_empty() {
            return;
        }
        let take = match earliest.as_ref() {
            None => true,
            Some(prev) => ts < prev.as_str(),
        };
        if take {
            earliest = Some(ts.to_string());
        }
    });
    earliest
}

/// Live activity snapshot — most-recent NDJSON event's phase/spec/session.
/// Still a reduced shape vs the legacy SQLite reader (no 60-bucket sparkline,
/// no per-phase fan-out); restoring those costs a dedicated reader and is
/// scoped to a separate follow-up.
#[must_use]
pub fn live_activity(repo_path: &Path) -> LiveActivity {
    let mut latest_ts: Option<String> = None;
    let mut latest_payload: Option<Value> = None;
    let mut latest_record: Option<Value> = None;
    for_each_ndjson_line(repo_path, |v| {
        let ts = v.get("ts").and_then(Value::as_str).unwrap_or("").to_string();
        if ts.is_empty() {
            return;
        }
        let take = match latest_ts.as_ref() {
            None => true,
            Some(prev) => ts > *prev,
        };
        if take {
            latest_ts = Some(ts);
            latest_payload = v.get("payload").cloned();
            latest_record = Some(v.clone());
        }
    });
    let payload = latest_payload.unwrap_or_default();
    let record = latest_record.unwrap_or_default();
    LiveActivity {
        events_last_5m: 0,
        current_phase: payload
            .get("to")
            .or_else(|| payload.get("phase"))
            .and_then(Value::as_str)
            .map(str::to_string),
        current_spec: record
            .get("spec")
            .or_else(|| payload.get("spec"))
            .and_then(Value::as_str)
            .map(str::to_string),
        last_event_ts: latest_ts,
        session_id: record
            .get("session_id")
            .or_else(|| payload.get("session_id"))
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

// ── Sessions ─────────────────────────────────────────────────────────────────

/// `dashboard_sessions` — list Claude Code sessions for the active workspace.
///
/// Aggregates one [`SessionRow`] per `.claude/.session/{id}/.events/` directory:
/// earliest/latest event `ts`, the last-seen `spec`, the `cwd` from
/// `session.start`, an event count, and an open/closed flag (recency, since no
/// session-end event exists). The `unknown` directory — a known
/// attribution-leak bucket for events whose `session_id` couldn't be resolved
/// at emit time — is labelled (`is_unknown_bucket`) rather than hidden.
///
/// Fail-open: a missing `.session` root yields an empty list. Rows are sorted
/// open-first, then most-recent activity first. `limit` (when `Some`) caps the
/// returned rows after sorting.
#[tauri::command]
#[must_use]
pub fn dashboard_sessions(repo_path: String, limit: Option<usize>) -> Vec<SessionRow> {
    let session_root = PathBuf::from(&repo_path)
        .join(".claude")
        .join(".session");
    let Ok(entries) = std::fs::read_dir(&session_root) else {
        return Vec::new();
    };

    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut rows: Vec<SessionRow> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        let mut records: Vec<Value> = Vec::new();
        collect_one_dir(&path.join(".events"), &mut records);

        let mut earliest: Option<String> = None;
        let mut latest: Option<String> = None;
        let mut last_spec: Option<(String, String)> = None; // (ts, spec)
        let mut cwd: Option<String> = None;
        let mut event_count: u64 = 0;

        for record in &records {
            event_count += 1;
            let ts = record.get("ts").and_then(Value::as_str).unwrap_or("");
            if !ts.is_empty() {
                if earliest.as_deref().map_or(true, |e| ts < e) {
                    earliest = Some(ts.to_string());
                }
                if latest.as_deref().map_or(true, |l| ts > l) {
                    latest = Some(ts.to_string());
                }
                // Track the spec of the latest event that carried one.
                if let Some(spec) = record.get("spec").and_then(Value::as_str) {
                    if !spec.is_empty()
                        && last_spec.as_ref().map_or(true, |(prev, _)| ts >= prev.as_str())
                    {
                        last_spec = Some((ts.to_string(), spec.to_string()));
                    }
                }
            }
            // `cwd` lives in the `session.start` payload; take the first seen.
            if cwd.is_none() {
                if let Some(c) = record
                    .get("payload")
                    .and_then(|p| p.get("cwd"))
                    .and_then(Value::as_str)
                {
                    if !c.is_empty() {
                        cwd = Some(c.to_string());
                    }
                }
            }
        }

        // Skip directories with no parseable events entirely — an empty dir is
        // not a session worth listing.
        if event_count == 0 {
            continue;
        }

        let status = match latest.as_deref().and_then(iso_to_ms) {
            Some(ms) if now_ms - ms <= SESSION_OPEN_WINDOW_MS => "open",
            _ => "closed",
        }
        .to_string();

        rows.push(SessionRow {
            id: id.clone(),
            slug: String::new(),
            started_at: earliest.unwrap_or_default(),
            last_activity_at: latest,
            last_spec: last_spec.map(|(_, spec)| spec),
            cwd,
            status,
            event_count,
            is_unknown_bucket: id == "unknown",
        });
    }

    // Open sessions first, then most-recent activity first.
    rows.sort_by(|a, b| {
        let a_open = a.status == "open";
        let b_open = b.status == "open";
        b_open
            .cmp(&a_open)
            .then_with(|| b.last_activity_at.cmp(&a.last_activity_at))
    });

    if let Some(n) = limit {
        rows.truncate(n);
    }
    rows
}

// ── Friction + collector health ──────────────────────────────────────────────

/// Friction entries — read from `.claude/.metrics/friction.json`. Empty vec
/// when the file is absent.
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

#[must_use]
pub fn collector_health_from_freshness(last_canary_at: Option<String>) -> CollectorHealth {
    CollectorHealth {
        healthy: last_canary_at.is_some(),
        last_canary_at,
        last_canary_level: None,
        last_canary_msg: None,
    }
}

#[tauri::command]
#[must_use]
pub fn collector_health(repo_path: String) -> CollectorHealth {
    let base = PathBuf::from(&repo_path);
    collector_health_impl(&base)
}

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
    let parsed: Value = serde_json::from_str(line).unwrap_or_default();
    CollectorHealth {
        healthy: true,
        last_canary_at: parsed.get("ts").and_then(Value::as_str).map(str::to_string),
        last_canary_level: parsed.get("level").and_then(Value::as_str).map(str::to_string),
        last_canary_msg: parsed.get("msg").and_then(Value::as_str).map(str::to_string),
    }
}

/// Public ISO→ms parser kept for callers that compose the value into other
/// payloads.
#[must_use]
pub fn parse_iso_ms_pub(s: &str) -> Option<i64> {
    iso_to_ms(s)
}

// ── Economy scope DTO ────────────────────────────────────────────────────────
//
// The frontend invokes the `dashboard_economy_*` + `dashboard_prompt_economy`
// commands with `{ scope }` (the discriminated union mirrored in
// `apps/dashboard/src/lib/types/economy.ts`). Restoring the correct argument
// shape here prevents a `IpcError`/panic on every economy widget — even
// though the body is still a default placeholder (see the "behavioural gap"
// note on each command).

/// JS-friendly mirror of `mustard_core::domain::economy::EconomyScope`. Internally
/// tagged on `kind` so the TS side can model it as a clean discriminated union.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EconomyScopeDto {
    Project { project: String },
    Spec { project: String, spec: String },
    Wave { project: String, spec: String, wave: String },
    AllProjects { projects: Vec<String> },
}

// ── Tauri-command surface ────────────────────────────────────────────────────
//
// W7D of [[2026-05-26-no-sqlite-git-source-of-truth]] wired these commands
// against the real NDJSON readers in `mustard_core::domain::economy::reader::*`
// (migrated in W7A). The behavioural gap left by wave-21 is closed —
// dashboard pages now see live data instead of `Default::default()`.

impl EconomyScopeDto {
    /// Translate the Tauri DTO into the core `(project_root, scope)` tuple
    /// the readers expect. Returns the absolute project root the scope is
    /// rooted at (used to open NDJSON files), plus the core scope value.
    /// `AllProjects` returns the first project's root as the lookup anchor
    /// (the multi-project reader fans out per-project anyway).
    fn to_core(&self) -> (PathBuf, mustard_core::domain::economy::EconomyScope) {
        use mustard_core::domain::economy::scope::{
            ProjectPath as CoreProjectPath, SpecId as CoreSpecId, WaveId as CoreWaveId,
        };
        use mustard_core::domain::economy::EconomyScope as CoreScope;
        match self {
            EconomyScopeDto::Project { project } => {
                let root = PathBuf::from(project);
                (root.clone(), CoreScope::Project(CoreProjectPath::new(root)))
            }
            EconomyScopeDto::Spec { project, spec } => {
                let root = PathBuf::from(project);
                (
                    root.clone(),
                    CoreScope::Spec {
                        project: CoreProjectPath::new(root),
                        spec: CoreSpecId::new(spec),
                    },
                )
            }
            EconomyScopeDto::Wave {
                project,
                spec,
                wave,
            } => {
                let root = PathBuf::from(project);
                (
                    root.clone(),
                    CoreScope::Wave {
                        project: CoreProjectPath::new(root),
                        spec: CoreSpecId::new(spec),
                        wave: CoreWaveId::new(wave),
                    },
                )
            }
            EconomyScopeDto::AllProjects { projects } => {
                let root = projects
                    .first()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."));
                let cores: Vec<CoreProjectPath> =
                    projects.iter().map(CoreProjectPath::new).collect();
                (root, CoreScope::AllProjects(cores))
            }
        }
    }
}

/// Walk every NDJSON file under the three canonical event sinks:
/// `<root>/.claude/spec/*/.events/`, `<root>/.claude/spec/*/wave-*/events/`
/// (and `wave-*/.events/`), and `<root>/.claude/.session/*/.events/`. Mirrors
/// the coverage of `mustard_core::domain::economy::reader::ndjson_paths` so the
/// per-page aggregators see the same complete event slice the core readers do.
///
/// `pub(crate)` so the Onda-2 aggregators in `lib.rs` and `spec_views.rs` reuse
/// the same walker (the directive's "complete walker" requirement — never the
/// spec-only `for_each_ndjson_line`, which misses `.session/` and wave subdirs).
pub(crate) fn walk_ndjson_events(root: &Path) -> Vec<Value> {
    let mut out = Vec::new();
    let claude = root.join(".claude");

    // Per-spec channel + wave subdirs.
    if let Ok(specs) = std::fs::read_dir(claude.join("spec")) {
        for spec_entry in specs.flatten() {
            let spec_path = spec_entry.path();
            if !spec_path.is_dir() {
                continue;
            }
            collect_one_dir(&spec_path.join(".events"), &mut out);
            if let Ok(waves) = std::fs::read_dir(&spec_path) {
                for wave_entry in waves.flatten() {
                    let wp = wave_entry.path();
                    if !wp.is_dir() {
                        continue;
                    }
                    let name = wp.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !name.starts_with("wave-") {
                        continue;
                    }
                    collect_one_dir(&wp.join("events"), &mut out);
                    collect_one_dir(&wp.join(".events"), &mut out);
                }
            }
        }
    }

    // Cross-spec session sink.
    if let Ok(sessions) = std::fs::read_dir(claude.join(".session")) {
        for entry in sessions.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            collect_one_dir(&path.join(".events"), &mut out);
        }
    }

    out
}

/// Canonical harness event NAME for a raw record (`"event"` ?? `"kind"`).
/// Re-exported `pub(crate)` for the Onda-2 aggregators in `lib.rs` /
/// `spec_views.rs` so every cross-spec fold matches the harness NAME, never the
/// logical `kind` class.
#[must_use]
pub(crate) fn event_name_of(record: &Value) -> &str {
    event_name(record)
}

/// `pub(crate)` ISO-8601 → epoch-ms for the Onda-2 aggregators (weekday × hour
/// heatmap, duration math). Same parser the attribution + session readers use.
#[must_use]
pub(crate) fn iso_to_ms_crate(s: &str) -> Option<i64> {
    iso_to_ms(s)
}

fn collect_one_dir(dir: &Path, out: &mut Vec<Value>) {
    let Ok(files) = std::fs::read_dir(dir) else {
        return;
    };
    for file in files.flatten() {
        let p = file.path();
        if p.extension().and_then(|s| s.to_str()) != Some("ndjson") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                out.push(v);
            }
        }
    }
}

/// `dashboard_prompt_economy` — aggregates three independently-measured blocks
/// from the NDJSON event channels:
///
/// 1. `cost`         — Anthropic-measured USD from `pipeline.telemetry.metric`
///                     (`claude_code.cost.usage`).
/// 2. `subtractions` — counterfactual bytes from `pipeline.economy.savings.*`
///                     (`tokens_saved × 4` byte proxy, grouped by wave).
/// 3. `claude_events`— operational counters from
///                     `pipeline.telemetry.metric:claude_code.active_time` + session count.
///
/// Plus a `freshness` block surfacing the most-recent timestamps + OTEL
/// collector health (re-uses [`collector_health_block`]).
#[tauri::command]
#[must_use]
pub fn dashboard_prompt_economy(scope: EconomyScopeDto) -> Value {
    let (root, _core_scope) = scope.to_core();
    let events = walk_ndjson_events(&root);

    // ── cost block ──
    let mut usd_total = 0.0f64;
    let mut by_model: HashMap<String, f64> = HashMap::new();
    let mut by_session: HashMap<String, f64> = HashMap::new();
    let mut last_metric_ts: Option<String> = None;
    let mut sessions_seen: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut active_seconds = 0.0f64;
    for ev in &events {
        let ev_name = ev.get("event").and_then(Value::as_str).unwrap_or("");
        if ev_name != "pipeline.telemetry.metric" {
            continue;
        }
        let payload = ev.get("payload").cloned().unwrap_or_default();
        let metric = payload.get("metric").and_then(Value::as_str).unwrap_or("");
        let sum = payload.get("sum").and_then(Value::as_f64).unwrap_or(0.0);
        let session = payload
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let model = payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        if metric == "claude_code.cost.usage" {
            usd_total += sum;
            *by_model.entry(model).or_insert(0.0) += sum;
            if !session.is_empty() {
                *by_session.entry(session.clone()).or_insert(0.0) += sum;
                sessions_seen.insert(session);
            }
        } else if metric == "claude_code.active_time" {
            active_seconds += sum;
        }
        if let Some(ts) = ev.get("ts").and_then(Value::as_str) {
            if last_metric_ts.as_deref().map_or(true, |cur| ts > cur) {
                last_metric_ts = Some(ts.to_string());
            }
        }
    }

    // ── subtractions block ──
    let mut subtractions_total_tokens = 0i64;
    let mut subtractions_event_count = 0i64;
    let mut subtractions_by_wave: HashMap<String, (i64, i64)> = HashMap::new();
    let mut last_subtraction_ts: Option<String> = None;
    for ev in &events {
        let ev_name = ev.get("event").and_then(Value::as_str).unwrap_or("");
        if !ev_name.starts_with("pipeline.economy.savings.") {
            continue;
        }
        let payload = ev.get("payload").cloned().unwrap_or_default();
        let tokens = payload
            .get("tokens_saved")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        subtractions_total_tokens += tokens;
        subtractions_event_count += 1;
        let wave = payload
            .get("wave_id")
            .and_then(Value::as_str)
            .unwrap_or("unattributed")
            .to_string();
        let entry = subtractions_by_wave.entry(wave).or_insert((0, 0));
        entry.0 += tokens;
        entry.1 += 1;
        if let Some(ts) = ev.get("ts").and_then(Value::as_str) {
            if last_subtraction_ts.as_deref().map_or(true, |cur| ts > cur) {
                last_subtraction_ts = Some(ts.to_string());
            }
        }
    }

    let mut by_model_arr: Vec<Value> = by_model
        .into_iter()
        .map(|(model, usd)| serde_json::json!({ "model": model, "usd": usd }))
        .collect();
    by_model_arr.sort_by(|a, b| {
        b["usd"]
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&a["usd"].as_f64().unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut by_session_arr: Vec<Value> = by_session
        .into_iter()
        .map(|(session, usd)| serde_json::json!({ "session_id": session, "usd": usd }))
        .collect();
    by_session_arr.sort_by(|a, b| {
        b["usd"]
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&a["usd"].as_f64().unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let by_wave_arr: Vec<Value> = {
        let mut rows: Vec<(String, (i64, i64))> = subtractions_by_wave.into_iter().collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows.into_iter()
            .map(|(wave, (tokens, count))| {
                serde_json::json!({
                    "wave": wave,
                    "sent_bytes": 0,
                    "avoided_bytes": tokens * 4,
                    "count": count,
                })
            })
            .collect()
    };

    let collector = collector_health_impl(&root);
    serde_json::json!({
        "cost": {
            "usd_total": usd_total,
            "by_model": by_model_arr,
            "by_session": by_session_arr,
        },
        "subtractions": {
            "context_sent_bytes": 0,
            "context_avoided_bytes": subtractions_total_tokens * 4,
            "event_count": subtractions_event_count,
            "by_wave": by_wave_arr,
            "session_sent_bytes": 0,
            "session_avoided_bytes": subtractions_total_tokens * 4,
            "session_count": sessions_seen.len() as i64,
            "session_known": !sessions_seen.is_empty(),
        },
        "claude_events": {
            "session_count": sessions_seen.len() as i64,
            "active_time_seconds": active_seconds,
        },
        "freshness": {
            "last_metric_ts": last_metric_ts,
            "last_subtraction_ts": last_subtraction_ts,
            "otel_healthy": collector.healthy,
            "canary_tail": collector.last_canary_msg.map(|m| vec![m]),
        }
    })
}

#[tauri::command]
#[must_use]
pub fn dashboard_economy_summary(scope: EconomyScopeDto) -> Value {
    let (root, core_scope) = scope.to_core();
    let summary = mustard_core::domain::economy::economy_summary(&root, core_scope)
        .unwrap_or_default();
    serde_json::to_value(summary).unwrap_or_else(|_| serde_json::json!({}))
}

#[tauri::command]
#[must_use]
pub fn dashboard_economy_savings_breakdown(scope: EconomyScopeDto) -> Value {
    let (root, core_scope) = scope.to_core();
    let breakdown = mustard_core::domain::economy::savings_breakdown(&root, core_scope)
        .unwrap_or_default();
    serde_json::to_value(breakdown).unwrap_or_else(|_| serde_json::json!({}))
}

#[tauri::command]
#[must_use]
pub fn dashboard_economy_context_routing(scope: EconomyScopeDto) -> Value {
    let (root, core_scope) = scope.to_core();
    let metrics = mustard_core::domain::economy::context_routing_quality(&root, core_scope)
        .unwrap_or_default();
    serde_json::to_value(metrics).unwrap_or_else(|_| serde_json::json!({}))
}

#[tauri::command]
#[must_use]
pub fn dashboard_economy_per_spec_costs(scope: EconomyScopeDto) -> Value {
    let (root, core_scope) = scope.to_core();
    let rows = mustard_core::domain::economy::per_spec_costs(&root, core_scope)
        .unwrap_or_default();
    serde_json::to_value(rows).unwrap_or_else(|_| serde_json::json!([]))
}

#[tauri::command]
#[must_use]
pub fn dashboard_economy_per_wave_costs(scope: EconomyScopeDto) -> Value {
    let (root, core_scope) = scope.to_core();
    let rows = mustard_core::domain::economy::per_wave_costs(&root, core_scope)
        .unwrap_or_default();
    serde_json::to_value(rows).unwrap_or_else(|_| serde_json::json!([]))
}

/// Spec trace — up to a 4-level tree (spec → wave → agent → tool).
///
/// W7D restored the full tree shape. Roll-up tokens per agent come from
/// [`mustard_core::domain::economy::per_agent_costs`] (scope-filtered to the spec).
/// `tool.use` events are bucketed by `wave` (record-level int/string, legacy
/// `payload.wave_id` fallback) then by `agent` (the dispatch that owned the
/// `tool_use_id`, resolved via the `agent.start` correlation; else the
/// record-level `actor`). Real `tool.use` records carry neither `wave_id` nor
/// `tool_use_id`, so a tool with no wave AND no agent attaches directly under
/// the spec root instead of collapsing into synthetic `root`/`main` branches.
#[tauri::command]
#[must_use]
pub fn dashboard_spec_trace(project_path: String, spec_name: String) -> Value {
    use mustard_core::domain::economy::scope::{ProjectPath as CoreProjectPath, SpecId as CoreSpecId};
    use mustard_core::domain::economy::EconomyScope as CoreScope;

    let base = PathBuf::from(&project_path);
    let spec_dir = ClaudePaths::for_project(&base)
        .ok()
        .and_then(|p| p.for_spec(&spec_name).ok())
        .map(|s| s.dir().to_path_buf())
        .unwrap_or_else(|| base.join(".claude").join("spec").join(&spec_name));

    // Per-agent token totals (scoped to this spec) — used to label the
    // agent-level nodes with roll-up cost/tokens.
    let core_scope = CoreScope::Spec {
        project: CoreProjectPath::new(&base),
        spec: CoreSpecId::new(&spec_name),
    };
    let agent_costs = mustard_core::domain::economy::per_agent_costs(&base, core_scope)
        .unwrap_or_default();
    let agent_tokens: HashMap<String, i64> = agent_costs
        .iter()
        .map(|a| (a.agent_id.0.clone(), a.tokens))
        .collect();
    let agent_cost_micros: HashMap<String, i64> = agent_costs
        .iter()
        .map(|a| (a.agent_id.0.clone(), a.cost_usd_micros))
        .collect();

    // Walk every NDJSON file under the spec dir (root + wave subdirs).
    let mut all_events: Vec<Value> = Vec::new();
    collect_one_dir(&spec_dir.join(".events"), &mut all_events);
    if let Ok(waves) = std::fs::read_dir(&spec_dir) {
        for wave_entry in waves.flatten() {
            let wp = wave_entry.path();
            if !wp.is_dir() {
                continue;
            }
            let name = wp.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.starts_with("wave-") {
                continue;
            }
            collect_one_dir(&wp.join(".events"), &mut all_events);
            collect_one_dir(&wp.join("events"), &mut all_events);
        }
    }

    // Pass 1: build `tool_use_id -> agent_id` map from `agent.start` events.
    let mut tool_to_agent: HashMap<String, String> = HashMap::new();
    for ev in &all_events {
        let ev_name = ev.get("event").and_then(Value::as_str).unwrap_or("");
        if ev_name != "agent.start" {
            continue;
        }
        let payload = match ev.get("payload") {
            Some(p) => p,
            None => continue,
        };
        let tool_use_id = payload
            .get("tool_use_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        let agent_id = payload
            .get("agent_id")
            .or_else(|| payload.get("subagentType"))
            .and_then(Value::as_str)
            .unwrap_or("unattributed")
            .to_string();
        if let Some(tu) = tool_use_id {
            tool_to_agent.insert(tu, agent_id);
        }
    }

    // Pass 2: bucket `tool.use` events by (wave, agent).
    //
    // wave: record-level `wave` (real harness shape — int or string), falling
    //       back to legacy `payload.wave_id`. `None` when neither is present.
    // agent: the dispatch that owned the event's `tool_use_id` (resolved via the
    //        `agent.start` correlation in pass 1), else the record-level `actor`,
    //        else legacy `payload.agent_id`. `None` when unattributable.
    //
    // When BOTH are `None`, the tool attaches directly under the spec root
    // rather than under synthetic `wave="root"`/`agent="main"` nodes (real
    // `tool.use` records carry neither `wave_id` nor `tool_use_id`, so the old
    // synthetic buckets collapsed every tool into one fake branch). Tools that
    // DO carry attribution still nest spec → wave → agent → tool.
    #[derive(Default)]
    struct WaveBucket {
        agents: BTreeMap<String, Vec<Value>>,
    }
    let mut by_wave: BTreeMap<String, WaveBucket> = BTreeMap::new();
    let mut loose_tools: Vec<Value> = Vec::new();
    for ev in &all_events {
        let ev_name = ev.get("event").and_then(Value::as_str).unwrap_or("");
        if ev_name != "tool.use" {
            continue;
        }
        let payload = ev.get("payload").cloned().unwrap_or_default();
        // Record-level `wave` may be an int or a string; normalise to a label.
        let wave_id = ev
            .get("wave")
            .and_then(|w| match w {
                Value::Number(n) => Some(format!("wave-{n}")),
                Value::String(s) if !s.is_empty() => Some(s.clone()),
                _ => None,
            })
            .or_else(|| {
                payload
                    .get("wave_id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            });
        let tool_use_id = payload
            .get("tool_use_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        let agent_id = tool_use_id
            .as_deref()
            .and_then(|tu| tool_to_agent.get(tu).cloned())
            .or_else(|| {
                ev.get("actor")
                    .and_then(Value::as_str)
                    .filter(|a| !a.is_empty())
                    .map(str::to_string)
            })
            .or_else(|| {
                payload
                    .get("agent_id")
                    .and_then(Value::as_str)
                    .filter(|a| !a.is_empty())
                    .map(str::to_string)
            });

        let tool_name = payload
            .get("tool")
            .or_else(|| payload.get("tool_name"))
            .and_then(Value::as_str)
            .unwrap_or("tool")
            .to_string();
        let target_label = payload
            .get("target")
            .and_then(|t| t.as_object())
            .and_then(|o| {
                o.get("file_path")
                    .or_else(|| o.get("file"))
                    .or_else(|| o.get("command"))
                    .or_else(|| o.get("description"))
            })
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let label = if target_label.is_empty() {
            tool_name
        } else {
            format!("{tool_name} · {target_label}")
        };
        let ts = ev.get("ts").and_then(Value::as_str).map(str::to_string);
        let tool_node = serde_json::json!({
            "kind": "tool",
            "label": label,
            "tokens": null,
            "duration_ms": null,
            "ts": ts,
            "payload": payload,
            "children": [],
        });
        match (wave_id, agent_id) {
            (None, None) => loose_tools.push(tool_node),
            (wave, agent) => {
                by_wave
                    .entry(wave.unwrap_or_else(|| "root".to_string()))
                    .or_default()
                    .agents
                    .entry(agent.unwrap_or_else(|| "main".to_string()))
                    .or_default()
                    .push(tool_node);
            }
        }
    }

    // Build the tree. Attributed tools nest spec → wave → agent → tool;
    // unattributed tools (`loose_tools`) attach as direct children of the spec,
    // after the wave branches. The frontend `<ExecutionTrace>` recurses over
    // `children` regardless of depth, so a spec → tool leaf renders correctly.
    let mut children: Vec<Value> = by_wave
        .into_iter()
        .map(|(wave_id, bucket)| {
            let agent_nodes: Vec<Value> = bucket
                .agents
                .into_iter()
                .map(|(agent_id, tool_nodes)| {
                    let tokens = agent_tokens.get(&agent_id).copied();
                    let cost_micros = agent_cost_micros.get(&agent_id).copied();
                    serde_json::json!({
                        "kind": "agent",
                        "label": agent_id,
                        "tokens": tokens,
                        "cost_usd_micros": cost_micros,
                        "duration_ms": null,
                        "ts": null,
                        "payload": null,
                        "children": tool_nodes,
                    })
                })
                .collect();
            serde_json::json!({
                "kind": "wave",
                "label": wave_id,
                "tokens": null,
                "duration_ms": null,
                "ts": null,
                "payload": null,
                "children": agent_nodes,
            })
        })
        .collect();
    children.extend(loose_tools);

    serde_json::json!({
        "kind": "spec",
        "label": spec_name,
        "tokens": null,
        "duration_ms": null,
        "ts": null,
        "payload": null,
        "children": children,
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
            payload["extra"]["tool_use_id"] = Value::String(tu.to_string());
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
        let lines = format!(
            "{}\n{}\n",
            span_line("sess-B", Some("tu-x"), "spec-old", "2026-05-27T09:00:00.000Z"),
            span_line("sess-B", Some("tu-y"), "spec-new", "2026-05-27T09:30:00.000Z"),
        );
        write_event(tmp.path(), "spec-old", "otel.ndjson", &lines);

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
            r#"{"event":"pipeline.phase","kind":"pipeline","ts":"2026-05-27T09:00:00.000Z","session_id":"s","spec":"alpha","payload":{"to":"PLAN"}}"#,
            r#"{"event":"pipeline.phase","kind":"pipeline","ts":"2026-05-27T09:05:00.000Z","session_id":"s","spec":"alpha","payload":{"to":"EXECUTE"}}"#,
        );
        write_event(tmp.path(), "alpha", "events.ndjson", &lines);
        let live = live_activity(tmp.path());
        assert_eq!(live.current_phase.as_deref(), Some("EXECUTE"));
        assert_eq!(live.current_spec.as_deref(), Some("alpha"));
    }

    #[test]
    fn session_start_returns_earliest_ts_in_latest_session() {
        let tmp = TempDir::new().unwrap();
        let lines = format!(
            "{}\n{}\n{}\n{}\n",
            r#"{"event":"k","kind":"other","ts":"2026-05-27T08:00:00.000Z","session_id":"sess-1","spec":"a","payload":{}}"#,
            r#"{"event":"k","kind":"other","ts":"2026-05-27T09:00:00.000Z","session_id":"sess-2","spec":"a","payload":{}}"#,
            r#"{"event":"k","kind":"other","ts":"2026-05-27T08:30:00.000Z","session_id":"sess-2","spec":"a","payload":{}}"#,
            r#"{"event":"k","kind":"other","ts":"2026-05-27T10:00:00.000Z","session_id":"sess-2","spec":"a","payload":{}}"#,
        );
        write_event(tmp.path(), "a", "events.ndjson", &lines);
        let start = session_start_ts(tmp.path()).expect("should resolve");
        assert_eq!(start, "2026-05-27T08:30:00.000Z");
    }

    #[test]
    fn workflow_by_phase_counts_pipeline_phase_events() {
        let tmp = TempDir::new().unwrap();
        let lines = concat!(
            r#"{"event":"pipeline.phase","kind":"pipeline","ts":"2026-05-27T09:00:00.000Z","spec":"a","payload":{"to":"ANALYZE"}}"#, "\n",
            r#"{"event":"pipeline.phase","kind":"pipeline","ts":"2026-05-27T09:05:00.000Z","spec":"a","payload":{"to":"PLAN"}}"#, "\n",
            r#"{"event":"pipeline.phase","kind":"pipeline","ts":"2026-05-27T09:10:00.000Z","spec":"a","payload":{"to":"PLAN"}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:11:00.000Z","spec":"a","payload":{"tool":"Read"}}"#, "\n",
        );
        write_event(tmp.path(), "a", "events.ndjson", lines);
        let block = workflow_by_phase(tmp.path());
        let plan = block.by_phase.iter().find(|p| p.phase == "PLAN").expect("PLAN row");
        let analyze = block.by_phase.iter().find(|p| p.phase == "ANALYZE").expect("ANALYZE row");
        assert_eq!(plan.count, 2);
        assert_eq!(analyze.count, 1);
    }

    #[test]
    fn tool_breakdown_aggregates_tool_use_events() {
        let tmp = TempDir::new().unwrap();
        let lines = concat!(
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:00:00.000Z","spec":"a","payload":{"tool":"Read"}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:01:00.000Z","spec":"a","payload":{"tool":"Read"}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:02:00.000Z","spec":"a","payload":{"tool":"Edit"}}"#, "\n",
            r#"{"event":"pipeline.phase","kind":"pipeline","ts":"2026-05-27T09:03:00.000Z","spec":"a","payload":{"to":"PLAN"}}"#, "\n",
        );
        write_event(tmp.path(), "a", "events.ndjson", lines);
        let rows = tool_breakdown(tmp.path());
        let read = rows.iter().find(|r| r.tool_name == "Read").expect("Read row");
        let edit = rows.iter().find(|r| r.tool_name == "Edit").expect("Edit row");
        assert_eq!(read.count, 2);
        assert_eq!(edit.count, 1);
    }

    #[test]
    fn agent_activity_aggregates_start_stop_pairs() {
        let tmp = TempDir::new().unwrap();
        let lines = concat!(
            r#"{"event":"agent.start","kind":"agent","ts":"2026-05-27T09:00:00.000Z","spec":"a","session_id":"s","actor":"explore-1","payload":{"subagentType":"Explore"}}"#, "\n",
            r#"{"event":"agent.stop","kind":"agent","ts":"2026-05-27T09:00:30.000Z","spec":"a","session_id":"s","actor":"explore-1","payload":{"subagentType":"Explore","isError":false}}"#, "\n",
            r#"{"event":"agent.start","kind":"agent","ts":"2026-05-27T09:01:00.000Z","spec":"a","session_id":"s","actor":"gp-1","payload":{"subagentType":"general-purpose"}}"#, "\n",
        );
        write_event(tmp.path(), "a", "events.ndjson", lines);
        let block = agent_activity(tmp.path());
        assert_eq!(block.total_dispatches, 2);
        let explore = block.agents.iter().find(|a| a.agent_type == "Explore").expect("Explore row");
        assert_eq!(explore.starts, 1);
        assert_eq!(explore.stops, 1);
    }

    #[test]
    fn measured_sums_telemetry_run_tokens() {
        let tmp = TempDir::new().unwrap();
        let lines = concat!(
            r#"{"event":"pipeline.telemetry.run","kind":"pipeline.telemetry.run","ts":"2026-05-27T09:00:00.000Z","spec":"a","payload":{"extra":{"input_tokens":1000,"output_tokens":500}},"tokens_in":1000,"tokens_out":500}"#, "\n",
            r#"{"event":"pipeline.telemetry.run","kind":"pipeline.telemetry.run","ts":"2026-05-27T09:01:00.000Z","spec":"a","payload":{"extra":{"input_tokens":200,"output_tokens":100}},"tokens_in":200,"tokens_out":100}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:02:00.000Z","spec":"a","payload":{"tool":"Read"}}"#, "\n",
        );
        write_event(tmp.path(), "a", "events.ndjson", lines);
        let block = measured(tmp.path());
        assert_eq!(block.tokens_total, 1800);
    }

    #[test]
    fn spec_trace_lists_tool_use_events_under_spec_root() {
        let tmp = TempDir::new().unwrap();
        let lines = concat!(
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:00:00.000Z","spec":"alpha","payload":{"tool":"Read","target":{"file_path":"src/foo.rs"}}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:01:00.000Z","spec":"alpha","payload":{"tool":"Edit","target":{"file_path":"src/bar.rs"}}}"#, "\n",
            r#"{"event":"pipeline.phase","kind":"pipeline","ts":"2026-05-27T09:02:00.000Z","spec":"alpha","payload":{"to":"PLAN"}}"#, "\n",
        );
        write_event(tmp.path(), "alpha", "events.ndjson", lines);
        let trace = dashboard_spec_trace(tmp.path().to_string_lossy().into_owned(), "alpha".to_string());
        assert_eq!(trace["kind"], "spec");
        assert_eq!(trace["label"], "alpha");
        let children = trace["children"].as_array().expect("children array");
        assert_eq!(children.len(), 2);
        assert!(children.iter().any(|c| c["label"].as_str().unwrap_or("").contains("Read")));
        assert!(children.iter().any(|c| c["label"].as_str().unwrap_or("").contains("Edit")));
    }

    fn write_session_event(dir: &Path, session: &str, name: &str, body: &str) {
        let events_dir = dir
            .join(".claude")
            .join(".session")
            .join(session)
            .join(".events");
        std::fs::create_dir_all(&events_dir).unwrap();
        std::fs::write(events_dir.join(name), body).unwrap();
    }

    #[test]
    fn sessions_aggregate_per_dir_with_unknown_bucket_labelled() {
        let tmp = TempDir::new().unwrap();
        // A real session: session.start (carries cwd) + a later tool.use.
        let sess_lines = concat!(
            r#"{"event":"session.start","kind":"session","ts":"2026-05-27T08:00:00.000Z","session_id":"sess-1","spec":null,"payload":{"cwd":"C:\\repo","source":"startup"}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T08:05:00.000Z","session_id":"sess-1","spec":"alpha","payload":{"tool":"Read"}}"#, "\n",
        );
        write_session_event(tmp.path(), "sess-1", "events.ndjson", sess_lines);
        // The unknown attribution-leak bucket.
        let unknown_lines = concat!(
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T07:00:00.000Z","session_id":null,"spec":null,"payload":{"tool":"Bash"}}"#, "\n",
        );
        write_session_event(tmp.path(), "unknown", "events.ndjson", unknown_lines);

        let rows = dashboard_sessions(tmp.path().to_string_lossy().into_owned(), None);
        assert_eq!(rows.len(), 2, "two session dirs aggregated");

        let s1 = rows.iter().find(|r| r.id == "sess-1").expect("sess-1 row");
        assert_eq!(s1.started_at, "2026-05-27T08:00:00.000Z");
        assert_eq!(s1.last_activity_at.as_deref(), Some("2026-05-27T08:05:00.000Z"));
        assert_eq!(s1.last_spec.as_deref(), Some("alpha"));
        assert_eq!(s1.cwd.as_deref(), Some("C:\\repo"));
        assert_eq!(s1.event_count, 2);
        assert_eq!(s1.status, "closed"); // 2026 timestamps are far in the past
        assert!(!s1.is_unknown_bucket);

        let unk = rows.iter().find(|r| r.id == "unknown").expect("unknown row");
        assert!(unk.is_unknown_bucket, "unknown bucket must be labelled, not hidden");
        assert_eq!(unk.event_count, 1);

        // `limit` caps after sorting.
        let one = dashboard_sessions(tmp.path().to_string_lossy().into_owned(), Some(1));
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn sessions_empty_when_no_session_dir() {
        let tmp = TempDir::new().unwrap();
        let rows = dashboard_sessions(tmp.path().to_string_lossy().into_owned(), None);
        assert!(rows.is_empty());
    }
}
