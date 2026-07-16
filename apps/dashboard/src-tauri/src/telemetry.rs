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
//! | `rtk_summary` | subprocess `rtk gain -f json --daily` |
//! | `hook_fire_counts` | filesystem `.claude/.metrics/*.jsonl` |
//! | `routing_breakdown` | filesystem `.claude/.metrics/model-routing-gate.jsonl` |
//! | `agent_activity` | NDJSON `event=="agent.start"`/`"agent.stop"` |
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
use std::collections::{BTreeMap, BTreeSet, HashMap};
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

/// Per-event friction entry. Kept for the dashboard "Atrito" widget.
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct FrictionEntry {
    pub kind: String,
    pub count: u64,
    pub last_ts: Option<String>,
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
    /// Number of `tool.use` events in the session — the "what was DONE" count.
    pub tools_used: u32,
    /// Number of DISTINCT files touched across all `tool.use` events (extracted
    /// from `payload.target.{file_path,file}`). The "what was ADJUSTED" count.
    pub files_touched: u32,
    /// The distinct file paths touched, sorted and capped at
    /// [`SESSION_FILES_CAP`] so a long-running session can't inflate the row.
    pub files: Vec<String>,
    /// Per-tool counts (`Read`, `Grep`, `Edit`, …), sorted by `count` desc —
    /// the "what was DONE" breakdown. Tool name read from `payload.tool`.
    pub tool_breakdown: Vec<SessionToolCount>,
    /// Work GROUP for the session — the suffix of the earliest `skill.invoked`
    /// whose `payload.skill` starts with `"mustard:"` (e.g. `mustard:feature` →
    /// `"feature"`). Falls back to `Some("outros")` when the only skills are
    /// non-mustard, and `None` when no `skill.invoked` was seen at all (the
    /// frontend treats `None` as "avulsa" — a session with no command).
    pub category: Option<String>,
    /// The REQUEST text — `payload.args` of the earliest mustard `skill.invoked`
    /// (the same one that set `category`); falls back to any `skill.invoked`'s
    /// `args`, then the earliest `user.prompt`'s `payload.prompt`. Normalised to
    /// a single line and truncated to ~160 chars. `None` when nothing matched.
    pub title: Option<String>,
    /// Work KIND the router classified this session's request as — the
    /// `payload.kind` of the earliest `pipeline.kind` event in the session
    /// (`"feature"` / `"bugfix"` / `"task"` / `"tactical-fix"`). This is the
    /// honest "what type of work" signal even for the lean `task`/`bugfix`
    /// fast-paths that never become a spec. `None` when no `pipeline.kind` event
    /// was seen (older sessions, or work the router never tagged).
    pub kind: Option<String>,
    /// Detected scope/ceremony paired with [`kind`](Self::kind) — `payload.scope`
    /// of the same earliest `pipeline.kind` event (`"light"` / `"full"` /
    /// `"lean"`). `None` when absent or when no `pipeline.kind` event was seen.
    pub scope: Option<String>,
}

/// One `tool → count` entry in a session's `tool_breakdown`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionToolCount {
    pub name: String,
    pub count: u32,
}

/// A session counts as `open` when its last activity is no older than this.
const SESSION_OPEN_WINDOW_MS: i64 = 15 * 60 * 1000;

/// Cap on the `files` list surfaced per session row — a long session can touch
/// hundreds of files; the row only needs a representative, sorted sample.
const SESSION_FILES_CAP: usize = 20;

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

// ── Session → spec read-time attribution (time-ordered binding) ──────────────

/// Time-ordered session→spec binding table, built once from the workspace event
/// log so spec-less work events (`tool.use` / `agent.*` written under
/// `.claude/.session/{id}/.events/` with `spec == null`) can be attributed to
/// the spec their session was bound to *at the time the work happened*.
///
/// The binding source is the set of pipeline lifecycle events
/// (`pipeline.scope` / `pipeline.stage` / `pipeline.status`) — each carries
/// BOTH `session_id` and `spec`, so they pin "session S was working spec X from
/// time T". A session can move between specs over its lifetime, so per session
/// we keep the full list of `(ts_ms, spec)` bindings sorted ascending and
/// resolve any spec-less event to the most-recent binding with `ts <= event.ts`.
///
/// Events whose `ts` precedes the session's first binding — or whose session was
/// never bound to any spec — stay unattributed (`None`); we never blanket-assign
/// a whole session to one spec.
#[derive(Debug, Default, Clone)]
pub struct SessionSpecTimeline {
    /// `session_id` → ascending `(ts_ms, spec)` bindings.
    by_session: HashMap<String, Vec<(i64, String)>>,
}

impl SessionSpecTimeline {
    /// The spec bound to `session_id` at `ts_ms` — the most-recent binding whose
    /// timestamp is `<= ts_ms`. `None` when the session has no binding at or
    /// before that instant (or is unknown).
    #[must_use]
    pub fn spec_at(&self, session_id: &str, ts_ms: i64) -> Option<&str> {
        let bindings = self.by_session.get(session_id)?;
        // `bindings` is sorted ascending by ts_ms (see `build_*`); take the last
        // one that started at or before `ts_ms`.
        bindings
            .iter()
            .rev()
            .find(|(b_ts, _)| *b_ts <= ts_ms)
            .map(|(_, spec)| spec.as_str())
    }

    /// Effective spec for a raw NDJSON `record`: its own non-empty `spec` when
    /// present (never overridden), else the time-ordered session binding for
    /// `(record.session_id, record.ts)`. `None` when neither resolves.
    #[must_use]
    pub fn attributed_spec<'r>(&'r self, record: &'r Value) -> Option<&'r str> {
        // 1. Honour an explicit, non-empty spec on the record.
        if let Some(spec) = record.get("spec").and_then(Value::as_str) {
            if !spec.is_empty() {
                return Some(spec);
            }
        }
        // 2. Fall back to the time-ordered session binding.
        let session = record.get("session_id").and_then(Value::as_str)?;
        if session.is_empty() {
            return None;
        }
        let ts_ms = record
            .get("ts_ms")
            .and_then(Value::as_i64)
            .or_else(|| record.get("ts").and_then(Value::as_str).and_then(iso_to_ms))?;
        self.spec_at(session, ts_ms)
    }
}

/// Build the [`SessionSpecTimeline`] for `repo_path` from the complete workspace
/// event log. Fail-open: a missing log / unreadable shards yield an empty table,
/// so attribution simply degrades to "honour the record's own spec".
#[must_use]
pub fn build_session_spec_timeline(repo_path: &Path) -> SessionSpecTimeline {
    build_session_spec_timeline_from(&walk_ndjson_events_cached(repo_path))
}

/// Binding-source event names — the lifecycle events that carry both
/// `session_id` and `spec`.
const BINDING_EVENTS: &[&str] = &["pipeline.scope", "pipeline.stage", "pipeline.status"];

/// Fold a pre-collected event slice into the session→spec timeline. Split out so
/// callers that already hold the workspace event vec (e.g. `dashboard_spec_trace`)
/// can reuse it without re-walking the filesystem.
#[must_use]
pub fn build_session_spec_timeline_from(records: &[Value]) -> SessionSpecTimeline {
    let mut by_session: HashMap<String, Vec<(i64, String)>> = HashMap::new();
    for record in records {
        if !BINDING_EVENTS.contains(&event_name(record)) {
            continue;
        }
        let Some(session) = record.get("session_id").and_then(Value::as_str) else {
            continue;
        };
        if session.is_empty() {
            continue;
        }
        let Some(spec) = record.get("spec").and_then(Value::as_str) else {
            continue;
        };
        if spec.is_empty() {
            continue;
        }
        let Some(ts_ms) = record
            .get("ts_ms")
            .and_then(Value::as_i64)
            .or_else(|| record.get("ts").and_then(Value::as_str).and_then(iso_to_ms))
        else {
            continue;
        };
        by_session
            .entry(session.to_string())
            .or_default()
            .push((ts_ms, spec.to_string()));
    }
    // Sort each session's bindings ascending by ts so `spec_at` can binary-walk
    // from the end for the last binding at-or-before a query timestamp.
    for bindings in by_session.values_mut() {
        bindings.sort_by_key(|(ts, _)| *ts);
    }
    SessionSpecTimeline { by_session }
}

// ── Attributed per-spec activity counts (card gap closure) ───────────────────

/// Per-spec activity counts folded over the **attributed** event stream — i.e.
/// each event is bucketed under [`SessionSpecTimeline::attributed_spec`], which
/// folds spec-less session work (`tool.use` / `agent.*` written under
/// `.claude/.session/{id}/.events/` with `spec == null`) onto the spec its
/// session was bound to at the event's timestamp.
///
/// The `mustard_core` `project_*` folds key strictly on `event.spec`, so they
/// miss those session events and a live spec's card reads "sem eventos" with
/// `tools 0 / arquivos 0`. These counts let the dashboard layer merge the
/// attributed totals back into the card without touching core.
#[derive(Debug, Default, Clone)]
pub(crate) struct AttributedSpecCounts {
    /// Total events attributed to the spec (explicit-spec + session-attributed).
    pub events: u32,
    /// `event == "tool.use"` count attributed to the spec.
    pub tools_used: u32,
    /// Distinct file paths touched by attributed `tool.use` events (from
    /// `payload.target.{file_path,file}`).
    pub files_touched: u32,
    /// Latest attributed event timestamp (ISO-8601, lexicographically max).
    pub last_event_at: Option<String>,
}

/// Fold the complete workspace event log into per-spec [`AttributedSpecCounts`],
/// keyed by spec name. Walks the events once (spec `.events/` + wave + session
/// sink via [`walk_ndjson_events`]) and builds the [`SessionSpecTimeline`] once,
/// so a caller that lists many specs (e.g. `dashboard_active_pipelines`) pays a
/// single pass instead of one per row.
///
/// Honours an explicit `event.spec` (never re-attributed), so an event already
/// counted by the core fold under its own spec lands in the same bucket here —
/// the caller merges by taking the larger of (core, attributed) so explicit
/// events are never double-counted.
///
/// Fail-open: a missing / unreadable log yields an empty map (callers fall back
/// to the core projection values untouched).
#[must_use]
pub(crate) fn attributed_spec_counts(
    repo_path: &Path,
) -> HashMap<String, AttributedSpecCounts> {
    #[cfg(test)]
    if let Ok(mut calls) = ATTRIBUTED_COUNTS_CALLS.lock() {
        *calls
            .entry(repo_path.to_string_lossy().into_owned())
            .or_insert(0) += 1;
    }
    let events = walk_ndjson_events_cached(repo_path);
    attributed_spec_counts_from(&events)
}

/// Test-visible per-repo invocation counter for [`attributed_spec_counts`],
/// mirroring the [`events_cache_parsed_files`] pattern. Keyed by repo path so
/// parallel tests on distinct `TempDir`s never observe each other's calls.
/// Backs the batch-command contract ("N cards cost exactly ONE workspace
/// fold") asserted in `lib.rs`.
#[cfg(test)]
static ATTRIBUTED_COUNTS_CALLS: std::sync::LazyLock<
    std::sync::Mutex<HashMap<String, u64>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

/// How many times [`attributed_spec_counts`] has run for `repo` so far.
#[cfg(test)]
pub(crate) fn attributed_spec_counts_calls(repo: &Path) -> u64 {
    ATTRIBUTED_COUNTS_CALLS
        .lock()
        .ok()
        .and_then(|calls| calls.get(repo.to_string_lossy().as_ref()).copied())
        .unwrap_or(0)
}

/// [`attributed_spec_counts`] over a pre-collected event slice — split out so a
/// caller already holding the workspace vec (and tests) can reuse it without a
/// second filesystem walk.
#[must_use]
pub(crate) fn attributed_spec_counts_from(
    records: &[Value],
) -> HashMap<String, AttributedSpecCounts> {
    let timeline = build_session_spec_timeline_from(records);
    // Per spec: counts plus the distinct-file set (collapsed to a count at the end).
    let mut by_spec: HashMap<String, (AttributedSpecCounts, std::collections::BTreeSet<String>)> =
        HashMap::new();
    for record in records {
        let Some(spec) = timeline.attributed_spec(record) else {
            continue;
        };
        let spec = spec.to_string();
        let entry = by_spec.entry(spec).or_default();
        let counts = &mut entry.0;
        counts.events = counts.events.saturating_add(1);
        // Latest timestamp wins (ISO-8601 sorts lexicographically).
        if let Some(ts) = record.get("ts").and_then(Value::as_str) {
            if counts
                .last_event_at
                .as_deref()
                .is_none_or(|cur| ts > cur)
            {
                counts.last_event_at = Some(ts.to_string());
            }
        }
        if event_name(record) == "tool.use" {
            counts.tools_used = counts.tools_used.saturating_add(1);
            if let Some(file) = record
                .get("payload")
                .and_then(|p| p.get("target"))
                .and_then(Value::as_object)
                .and_then(|o| o.get("file_path").or_else(|| o.get("file")))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                entry.1.insert(file.to_string());
            }
        }
    }
    by_spec
        .into_iter()
        .map(|(spec, (mut counts, files))| {
            counts.files_touched = u32::try_from(files.len()).unwrap_or(u32::MAX);
            (spec, counts)
        })
        .collect()
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

/// Collapse whitespace/newlines into a single line and truncate to ~160 chars
/// (cutting on the first line break first, so a multiline request keeps only
/// its opening line). Returns `None` for empty/whitespace-only input. Used to
/// turn a raw `skill.invoked` `args` / `user.prompt` `prompt` into a one-line
/// session title.
fn one_line_title(raw: &str) -> Option<String> {
    // Cut at the first line break — a multiline request keeps only line one.
    let first_line = raw.split(['\n', '\r']).next().unwrap_or(raw);
    // Collapse any remaining internal runs of whitespace into single spaces.
    let collapsed = first_line.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    const MAX: usize = 160;
    if collapsed.chars().count() <= MAX {
        return Some(collapsed);
    }
    let truncated: String = collapsed.chars().take(MAX).collect();
    Some(format!("{}…", truncated.trim_end()))
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

// ── Transcript motivations (assistant narration → tool) ──────────────────────
//
// The assistant text that *motivated* each tool call does not live in the hook
// NDJSON — it is only in the Claude Code session transcript JSONL at
// `<home>/.claude/projects/<encoded-cwd>/<session_id>.jsonl`. We read that file
// retroactively (old sessions already have a transcript), pair the narration
// preceding each `tool_use` block with that block's `id`, and the trace builder
// splices the text onto the matching `tool.use` node by `tool_use_id`
// (transcript `tool_use.id` === event `payload.tool_use_id`, confirmed e.g.
// `toolu_01MyHwwTRprDPzzZFwWsWfc4`). Every step is fail-open: a missing home,
// absent transcript, or malformed line degrades to an empty map (today's
// behaviour — no motivation rendered).

/// Encode a session `cwd` into the directory name Claude Code uses under
/// `~/.claude/projects/`: every `:` `\` `/` `.` becomes `-`. Hyphens already in
/// the path are preserved (only those four characters are rewritten). E.g.
/// `C:\Atiz\sialia` → `C--Atiz-sialia`;
/// `C:\Atiz\mustard\.claude\worktrees\x` → `C--Atiz-mustard--claude-worktrees-x`.
fn encode_cwd_for_projects(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if matches!(c, ':' | '\\' | '/' | '.') { '-' } else { c })
        .collect()
}

/// Filesystem path of the transcript JSONL for a session, given its absolute
/// working directory and id:
/// `<home>/.claude/projects/<encode(cwd)>/<session_id>.jsonl`. Returns `None`
/// when the user's home directory can't be resolved.
#[must_use]
pub fn transcript_path_for(cwd: &str, session_id: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))?;
    Some(
        home.join(".claude")
            .join("projects")
            .join(encode_cwd_for_projects(cwd))
            .join(format!("{session_id}.jsonl")),
    )
}

/// Parse a Claude Code session transcript JSONL into a `tool_use_id → narration`
/// map: the assistant `text` that immediately preceded each `tool_use` block.
///
/// Walks the file **in order** (an assistant turn can be split across several
/// records, one block per line), tracking the current narration in `last_text`:
/// - a non-empty `"text"` block becomes / appends to `last_text` (consecutive
///   text blocks join with `\n`);
/// - a `"tool_use"` block, when `last_text` is non-empty, maps its `id` →
///   `last_text` (the motivation for that tool);
/// - a real user record (`type == "user"`) clears `last_text` so motivation
///   never leaks across the turn boundary;
/// - `"thinking"` blocks are ignored (private, potentially huge/sensitive).
///
/// Fail-open: an unreadable file, or any malformed line, is skipped; an empty
/// map is a valid result. Takes a `&Path` (not a session id) so it is testable
/// against a fixture without resolving a home directory.
#[must_use]
pub fn transcript_motivations(path: &Path) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return map;
    };
    let mut last_text = String::new();
    // `true` once `last_text` has been consumed by a `tool_use`. The next `text`
    // block then begins a FRESH narration (replaces rather than appends), so two
    // tools separated by new narration don't share concatenated text — yet two
    // CONSECUTIVE tools with no text between them still share the same narration
    // (the real transcript pattern: one rationale, several tool calls).
    let mut consumed = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        // A real user message starts a fresh turn — drop any pending narration so
        // a tool in the next assistant turn can't inherit a stale motivation.
        if record.get("type").and_then(Value::as_str) == Some("user") {
            last_text.clear();
            consumed = false;
        }
        let Some(content) = record
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for block in content {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(txt) = block.get("text").and_then(Value::as_str) {
                        if !txt.trim().is_empty() {
                            // A text block after a tool consumed the narration
                            // opens a new rationale — replace, don't append.
                            if consumed {
                                last_text.clear();
                                consumed = false;
                            }
                            if last_text.is_empty() {
                                last_text.push_str(txt);
                            } else {
                                last_text.push('\n');
                                last_text.push_str(txt);
                            }
                        }
                    }
                }
                Some("tool_use") => {
                    if last_text.is_empty() {
                        continue;
                    }
                    if let Some(id) = block.get("id").and_then(Value::as_str) {
                        if !id.is_empty() {
                            map.insert(id.to_string(), last_text.clone());
                        }
                    }
                    // Keep `last_text` for an immediately-following tool (shared
                    // rationale) but mark it consumed so new text starts fresh.
                    consumed = true;
                }
                // `thinking` (private reasoning) and any other block kind are
                // ignored — only `text` narration motivates a tool.
                _ => {}
            }
        }
    }
    map
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
    // Session→spec attribution. A session's OWN events (`.session/{id}/.events/`)
    // are typically spec-less — the spec binding lives in the per-spec pipeline
    // stream (`pipeline.scope/stage/status`, which carry BOTH session_id and
    // spec). Without this, `last_spec` stays null even for feature runs, and the
    // dashboard can't link a work item to its spec (no waves/PRD/QA drill-in).
    // Build the timeline once (cached walk) and fall back to it per row below.
    let timeline = build_session_spec_timeline(&PathBuf::from(&repo_path));
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
        // The fold: per-session enrichment replicating the `tool.use` extraction
        // from `attributed_spec_counts_from` (kept inline rather than calling it
        // — that helper is keyed by spec and builds a timeline we don't need).
        let mut tools_used: u32 = 0;
        let mut files: BTreeSet<String> = BTreeSet::new();
        let mut tool_counts: BTreeMap<String, u32> = BTreeMap::new();
        // category/title derivation — track, by ascending `ts`, the earliest
        // mustard `skill.invoked` (skill + args), the earliest non-mustard one
        // (to fall back the GROUP to "outros"), the earliest skill args of any
        // flavour (title fallback), and the earliest `user.prompt` text (last
        // resort). All "earliest" = smallest `ts` seen so far.
        let mut earliest_mustard_skill: Option<(String, String, String)> = None; // (ts, suffix, args)
        let mut earliest_any_skill_ts: Option<String> = None;
        let mut earliest_any_skill_args: Option<(String, String)> = None; // (ts, args)
        let mut earliest_prompt: Option<(String, String)> = None; // (ts, prompt)
        // Work-type signal (porta-unica): the EARLIEST `pipeline.kind` event in
        // the session decides the session's kind/scope — it is emitted right
        // when the router dispatches the flow, so the first one is the original
        // classification of this session's request.
        let mut earliest_kind: Option<(String, String, Option<String>)> = None; // (ts, kind, scope)

        for record in &records {
            event_count += 1;
            if event_name(record) == "skill.invoked" {
                let ts = record.get("ts").and_then(Value::as_str).unwrap_or("");
                let skill = record
                    .get("payload")
                    .and_then(|p| p.get("skill"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let args = record
                    .get("payload")
                    .and_then(|p| p.get("args"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if !ts.is_empty() && !skill.is_empty() {
                    // Any-skill bookkeeping (group fallback "outros" + title fallback).
                    if earliest_any_skill_ts.as_deref().map_or(true, |p| ts < p) {
                        earliest_any_skill_ts = Some(ts.to_string());
                    }
                    if !args.is_empty()
                        && earliest_any_skill_args
                            .as_ref()
                            .map_or(true, |(p, _)| ts < p.as_str())
                    {
                        earliest_any_skill_args = Some((ts.to_string(), args.clone()));
                    }
                    // The mustard skill wins category + the primary title source.
                    if let Some(suffix) = skill.strip_prefix("mustard:") {
                        if !suffix.is_empty()
                            && earliest_mustard_skill
                                .as_ref()
                                .map_or(true, |(p, _, _)| ts < p.as_str())
                        {
                            earliest_mustard_skill =
                                Some((ts.to_string(), suffix.to_string(), args));
                        }
                    }
                }
            }
            if event_name(record) == "user.prompt" {
                let ts = record.get("ts").and_then(Value::as_str).unwrap_or("");
                let prompt = record
                    .get("payload")
                    .and_then(|p| p.get("prompt"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if !ts.is_empty()
                    && !prompt.is_empty()
                    && earliest_prompt.as_ref().map_or(true, |(p, _)| ts < p.as_str())
                {
                    earliest_prompt = Some((ts.to_string(), prompt.to_string()));
                }
            }
            if event_name(record) == "pipeline.kind" {
                let ts = record.get("ts").and_then(Value::as_str).unwrap_or("");
                let kind = record
                    .get("payload")
                    .and_then(|p| p.get("kind"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if !ts.is_empty()
                    && !kind.is_empty()
                    && earliest_kind.as_ref().map_or(true, |(p, _, _)| ts < p.as_str())
                {
                    let scope = record
                        .get("payload")
                        .and_then(|p| p.get("scope"))
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                    earliest_kind = Some((ts.to_string(), kind.to_string(), scope));
                }
            }
            if event_name(record) == "tool.use" {
                tools_used = tools_used.saturating_add(1);
                // Tool name lives at `payload.tool` (e.g. "Read"); count it.
                if let Some(tool) = record
                    .get("payload")
                    .and_then(|p| p.get("tool"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    *tool_counts.entry(tool.to_string()).or_insert(0) += 1;
                }
                // Touched file lives at `payload.target.{file_path,file}`.
                if let Some(file) = record
                    .get("payload")
                    .and_then(|p| p.get("target"))
                    .and_then(Value::as_object)
                    .and_then(|o| o.get("file_path").or_else(|| o.get("file")))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    files.insert(file.to_string());
                }
            }
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

        let files_touched = u32::try_from(files.len()).unwrap_or(u32::MAX);
        let files_list: Vec<String> = files.into_iter().take(SESSION_FILES_CAP).collect();
        // Sort the breakdown by count desc, ties broken by name for stability.
        let mut tool_breakdown: Vec<SessionToolCount> = tool_counts
            .into_iter()
            .map(|(name, count)| SessionToolCount { name, count })
            .collect();
        tool_breakdown.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));

        // category: the earliest mustard skill's suffix wins; else "outros" when
        // some non-mustard skill ran; else None (no skill at all → "avulsa").
        let category = match &earliest_mustard_skill {
            Some((_, suffix, _)) => Some(suffix.clone()),
            None if earliest_any_skill_ts.is_some() => Some("outros".to_string()),
            None => None,
        };
        // title: the mustard skill's args (same event as category) → any skill's
        // args → the earliest user.prompt. Each is normalised to one ~160-char
        // line; an empty/absent source falls through to the next.
        let title = earliest_mustard_skill
            .as_ref()
            .and_then(|(_, _, args)| one_line_title(args))
            .or_else(|| {
                earliest_any_skill_args
                    .as_ref()
                    .and_then(|(_, args)| one_line_title(args))
            })
            .or_else(|| {
                earliest_prompt
                    .as_ref()
                    .and_then(|(_, prompt)| one_line_title(prompt))
            });

        rows.push(SessionRow {
            id: id.clone(),
            slug: String::new(),
            started_at: earliest.unwrap_or_default(),
            last_activity_at: latest,
            // Own-events spec when present; otherwise the session's most-recent
            // spec binding from the cross-stream timeline (the common case for
            // feature runs whose session events are spec-less).
            last_spec: last_spec
                .map(|(_, spec)| spec)
                .or_else(|| timeline.spec_at(&id, i64::MAX).map(str::to_string)),
            cwd,
            status,
            event_count,
            is_unknown_bucket: id == "unknown",
            tools_used,
            files_touched,
            files: files_list,
            tool_breakdown,
            category,
            title,
            kind: earliest_kind.as_ref().map(|(_, k, _)| k.clone()),
            scope: earliest_kind.and_then(|(_, _, s)| s),
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
    for p in enumerate_ndjson_paths(root) {
        out.extend(parse_ndjson_file(&p));
    }
    out
}

/// Enumerate every `.ndjson` shard path under the three canonical event sinks
/// — directory metadata only, no file contents are read. The incremental cache
/// uses this for its full sweep (re-stat everything, re-parse only changed
/// fingerprints); [`walk_ndjson_events`] uses it for the uncached walk.
fn enumerate_ndjson_paths(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let claude = root.join(".claude");

    // Per-spec channel + wave subdirs.
    if let Ok(specs) = std::fs::read_dir(claude.join("spec")) {
        for spec_entry in specs.flatten() {
            let spec_path = spec_entry.path();
            if !spec_path.is_dir() {
                continue;
            }
            collect_dir_ndjson(&spec_path.join(".events"), &mut out);
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
                    collect_dir_ndjson(&wp.join("events"), &mut out);
                    collect_dir_ndjson(&wp.join(".events"), &mut out);
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
            collect_dir_ndjson(&path.join(".events"), &mut out);
        }
    }

    out
}

/// One parsed NDJSON shard plus the fingerprint (`len` + `modified`) it was
/// parsed at. The fingerprint is the incremental-cache key part beyond the
/// path: a shard is re-read only when its size or mtime moved (NDJSON is
/// append-only, so any write changes the length).
struct FileChunk {
    len: u64,
    modified: Option<std::time::SystemTime>,
    events: Vec<Value>,
}

/// Per-repo incremental parsed-events cache (spec
/// `performance-dashboard-rotas-lentas-cache`, wave 1).
///
/// The previous cache held one flat `Arc<Vec<Value>>` per repo and the watcher
/// dropped the WHOLE entry on any change — every event write re-walked and
/// re-parsed all ~10k workspace shards. This shape keeps the parse per shard:
///
/// * `files` — shard path → [`FileChunk`] (fingerprint + parsed events).
/// * `dirty` — shard paths the watcher marked changed; the next read
///   re-parses ONLY these (milliseconds per event in steady state).
/// * `sweep` — full re-enumeration requested (cold start / generic
///   [`invalidate_events_cache`]): re-stat everything, re-parse only
///   fingerprint mismatches, drop vanished shards.
/// * `snapshot` — the flattened `Arc<Vec<Value>>` slice commands consume.
/// * `harness` — the same snapshot converted to `HarnessEvent` for the
///   `mustard-core` projections, built lazily and dropped together with
///   `snapshot`.
struct RepoEventsCache {
    files: HashMap<PathBuf, FileChunk>,
    dirty: std::collections::HashSet<PathBuf>,
    sweep: bool,
    snapshot: Option<std::sync::Arc<Vec<Value>>>,
    harness: Option<std::sync::Arc<Vec<mustard_core::domain::model::event::HarnessEvent>>>,
    /// How many shard parses this repo has performed — test-visible so the
    /// incremental contract ("touch 1 file → re-read exactly 1 file") is
    /// asserted by counting parses, not by timing.
    parsed_files: u64,
}

impl RepoEventsCache {
    fn new() -> Self {
        Self {
            files: HashMap::new(),
            dirty: std::collections::HashSet::new(),
            sweep: true,
            snapshot: None,
            harness: None,
            parsed_files: 0,
        }
    }

    /// Bring `snapshot` up to date. Warm + clean → no IO at all. Dirty → stat
    /// and re-parse only the marked shards. Sweep → re-enumerate the shard set
    /// (metadata-only walk), re-parsing only fingerprint mismatches.
    fn ensure_fresh(&mut self, repo: &Path) {
        if self.snapshot.is_some() && !self.sweep && self.dirty.is_empty() {
            return;
        }
        if self.sweep || self.snapshot.is_none() {
            let live = enumerate_ndjson_paths(repo);
            let live_set: std::collections::HashSet<&PathBuf> = live.iter().collect();
            self.files.retain(|p, _| live_set.contains(p));
            for p in &live {
                self.refresh_file(p);
            }
            self.sweep = false;
            self.dirty.clear();
        } else {
            let dirty: Vec<PathBuf> = self.dirty.drain().collect();
            for p in &dirty {
                self.refresh_file(p);
            }
        }
        // Deterministic flatten: shard paths ascending. Shard names start with
        // a nanosecond timestamp, so this is roughly chronological per dir;
        // every projection that needs strict order sorts by `ts` itself.
        let mut keys: Vec<PathBuf> = self.files.keys().cloned().collect();
        keys.sort();
        let total = self.files.values().map(|c| c.events.len()).sum();
        let mut flat: Vec<Value> = Vec::with_capacity(total);
        for k in &keys {
            if let Some(chunk) = self.files.get(k) {
                flat.extend(chunk.events.iter().cloned());
            }
        }
        self.snapshot = Some(std::sync::Arc::new(flat));
        self.harness = None;
    }

    /// Re-stat one shard; re-parse only when the fingerprint moved. A vanished
    /// shard drops its chunk (covers deleted specs / rotated session dirs).
    fn refresh_file(&mut self, path: &Path) {
        match std::fs::metadata(path) {
            Ok(md) => {
                let len = md.len();
                let modified = md.modified().ok();
                if let Some(chunk) = self.files.get(path) {
                    if modified.is_some() && chunk.modified == modified && chunk.len == len {
                        return; // fingerprint unchanged — keep the parsed shard
                    }
                }
                let events = parse_ndjson_file(path);
                self.parsed_files = self.parsed_files.saturating_add(1);
                self.files
                    .insert(path.to_path_buf(), FileChunk { len, modified, events });
            }
            Err(_) => {
                self.files.remove(path);
            }
        }
    }
}

/// Process-global, per-project parsed-events cache.
///
/// Keyed by the repo path (the same `String` the commands receive), the value
/// is the per-repo [`RepoEventsCache`] behind its own `Mutex`. The dashboard's
/// live refresh fans every page query out within a single
/// `dashboard:fs-change` burst, so without this cache every burst re-walked
/// and re-parsed the WHOLE workspace NDJSON once *per command*, synchronously.
/// The watcher marks ONLY the changed shard dirty the instant a relevant file
/// changes (see `watcher.rs`), so the first command after a write re-parses
/// just that shard and the rest of the burst hits the warm slice.
///
/// Lock discipline: the outer `Mutex` is held only for the O(1) map probe /
/// insert — never across a parse. The per-repo `Mutex` IS held across the
/// (incremental) rebuild, deliberately: same-repo callers serialise on it so a
/// burst never duplicates the rebuild, while parallel projects proceed on
/// their own entries.
static EVENTS_CACHE: std::sync::LazyLock<
    std::sync::Mutex<HashMap<String, std::sync::Arc<std::sync::Mutex<RepoEventsCache>>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

/// Fetch (or create) the per-repo cache handle. The global lock is released
/// before the caller locks the entry, so a long rebuild on one repo never
/// blocks lookups for another.
fn repo_cache_handle(repo: &Path) -> std::sync::Arc<std::sync::Mutex<RepoEventsCache>> {
    let key = repo.to_string_lossy().into_owned();
    if let Ok(mut guard) = EVENTS_CACHE.lock() {
        return std::sync::Arc::clone(
            guard
                .entry(key)
                .or_insert_with(|| std::sync::Arc::new(std::sync::Mutex::new(RepoEventsCache::new()))),
        );
    }
    // Poisoned global lock: fail-open with an unshared entry (parses fresh,
    // never caches) — telemetry is not load-bearing.
    std::sync::Arc::new(std::sync::Mutex::new(RepoEventsCache::new()))
}

/// Cached counterpart of [`walk_ndjson_events`]: returns the shared parsed event
/// slice for `repo`, re-reading only the shards whose fingerprint moved since
/// the last call. See [`EVENTS_CACHE`] for the lock discipline. Callers read it
/// as `&[Value]` via deref.
#[must_use]
pub(crate) fn walk_ndjson_events_cached(repo: &Path) -> std::sync::Arc<Vec<Value>> {
    let handle = repo_cache_handle(repo);
    let mut cache = match handle.lock() {
        Ok(c) => c,
        // Poisoned entry: fail-open with a fresh uncached parse.
        Err(_) => return std::sync::Arc::new(walk_ndjson_events(repo)),
    };
    cache.ensure_fresh(repo);
    match &cache.snapshot {
        Some(s) => std::sync::Arc::clone(s),
        None => std::sync::Arc::new(Vec::new()),
    }
}

/// The cached workspace slice converted for the `mustard-core` projections
/// (`project_spec_view` / `project_waves` / `project_quality` /
/// `project_timeline` / `project_workspace`).
///
/// Replaces the per-command `mustard_core::view::projection::read_workspace_events`
/// disk walk (~10k shard opens, five times per spec-detail render) with one
/// conversion over the warm snapshot, cached until the next invalidation. Note
/// the slice is a SUPERSET of the old core walk: it also carries the wave
/// (`wave-N-*/events/` and `wave-N-*/.events/`) and session
/// (`.session/*/.events/`) sinks — the
/// per-spec projections filter by `event.spec`, so extra evidence only
/// improves them.
#[must_use]
pub(crate) fn workspace_harness_events_cached(
    repo: &Path,
) -> std::sync::Arc<Vec<mustard_core::domain::model::event::HarnessEvent>> {
    let handle = repo_cache_handle(repo);
    let mut cache = match handle.lock() {
        Ok(c) => c,
        Err(_) => {
            // Poisoned entry: fail-open with a fresh uncached parse + convert.
            let values = walk_ndjson_events(repo);
            return std::sync::Arc::new(
                mustard_core::view::projection::harness_events_from_values(values.iter()),
            );
        }
    };
    cache.ensure_fresh(repo);
    if cache.harness.is_none() {
        let converted = match &cache.snapshot {
            Some(s) => mustard_core::view::projection::harness_events_from_values(s.iter()),
            None => Vec::new(),
        };
        cache.harness = Some(std::sync::Arc::new(converted));
    }
    match &cache.harness {
        Some(h) => std::sync::Arc::clone(h),
        None => std::sync::Arc::new(Vec::new()),
    }
}

/// Request a full sweep of `repo`'s cache: the next read re-enumerates the
/// shard set and re-stats everything, but still re-parses ONLY the shards
/// whose fingerprint moved. Safety-net invalidation (spec-dir deletions,
/// tests); the steady-state path is [`invalidate_events_cache_path`]. A no-op
/// when the entry is absent or a lock is poisoned (fail-open — a stale cache
/// is corrected on the next change).
pub fn invalidate_events_cache(repo: &str) {
    let entry = match EVENTS_CACHE.lock() {
        Ok(guard) => guard.get(repo).cloned(),
        Err(_) => None,
    };
    if let Some(entry) = entry {
        if let Ok(mut cache) = entry.lock() {
            cache.sweep = true;
        }
    }
}

/// Mark ONE shard dirty so the next read re-parses only that file — the
/// watcher's steady-state invalidation (it knows the exact path that changed,
/// including brand-new and deleted shards). Non-`.ndjson` paths are ignored:
/// they never feed the parsed snapshot. A no-op for repos with no cache entry
/// (the cold-start sweep will pick the file up anyway).
pub fn invalidate_events_cache_path(repo: &str, path: &Path) {
    if path.extension().and_then(|s| s.to_str()) != Some("ndjson") {
        return;
    }
    let entry = match EVENTS_CACHE.lock() {
        Ok(guard) => guard.get(repo).cloned(),
        Err(_) => None,
    };
    if let Some(entry) = entry {
        if let Ok(mut cache) = entry.lock() {
            cache.dirty.insert(path.to_path_buf());
        }
    }
}

/// Test-visible: how many shard parses `repo`'s cache has performed so far.
/// Backs the incremental-contract assertions ("warm second call reads nothing",
/// "touch 1 file → exactly 1 re-read").
#[cfg(test)]
pub(crate) fn events_cache_parsed_files(repo: &Path) -> u64 {
    let entry = match EVENTS_CACHE.lock() {
        Ok(guard) => guard.get(repo.to_string_lossy().as_ref()).cloned(),
        Err(_) => None,
    };
    entry
        .and_then(|e| e.lock().ok().map(|c| c.parsed_files))
        .unwrap_or(0)
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

/// Append every `.ndjson` path directly inside `dir` to `out` (no recursion,
/// no content reads). Fail-open: an unreadable dir contributes nothing.
fn collect_dir_ndjson(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(files) = std::fs::read_dir(dir) else {
        return;
    };
    for file in files.flatten() {
        let p = file.path();
        if p.extension().and_then(|s| s.to_str()) == Some("ndjson") {
            out.push(p);
        }
    }
}

/// Append every parsed record from the `.ndjson` shards directly inside `dir`
/// to `out`. Kept for the narrow per-dir readers (spec trace, session feeds)
/// that intentionally read ONE directory rather than the cached workspace
/// snapshot. Composed from the same enumerate/parse primitives as the cache.
fn collect_one_dir(dir: &Path, out: &mut Vec<Value>) {
    let mut paths = Vec::new();
    collect_dir_ndjson(dir, &mut paths);
    for p in paths {
        out.extend(parse_ndjson_file(&p));
    }
}

/// Parse one NDJSON shard into raw `Value` records. Fail-open: an unreadable
/// file yields an empty vec, malformed lines are skipped.
fn parse_ndjson_file(path: &Path) -> Vec<Value> {
    let mut out = Vec::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return out;
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
    out
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
    let events = walk_ndjson_events_cached(&root);

    // ── cost block ──
    let mut usd_total = 0.0f64;
    let mut by_model: HashMap<String, f64> = HashMap::new();
    let mut by_session: HashMap<String, f64> = HashMap::new();
    let mut last_metric_ts: Option<String> = None;
    let mut sessions_seen: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut active_seconds = 0.0f64;
    for ev in events.iter() {
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
    for ev in events.iter() {
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

// The five `dashboard_economy_*` commands below are async + `spawn_blocking`
// for the same reason as the heavy `lib.rs` commands: the core economy
// readers walk NDJSON on disk per call (and may fan out across every project
// under `EconomyScope::AllProjects`), and a synchronous Tauri command runs on
// the main thread — blocking every queued `invoke` (observed as a frozen
// route switch away from the Economia page). A join error degrades to the
// same empty JSON shape the old sync body returned.

#[tauri::command]
pub async fn dashboard_economy_summary(scope: EconomyScopeDto) -> Value {
    tauri::async_runtime::spawn_blocking(move || {
        let (root, core_scope) = scope.to_core();
        let summary = mustard_core::domain::economy::economy_summary(&root, core_scope)
            .unwrap_or_default();
        serde_json::to_value(summary).unwrap_or_else(|_| serde_json::json!({}))
    })
    .await
    .unwrap_or_else(|_| serde_json::json!({}))
}

#[tauri::command]
pub async fn dashboard_economy_savings_breakdown(scope: EconomyScopeDto) -> Value {
    tauri::async_runtime::spawn_blocking(move || {
        let (root, core_scope) = scope.to_core();
        let breakdown = mustard_core::domain::economy::savings_breakdown(&root, core_scope)
            .unwrap_or_default();
        serde_json::to_value(breakdown).unwrap_or_else(|_| serde_json::json!({}))
    })
    .await
    .unwrap_or_else(|_| serde_json::json!({}))
}

#[tauri::command]
pub async fn dashboard_economy_context_routing(scope: EconomyScopeDto) -> Value {
    tauri::async_runtime::spawn_blocking(move || {
        let (root, core_scope) = scope.to_core();
        let metrics = mustard_core::domain::economy::context_routing_quality(&root, core_scope)
            .unwrap_or_default();
        serde_json::to_value(metrics).unwrap_or_else(|_| serde_json::json!({}))
    })
    .await
    .unwrap_or_else(|_| serde_json::json!({}))
}

#[tauri::command]
pub async fn dashboard_economy_per_spec_costs(scope: EconomyScopeDto) -> Value {
    tauri::async_runtime::spawn_blocking(move || {
        let (root, core_scope) = scope.to_core();
        let rows = mustard_core::domain::economy::per_spec_costs(&root, core_scope)
            .unwrap_or_default();
        serde_json::to_value(rows).unwrap_or_else(|_| serde_json::json!([]))
    })
    .await
    .unwrap_or_else(|_| serde_json::json!([]))
}

#[tauri::command]
pub async fn dashboard_economy_per_wave_costs(scope: EconomyScopeDto) -> Value {
    tauri::async_runtime::spawn_blocking(move || {
        let (root, core_scope) = scope.to_core();
        let rows = mustard_core::domain::economy::per_wave_costs(&root, core_scope)
            .unwrap_or_default();
        serde_json::to_value(rows).unwrap_or_else(|_| serde_json::json!([]))
    })
    .await
    .unwrap_or_else(|_| serde_json::json!([]))
}

/// Pairs `tool.result` NDJSON events back onto their originating `tool.use`
/// trace node so the frontend `<ToolEventRow>` can render the captured output
/// (`payload.result.stdout_excerpt` / `content_excerpt` / file diff).
///
/// Two correlation strategies, in order — mirroring the rt
/// `tool_result_observer` contract ("by `tool_use_id` when forwarded by Claude
/// Code, else by chronological order"):
///
/// 1. **By id.** When a `tool.use` node carries a `tool_use_id` that a
///    `tool.result` echoes, pair them exactly. Robust to interleaving.
/// 2. **Chronological fallback.** Real `tool.use` heartbeats (the
///    `metrics_observer` shape) carry no `tool_use_id`, so we match the next
///    unconsumed `tool.result` whose timestamp is `>=` the use's and whose
///    `tool` name agrees. A `tool.result` immediately follows its `tool.use`,
///    so position order is correct.
///
/// Each result is consumed at most once (`pair_for` removes it), so two
/// identical commands never alias the same captured output.
///
/// All results live in a single timestamp-ordered `chrono` queue. Id-bearing
/// results ALSO register their slot index in `id_index` for an O(1) tier-1
/// hit. Crucially, a result that carries a `tool_use_id` is still reachable by
/// the tier-2 chronological scan — the common real-world case is the *result*
/// carrying an id while the *use* heartbeat (the `metrics_observer` shape) does
/// not, so the use can only ever pair via tier-2.
struct ResultPairing {
    /// `tool_use_id → index into `chrono``. Points at the slot to claim on an
    /// exact id hit; the slot is tombstoned (`None`) once any tier consumes it.
    id_index: HashMap<String, usize>,
    /// Timestamp-ordered result slots. Tombstoned to `None` once paired.
    chrono: Vec<Option<ChronoResult>>,
    /// Cursor into `chrono`: every entry before it is consumed, so the tier-2
    /// scan stays amortised-linear across the whole `tool.use` stream.
    cursor: usize,
}

/// One `tool.result` awaiting pairing.
struct ChronoResult {
    ts_ms: i64,
    tool: String,
    payload: Value,
}

impl ResultPairing {
    /// Build the pairing index from the full event slice in one linear pass,
    /// then sort the result queue by timestamp.
    fn build(all_events: &[Value]) -> Self {
        let mut chrono: Vec<ChronoResult> = Vec::new();
        let mut ids: Vec<Option<String>> = Vec::new();
        for ev in all_events {
            if ev.get("event").and_then(Value::as_str) != Some("tool.result") {
                continue;
            }
            let Some(payload) = ev.get("payload").filter(|p| p.is_object()).cloned() else {
                continue;
            };
            // Prefer record-level `ts_ms` (an int the hooks write), else the ISO `ts`.
            let ts_ms = ev
                .get("ts_ms")
                .and_then(Value::as_i64)
                .or_else(|| ev.get("ts").and_then(Value::as_str).and_then(iso_to_ms))
                .unwrap_or(0);
            let tool = payload
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let id = payload
                .get("tool_use_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            ids.push(id);
            chrono.push(ChronoResult { ts_ms, tool, payload });
        }
        // Sort by ts, carrying the parallel `ids` vec along so `id_index` stays
        // correct against the final slot positions.
        let mut order: Vec<usize> = (0..chrono.len()).collect();
        order.sort_by_key(|&i| chrono[i].ts_ms);
        let mut sorted: Vec<Option<ChronoResult>> = Vec::with_capacity(chrono.len());
        let mut id_index: HashMap<String, usize> = HashMap::new();
        // Drain in sorted order. `Option::take` lets us move each owned
        // `ChronoResult` out of the source vec exactly once.
        let mut chrono_opt: Vec<Option<ChronoResult>> = chrono.into_iter().map(Some).collect();
        for (new_idx, &src) in order.iter().enumerate() {
            if let Some(id) = ids[src].clone() {
                // Last write wins on duplicate ids (later result supersedes).
                id_index.insert(id, new_idx);
            }
            sorted.push(chrono_opt[src].take());
        }
        Self { id_index, chrono: sorted, cursor: 0 }
    }

    /// Take the `tool.result` payload paired with the `tool.use` record `ev`
    /// (whose resolved `tool_name` is supplied by the caller), or `None` when no
    /// result was captured for it. The result is removed so it is never reused.
    fn pair_for(&mut self, ev: &Value, tool_name: &str) -> Option<Value> {
        // Tier 1 — exact id match (only when the *use* itself carries an id).
        if let Some(id) = ev
            .get("payload")
            .and_then(|p| p.get("tool_use_id"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            if let Some(&idx) = self.id_index.get(id) {
                if let Some(slot) = self.chrono.get_mut(idx) {
                    if let Some(result) = slot.take() {
                        return Some(result.payload);
                    }
                }
            }
        }
        // Tier 2 — chronological fallback. Claim the earliest unconsumed result
        // at-or-after this use's timestamp whose tool name agrees (an empty name
        // on either side is a wildcard so older / unlabelled events still pair).
        let use_ms = ev
            .get("ts_ms")
            .and_then(Value::as_i64)
            .or_else(|| ev.get("ts").and_then(Value::as_str).and_then(iso_to_ms))
            .unwrap_or(0);
        while self.cursor < self.chrono.len() && self.chrono[self.cursor].is_none() {
            self.cursor += 1;
        }
        for slot in self.chrono.iter_mut().skip(self.cursor) {
            let Some(candidate) = slot.as_ref() else { continue };
            if candidate.ts_ms < use_ms {
                continue;
            }
            let tool_ok = candidate.tool.is_empty()
                || tool_name.is_empty()
                || candidate.tool == tool_name;
            if tool_ok {
                return slot.take().map(|c| c.payload);
            }
        }
        None
    }
}

/// A subagent's active span, derived from a paired `agent.start`/`agent.stop`.
///
/// Tools are attributed to a subagent purely by *time*: a `tool.use` whose
/// timestamp falls in `[start_ms, end_ms)` (same real `session_id`) ran inside
/// this dispatch. `end_ms == u64::MAX` marks a still-running subagent (an
/// `agent.start` with no matching stop yet). Only events sharing a genuine
/// session id correlate — see [`real_session_id`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentInterval {
    session_id: String,
    start_ms: u64,
    end_ms: u64,
    /// Human label (the `agent.start.description`, else the subagent type).
    name: String,
    /// `agent.start.subagentType` (e.g. `Explore` / `general-purpose`).
    subagent_type: String,
    /// `agent.start.payload.agent_id` — the economy's per-agent cost key (e.g.
    /// `Explore` / `general-purpose` / `mustard-review`). Used to look up the
    /// agent's roll-up tokens/cost; the human `name` (the description) is a
    /// display label and never the cost key.
    agent_id: String,
    /// Wave number parsed from the description (`Wave 1 …` / `Onda 2 …`), if any.
    wave: Option<u32>,
    /// `agent.start.payload.tool_use_id` — the id of the *Task spawn* call that
    /// dispatched this subagent. This is the key the transcript's narration is
    /// keyed under (the assistant's `text` block that preceded the `Task` call),
    /// so the trace can splice the spawning motivation onto the agent node. `None`
    /// when the start carried no `tool_use_id` (session-less / legacy events).
    spawn_tool_use_id: Option<String>,
}

/// Resolved attribution for one `tool.use`: the owning agent's display name,
/// the economy cost key (`agent_id`), its subagent type, and the wave it belongs
/// to. The orchestrator (a tool outside every interval) resolves to
/// `agent = "orquestrador"`, no `agent_id`, no type, no wave.
struct ToolAttribution {
    agent: String,
    /// The matched interval's `agent_id` — the key the per-agent token/cost map
    /// is built on. `None` for the orchestrator (which has no economy row).
    agent_id: Option<String>,
    subagent_type: Option<String>,
    wave: Option<u32>,
    /// The matched interval's `spawn_tool_use_id` (the `agent.start`'s Task-spawn
    /// id). Propagated so the tree builder can key the spawning motivation onto
    /// the agent node. `None` for the orchestrator (no interval).
    spawn_tool_use_id: Option<String>,
}

/// The orchestrator label for tools that ran outside every subagent interval.
const ORCHESTRATOR: &str = "orquestrador";

/// The genuine `session_id` of an event, or `None` when the wire carries no
/// real session.
///
/// Hooks stamp `"unknown"` (see `build_harness_event` in apps/rt) when the
/// harness threads no session id, and some session-less records carry an
/// empty/absent field. Such ids are NOT identities — collapsing them into one
/// bucket would interleave the intervals of unrelated runs (real data holds
/// ~1331 session-less `tool.use` events). Only a present, non-empty, non-
/// `"unknown"` id participates in interval correlation.
fn real_session_id(ev: &Value) -> Option<&str> {
    ev.get("session_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && *s != "unknown")
}

/// Read a record's epoch-millis timestamp — record-level `ts_ms` (the int the
/// hooks write), else the ISO `ts` parsed to ms. `None` when neither is present.
fn ts_ms_of(record: &Value) -> Option<u64> {
    record
        .get("ts_ms")
        .and_then(Value::as_u64)
        .or_else(|| {
            record
                .get("ts")
                .and_then(Value::as_str)
                .and_then(iso_to_ms)
                .and_then(|ms| u64::try_from(ms).ok())
        })
}

/// Parse a wave number from a free-text description: case-insensitive
/// `wave 1` / `onda 2`. `None` when no such token is present.
fn parse_wave(description: &str) -> Option<u32> {
    let lower = description.to_ascii_lowercase();
    // Hand-rolled scan (no regex dep): find "wave"/"onda", skip whitespace, read
    // the digit run, and require a word boundary before the keyword so e.g.
    // "software 1" never matches.
    for kw in ["wave", "onda"] {
        let mut from = 0;
        while let Some(rel) = lower[from..].find(kw) {
            let at = from + rel;
            let before_ok = at == 0
                || !lower.as_bytes()[at - 1].is_ascii_alphanumeric();
            let after = at + kw.len();
            if before_ok {
                let rest = &lower[after..];
                let trimmed = rest.trim_start();
                // Only count whitespace as the separator (a `\b...\s+` shape).
                if rest.len() != trimmed.len() || rest.is_empty() {
                    let digits: String =
                        trimmed.chars().take_while(char::is_ascii_digit).collect();
                    if !digits.is_empty() {
                        if let Ok(n) = digits.parse::<u32>() {
                            return Some(n);
                        }
                    }
                }
            }
            from = at + kw.len();
        }
    }
    None
}

/// One wave-plan role: its normalised token sequence (lowercase, split on
/// non-alphanumerics) plus the wave number its directory carried.
type RoleWave = (Vec<String>, u32);

/// Lowercase a free-text string and split it into alphanumeric tokens, dropping
/// empties. `"App: desdobrar-dialog"` → `["app", "desdobrar", "dialog"]`. Used to
/// match a dispatch description against a wave-plan role name without caring about
/// the separator (`-`, `:`, space) the writer happened to use.
fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

/// Read the `role → wave_number` map from a spec's `wave-{N}-{role}` subdirectories.
///
/// A wave plan materialises its waves as directories
/// `<spec>/wave-{N}-{role}/` (e.g. `wave-1-backend-ledger`, `wave-3-core`), so the
/// role→N binding is knowable from disk. Each entry's role is stored as its token
/// sequence (see [`tokenize`]) so the later match is separator-insensitive.
///
/// Fail-soft: an unreadable / absent dir (a non-wave Light spec) yields an empty
/// map, and the per-interval resolution falls back to wave-less exactly as before.
fn read_wave_role_map(spec_dir: &Path) -> Vec<RoleWave> {
    let Ok(entries) = std::fs::read_dir(spec_dir) else {
        return Vec::new();
    };
    let mut map: Vec<RoleWave> = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        // `wave-{N}-{role}` → (N, role). Strip the `wave-` prefix, take the leading
        // digit run as N, then the remainder (after the separating `-`) as the role.
        let Some(rest) = name.strip_prefix("wave-") else {
            continue;
        };
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            continue;
        }
        let Ok(wave) = digits.parse::<u32>() else {
            continue;
        };
        let role = &rest[digits.len()..];
        let role = role.strip_prefix('-').unwrap_or(role);
        let role_tokens = tokenize(role);
        if role_tokens.is_empty() {
            continue;
        }
        map.push((role_tokens, wave));
    }
    map
}

/// `true` when `needle` appears as a contiguous run inside `haystack`.
fn contains_subslice(haystack: &[String], needle: &[String]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Resolve a wave for a dispatch that carried no `wave N` token, by matching its
/// description (and subagent type) against the wave-plan `role → wave` map.
///
/// A description's tokens must CONTAIN a role's token sequence contiguously — so
/// `"Review backend-ledger"` (tokens `[review, backend, ledger]`) matches the
/// `backend-ledger` role (`[backend, ledger]`) → its wave. The subagent type is
/// tried as a secondary haystack (a bare role name dispatched as the type).
///
/// Guards against weak matches: a single-token role of length 1 (e.g. a role
/// literally named `"a"`) never matches — too coarse. When several roles match,
/// the one with the most tokens wins (the most specific role); ties keep the
/// lowest wave for determinism. `None` when nothing matches — the interval stays
/// wave-less, nesting straight under the spec as today.
fn match_role_wave(description: &str, subagent_type: &str, role_map: &[RoleWave]) -> Option<u32> {
    if role_map.is_empty() {
        return None;
    }
    let desc_tokens = tokenize(description);
    let type_tokens = tokenize(subagent_type);
    let mut best: Option<(usize, u32)> = None; // (role token count, wave)
    for (role_tokens, wave) in role_map {
        // Reject a trivially-coarse role: a lone 1-char token would match noise.
        if role_tokens.len() == 1 && role_tokens[0].len() < 2 {
            continue;
        }
        if contains_subslice(&desc_tokens, role_tokens)
            || contains_subslice(&type_tokens, role_tokens)
        {
            let specificity = role_tokens.len();
            let take = match best {
                None => true,
                Some((best_len, best_wave)) => {
                    specificity > best_len || (specificity == best_len && *wave < best_wave)
                }
            };
            if take {
                best = Some((specificity, *wave));
            }
        }
    }
    best.map(|(_, wave)| wave)
}

/// Build the per-session subagent intervals from the event slice.
///
/// Groups `agent.start`/`agent.stop` by their genuine `session_id`, orders by
/// `ts_ms`, and pairs them with a per-session **stack** (LIFO) so nested
/// dispatches close in the right order. Each `agent.start` with a non-empty
/// `subagentType` pushes a frame; each `agent.stop` pops the top and closes its
/// interval. `agent.start` events with an empty/missing `subagentType` are
/// ignored as noise. A frame left on the stack at the end (a subagent still
/// running) closes at `u64::MAX`.
///
/// Events without a real session id (missing / empty / `"unknown"`) carry no
/// identity and are dropped here — a session-less `agent.start`/`agent.stop`
/// neither opens nor closes an interval, so it cannot capture tools from an
/// unrelated run. See [`real_session_id`].
///
/// Each interval's wave is resolved from its dispatch description: a literal
/// `wave N` / `onda N` token wins ([`parse_wave`]); failing that, the description
/// is matched against the wave-plan `role → wave` map (`role_map`, read from the
/// `wave-{N}-{role}` dirs) so a role-named dispatch with no wave number still
/// attributes ([`match_role_wave`]). Pass an empty `role_map` to disable the
/// fallback (a non-wave spec).
fn build_agent_intervals(all_events: &[Value], role_map: &[RoleWave]) -> Vec<AgentInterval> {
    // (ts_ms, is_start, session, name, subagent_type, agent_id, wave) per event.
    struct Marker {
        ts_ms: u64,
        is_start: bool,
        session: String,
        name: String,
        subagent_type: String,
        agent_id: String,
        wave: Option<u32>,
        spawn_tool_use_id: Option<String>,
    }
    let mut markers: Vec<Marker> = Vec::new();
    for ev in all_events {
        let ev_name = ev.get("event").and_then(Value::as_str).unwrap_or("");
        let is_start = ev_name == "agent.start";
        let is_stop = ev_name == "agent.stop";
        if !is_start && !is_stop {
            continue;
        }
        let Some(ts_ms) = ts_ms_of(ev) else { continue };
        // Only genuinely-sessioned events correlate; a session-less marker would
        // collapse unrelated runs into one pseudo-session and mis-bucket tools.
        let Some(session) = real_session_id(ev).map(str::to_string) else {
            continue;
        };
        let payload = ev.get("payload");
        let subagent_type = payload
            .and_then(|p| p.get("subagentType"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        // An `agent.start` is only a real dispatch when it names a subagent type;
        // the alternating empty-type starts are observer noise.
        if is_start && subagent_type.is_empty() {
            continue;
        }
        let description = payload
            .and_then(|p| p.get("description"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let name = if description.is_empty() {
            subagent_type.clone()
        } else {
            description.clone()
        };
        // The economy keys per-agent cost on `agent.start.payload.agent_id`
        // (`tool_input.agent_id` ?? `subagentType`); fall back to the type when
        // the field is absent so the lookup key matches the run event's.
        let agent_id = payload
            .and_then(|p| p.get("agent_id"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or(subagent_type.as_str())
            .to_string();
        // Wave resolution, in priority order: (1) an explicit `wave N` / `onda N`
        // token in the description; (2) failing that, match the description (and
        // subagent type) against the wave-plan `role → wave` map read from the
        // `wave-{N}-{role}` dirs — so a `mustard-review` dispatch named
        // "Review backend-ledger" lands on its wave even with no wave number.
        let wave = parse_wave(&description)
            .or_else(|| match_role_wave(&description, &subagent_type, role_map));
        // The Task-spawn id of THIS dispatch (only meaningful on a start); the
        // transcript narration that motivated the spawn is keyed under it.
        let spawn_tool_use_id = payload
            .and_then(|p| p.get("tool_use_id"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        markers.push(Marker {
            ts_ms,
            is_start,
            session,
            name,
            subagent_type,
            agent_id,
            wave,
            spawn_tool_use_id,
        });
    }
    // Stable sort by ts so a start and stop sharing a ms keep emit order (start
    // before stop is the natural write order; the stack tolerates either).
    markers.sort_by_key(|m| m.ts_ms);

    // Per-session stack of open frames.
    struct Frame {
        start_ms: u64,
        name: String,
        subagent_type: String,
        agent_id: String,
        wave: Option<u32>,
        spawn_tool_use_id: Option<String>,
    }
    let mut stacks: HashMap<String, Vec<Frame>> = HashMap::new();
    let mut intervals: Vec<AgentInterval> = Vec::new();
    for m in markers {
        let stack = stacks.entry(m.session.clone()).or_default();
        if m.is_start {
            stack.push(Frame {
                start_ms: m.ts_ms,
                name: m.name,
                subagent_type: m.subagent_type,
                agent_id: m.agent_id,
                wave: m.wave,
                spawn_tool_use_id: m.spawn_tool_use_id,
            });
        } else if let Some(frame) = stack.pop() {
            intervals.push(AgentInterval {
                session_id: m.session,
                start_ms: frame.start_ms,
                end_ms: m.ts_ms,
                name: frame.name,
                subagent_type: frame.subagent_type,
                agent_id: frame.agent_id,
                wave: frame.wave,
                spawn_tool_use_id: frame.spawn_tool_use_id,
            });
        }
        // A stray `agent.stop` with an empty stack is dropped (no open frame).
    }
    // Close any still-running subagents at +∞ so their in-flight tools attribute.
    for (session, stack) in stacks {
        for frame in stack {
            intervals.push(AgentInterval {
                session_id: session.clone(),
                start_ms: frame.start_ms,
                end_ms: u64::MAX,
                name: frame.name,
                subagent_type: frame.subagent_type,
                agent_id: frame.agent_id,
                wave: frame.wave,
                spawn_tool_use_id: frame.spawn_tool_use_id,
            });
        }
    }
    intervals
}

/// Attribute one `tool.use` record to its owning agent. Picks the **innermost**
/// interval (smallest window) whose half-open `[start_ms, end_ms)` contains the
/// tool's `ts_ms` in the same real session; falls back to the orchestrator when
/// no interval matches (the tool ran between dispatches, or its ts/session is
/// unreadable).
///
/// The end bound is EXCLUSIVE: a tool logged at the exact `agent.stop`
/// millisecond ran after the dispatch closed, so it attributes to the enclosing
/// interval (or the orchestrator), not the just-popped one. The start bound
/// stays inclusive, and `end_ms == u64::MAX` (a still-running subagent) keeps
/// capturing every later tool.
///
/// A tool with no real session id attributes to the ORCHESTRATOR: it shares no
/// identity with any interval, so it must not borrow a session-less agent's
/// window (the null/empty-session collapse FIX 1 guards against).
fn attribute_tool(ev: &Value, intervals: &[AgentInterval]) -> ToolAttribution {
    let owner = real_session_id(ev).zip(ts_ms_of(ev)).and_then(|(session, ts)| {
        intervals
            .iter()
            .filter(|iv| iv.session_id == session && ts >= iv.start_ms && ts < iv.end_ms)
            // Innermost = smallest window (deepest nesting).
            .min_by_key(|iv| iv.end_ms.saturating_sub(iv.start_ms))
    });
    match owner {
        Some(iv) => ToolAttribution {
            agent: iv.name.clone(),
            agent_id: Some(iv.agent_id.clone()),
            subagent_type: Some(iv.subagent_type.clone()),
            wave: iv.wave,
            spawn_tool_use_id: iv.spawn_tool_use_id.clone(),
        },
        None => ToolAttribution {
            agent: ORCHESTRATOR.to_string(),
            agent_id: None,
            subagent_type: None,
            wave: None,
            spawn_tool_use_id: None,
        },
    }
}

/// Look up an agent's economy roll-up value (tokens or cost) by its `agent_id`.
///
/// The per-agent map is keyed on the economy's `agent_id`
/// ([`mustard_core::domain::economy::per_agent_costs`], grouped on
/// `payload.agent_id`). The `agent.start` and the finalising run event carry the
/// SAME `agent_id`, so an exact hit is the normal path. When a writer suffixes
/// the run-event id (e.g. `Explore-1` while the start said `Explore`), fall back
/// to the unique key that has `id` as a `-`-delimited prefix. A prefix that
/// matches more than one economy row is ambiguous and yields `None` — never
/// attach another agent's cost; the node then simply omits the value.
fn lookup_agent_metric(map: &HashMap<String, i64>, id: &str) -> Option<i64> {
    if let Some(v) = map.get(id) {
        return Some(*v);
    }
    let prefix = format!("{id}-");
    let mut hit: Option<i64> = None;
    for (k, v) in map {
        if k.starts_with(&prefix) {
            if hit.is_some() {
                return None; // ambiguous — refuse to guess
            }
            hit = Some(*v);
        }
    }
    hit
}

/// Spec trace — a tree of `spec → [wave] → agent → tool`.
///
/// W7D restored the full tree shape; the agent attribution was later rebuilt on
/// time intervals (the wire carries no per-tool agent identity — every
/// `tool.use` has `actor="metrics-tracker"`, empty `wave`, and no matching
/// `tool_use_id`). [`build_agent_intervals`] pairs `agent.start`/`agent.stop`
/// per session; [`attribute_tool`] assigns each `tool.use` to the innermost
/// interval that brackets its timestamp, else the orchestrator. Wave numbers
/// come from the dispatch description (`Wave 1 …`). Roll-up tokens per agent
/// come from [`mustard_core::domain::economy::per_agent_costs`] (scope-filtered
/// to the spec). Tools without a wave (orchestrator / unparsed) hang straight
/// off the spec rather than under a synthetic wave node.
/// Off-main-thread wrapper for [`dashboard_spec_trace_impl`] (full workspace
/// walk + per-agent cost projection + tree build). A join error degrades to an
/// empty `{}` object — never an Err toast (the trace renderer tolerates an empty
/// tree). The sync `_impl` is kept so unit tests call it directly.
#[tauri::command]
pub async fn dashboard_spec_trace(project_path: String, spec_name: String) -> Value {
    tauri::async_runtime::spawn_blocking(move || dashboard_spec_trace_impl(project_path, spec_name))
        .await
        .unwrap_or_else(|_| serde_json::json!({}))
}

#[must_use]
pub fn dashboard_spec_trace_impl(project_path: String, spec_name: String) -> Value {
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

    // Read-time session→spec attribution: a session's work events (`tool.use`,
    // `agent.*`) are written under `.claude/.session/{id}/.events/` with
    // `spec == null`, so the spec-dir walk above misses them and the trace looks
    // idle while work is in flight. Build the time-ordered binding from the whole
    // workspace log and pull in every session event that attributes to THIS spec
    // (its session was bound here at the event's ts). Fail-open: an empty
    // workspace log leaves `all_events` as the spec-dir-only set (today's behavior).
    let workspace = walk_ndjson_events_cached(&base);
    let timeline = build_session_spec_timeline_from(&workspace);
    let session_root = base.join(".claude").join(".session");
    if let Ok(session_dirs) = std::fs::read_dir(&session_root) {
        for entry in session_dirs.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            let mut session_events: Vec<Value> = Vec::new();
            collect_one_dir(&p.join(".events"), &mut session_events);
            for ev in session_events {
                // Only attribute spec-less events (an explicit non-empty spec is
                // already honoured by the spec-dir walk and must not be double-counted).
                let already_specced = ev
                    .get("spec")
                    .and_then(Value::as_str)
                    .map_or(false, |s| !s.is_empty());
                if already_specced {
                    continue;
                }
                if timeline.attributed_spec(&ev) == Some(spec_name.as_str()) {
                    all_events.push(ev);
                }
            }
        }
    }

    // Pass 1: build subagent intervals from `agent.start`/`agent.stop` events.
    //
    // A subagent's tools are NOT tagged with its identity on the wire — every
    // `tool.use` carries `actor="metrics-tracker"`, an empty `wave`, and no
    // `tool_use_id` that matches the inner tools (the `agent.start.tool_use_id`
    // is the *Task spawn* id, not the inner tools' ids). The only honest signal
    // is *time*: the `tool.use` events that fall between an `agent.start` and
    // its matching `agent.stop` within the same `session_id` belong to that
    // subagent; tools outside every interval belong to the orchestrator. See
    // [`build_agent_intervals`].
    //
    // The `role → wave` map (read from this spec's `wave-{N}-{role}` dirs) lets a
    // dispatch whose description carries a role name but no `wave N` token (e.g. a
    // `Review backend-ledger` review pass) still attribute to its wave. Empty for a
    // non-wave spec, in which case the resolution falls back to `parse_wave` only.
    let role_map = read_wave_role_map(&spec_dir);

    // The spec trace keeps today's behaviour exactly — no transcript narration
    // (an empty map splices nothing), so the `spec_trace_*` tests stay green.
    build_trace_tree(
        &all_events,
        &role_map,
        &agent_tokens,
        &agent_cost_micros,
        "spec",
        &spec_name,
        &HashMap::new(),
    )
}

/// Off-main-thread wrapper for [`dashboard_session_trace_impl`]. Mirrors
/// [`dashboard_spec_trace`]: a join error degrades to an empty `{}` object —
/// never an Err toast (the trace renderer tolerates an empty tree).
#[tauri::command]
pub async fn dashboard_session_trace(project_path: String, session_id: String) -> Value {
    tauri::async_runtime::spawn_blocking(move || {
        dashboard_session_trace_impl(project_path, session_id)
    })
    .await
    .unwrap_or_else(|_| serde_json::json!({}))
}

/// The hierarchical trace for ONE session, built with the SAME
/// [`build_trace_tree`] machinery the spec trace uses (SOLID — one tree
/// builder, no parallel view).
///
/// A session's work events live under `.claude/.session/{id}/.events/` — there
/// are no wave subdirs and no `wave-{N}-{role}` directories, so the role→wave
/// map is empty and tool nodes hang off the orchestrator (or off whatever
/// `agent.start`/`agent.stop` intervals the session recorded). Token/cost
/// roll-up is absent: [`mustard_core::domain::economy::EconomyScope`] has no
/// `Session` variant, so we pass empty maps rather than invent a scope (the
/// agent nodes simply carry no token pill — fail-open, acceptable).
///
/// Fail-open: a missing/unreadable session directory yields the empty-children
/// session root, never an error.
#[must_use]
pub fn dashboard_session_trace_impl(project_path: String, session_id: String) -> Value {
    let base = PathBuf::from(&project_path);

    // Sessions have no waves nor subdirs — a single `.events/` channel.
    let mut all_events: Vec<Value> = Vec::new();
    collect_one_dir(
        &base
            .join(".claude")
            .join(".session")
            .join(&session_id)
            .join(".events"),
        &mut all_events,
    );

    // No `wave-{N}-{role}` dirs for a session → empty role map (same type as
    // `read_wave_role_map`'s return, so `build_agent_intervals` falls back to
    // `parse_wave` only). No economy scope for a session → empty token maps.
    let role_map: Vec<RoleWave> = Vec::new();
    let agent_tokens: HashMap<String, i64> = HashMap::new();
    let agent_cost_micros: HashMap<String, i64> = HashMap::new();

    // Assistant narration that motivated each tool lives only in the session
    // transcript JSONL, keyed under `<home>/.claude/projects/<encode(cwd)>/`. The
    // session's `cwd` is on its `session.start` payload. Fail-open at every step:
    // no cwd / no home / absent transcript → empty map → no motivation spliced
    // (today's behaviour). Resolved here, not threaded from the frontend, so the
    // trace command signature is unchanged.
    let cwd = all_events.iter().find_map(|ev| {
        if event_name(ev) != "session.start" {
            return None;
        }
        ev.get("payload")
            .and_then(|p| p.get("cwd"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    });
    let motivations = cwd
        .as_deref()
        .and_then(|cwd| transcript_path_for(cwd, &session_id))
        .map(|path| transcript_motivations(&path))
        .unwrap_or_default();

    build_trace_tree(
        &all_events,
        &role_map,
        &agent_tokens,
        &agent_cost_micros,
        "session",
        &session_id,
        &motivations,
    )
}

/// Shared tree builder for `{spec,session} → [wave] → agent → tool`.
///
/// Owns passes 1 (`build_agent_intervals`), 1.5 (`ResultPairing::build`) and 2
/// (the `by_wave` attribution loop) plus the final tree assembly. The root node
/// is parameterised (`root_kind` / `root_label`) so the spec trace passes
/// `("spec", spec_name)` and the session trace passes `("session", session_id)`
/// — every nested level is identical, which is the whole point (one renderer,
/// one shape, no parallel view).
fn build_trace_tree(
    all_events: &[Value],
    role_map: &[RoleWave],
    agent_tokens: &HashMap<String, i64>,
    agent_cost_micros: &HashMap<String, i64>,
    root_kind: &str,
    root_label: &str,
    motivations: &HashMap<String, String>,
) -> Value {
    // Pass 1: build subagent intervals from `agent.start`/`agent.stop` events.
    //
    // A subagent's tools are NOT tagged with its identity on the wire — every
    // `tool.use` carries `actor="metrics-tracker"`, an empty `wave`, and no
    // `tool_use_id` that matches the inner tools (the `agent.start.tool_use_id`
    // is the *Task spawn* id, not the inner tools' ids). The only honest signal
    // is *time*: the `tool.use` events that fall between an `agent.start` and
    // its matching `agent.stop` within the same `session_id` belong to that
    // subagent; tools outside every interval belong to the orchestrator. See
    // [`build_agent_intervals`].
    //
    // The `role → wave` map (read from this spec's `wave-{N}-{role}` dirs) lets a
    // dispatch whose description carries a role name but no `wave N` token (e.g. a
    // `Review backend-ledger` review pass) still attribute to its wave. Empty for a
    // session (or a non-wave Light spec), in which case the resolution falls back
    // to `parse_wave` only.
    let intervals = build_agent_intervals(all_events, role_map);

    // Pass 1.5: pair every `tool.result` event with its originating `tool.use`.
    //
    // The PostToolUse `tool_result_observer` (apps/rt) emits a separate
    // `tool.result` NDJSON record carrying the captured side-effects
    // (`stdout_excerpt` / `stderr_excerpt` / `content_excerpt` / file diff). The
    // frontend `<ToolEventRow>` reads them off `payload.result` on the tool node,
    // so we splice the matching result payload into each `tool.use` node here —
    // without it the renderer always shows "tool_result pendente". The pairing
    // is built once into a `ResultPairing` (a `tool_use_id → result` map plus a
    // chronologically-sorted fallback queue) to stay linear in the event count.
    let mut pairing = ResultPairing::build(all_events);

    // Pass 2: attribute every `tool.use` to the subagent whose interval contains
    // it (the innermost when nested), else the orchestrator. The attribution
    // yields `(agent label, subagent_type, wave)`; the tree then nests
    // spec → [wave] → agent → tool. A tool with a wave gets the wave level; a
    // tool without one (orchestrator, or a subagent whose description carried no
    // wave number) is bucketed under the synthetic `NO_WAVE` key so the agent
    // node hangs straight off the spec — the renderer collapses that bucket.
    const NO_WAVE: &str = "\u{0}__no_wave__";
    // wave-key → agent-key → (AgentNodeMeta, ordered tool nodes). The agent key
    // is `(name, subagent_type)` so two distinct dispatches with the same
    // description but different types don't merge; the orchestrator is its own
    // key.
    #[derive(Default)]
    struct AgentBucket {
        subagent_type: Option<String>,
        /// Economy cost key for this agent (`agent_id`); `None` for the
        /// orchestrator. Token/cost roll-up is looked up by this, not the label.
        agent_id: Option<String>,
        /// The dispatching `agent.start.tool_use_id` (Task-spawn id), captured
        /// from the first attributed tool's interval. Used to splice the spawning
        /// motivation onto the agent node. `None` for the orchestrator.
        spawn_tool_use_id: Option<String>,
        tools: Vec<(Option<u64>, Value)>,
    }
    let mut by_wave: BTreeMap<String, BTreeMap<String, AgentBucket>> = BTreeMap::new();
    for ev in all_events {
        let ev_name = ev.get("event").and_then(Value::as_str).unwrap_or("");
        if ev_name != "tool.use" {
            continue;
        }
        let payload = ev.get("payload").cloned().unwrap_or_default();
        let attr = attribute_tool(ev, &intervals);

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
        // Splice the paired `tool.result` payload onto `payload.result` so the
        // frontend renders the captured stdout/diff/content instead of the
        // "tool_result pendente" placeholder. `pair_for` consumes the matched
        // result (id hit, else next chronological one for this tool) so two
        // identical commands never share a single result. Done before `label`
        // (which moves `tool_name`) so the borrow is still valid.
        let mut payload = payload;
        if let Some(result) = pairing.pair_for(ev, &tool_name) {
            if let Value::Object(map) = &mut payload {
                map.insert("result".to_string(), result);
            }
        }
        // Splice the assistant narration that motivated this tool onto
        // `payload.motivation` (sibling of `result`), matched by the event's
        // `payload.tool_use_id` against the transcript's `tool_use.id`. Absent
        // for tools with no preceding narration — most tools, which is fine.
        if let Some(tool_use_id) = payload
            .get("tool_use_id")
            .and_then(Value::as_str)
            .map(str::to_string)
        {
            if let Some(motivation) = motivations.get(&tool_use_id) {
                if let Value::Object(map) = &mut payload {
                    map.insert(
                        "motivation".to_string(),
                        Value::String(motivation.clone()),
                    );
                }
            }
        }
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

        let wave_key = attr
            .wave
            .map(|w| format!("wave-{w}"))
            .unwrap_or_else(|| NO_WAVE.to_string());
        // Capture the spawn id before `attr.agent` is moved into `entry`.
        let spawn_tool_use_id = attr.spawn_tool_use_id;
        let agent_bucket = by_wave
            .entry(wave_key)
            .or_default()
            .entry(attr.agent)
            .or_default();
        if agent_bucket.subagent_type.is_none() {
            agent_bucket.subagent_type = attr.subagent_type;
        }
        if agent_bucket.agent_id.is_none() {
            agent_bucket.agent_id = attr.agent_id;
        }
        // Record the dispatching Task-spawn id from the first attributed tool's
        // interval (every tool of one dispatch shares it). The orchestrator has
        // no interval, so its bucket keeps `None` — fail-open: no motivation.
        if agent_bucket.spawn_tool_use_id.is_none() {
            agent_bucket.spawn_tool_use_id = spawn_tool_use_id;
        }
        agent_bucket.tools.push((ts_ms_of(ev), tool_node));
    }

    // Pass 2.5: collect "what I asked" as root-level `kind:"prompt"` nodes,
    // surfaced at the top of the trace before any agent/wave activity. Two event
    // sources feed this:
    //   - `user.prompt` — a free-text turn; the request lives on `payload.prompt`.
    //   - `skill.invoked` — a slash command like `/feature`; the request text is
    //     the skill's `payload.args` (e.g. `{"skill":"mustard:feature","args":…}`).
    //     OLD sessions predate `user.prompt` entirely yet still recorded
    //     `skill.invoked`, so collecting it is what makes a retroactive session
    //     show the request at the top instead of bare collapsed agent nodes.
    // Each carries the full text both as the (truncated-in-header) `label` and
    // verbatim under `payload.prompt` so the frontend can expand to the multiline
    // original. The combined set is ordered by `ts` ASC and spliced at the FRONT
    // of `children`. When neither event exists the tree is byte-identical to
    // before (no node injected) — the `spec_trace_*` / `trace_*` tests stay green.
    let mut prompt_nodes: Vec<(Option<u64>, Value)> = Vec::new();
    for ev in all_events {
        match event_name(ev) {
            "user.prompt" => {
                let prompt = ev
                    .get("payload")
                    .and_then(|p| p.get("prompt"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let ts = ev.get("ts").and_then(Value::as_str).map(str::to_string);
                prompt_nodes.push((
                    ts_ms_of(ev),
                    serde_json::json!({
                        "kind": "prompt",
                        "label": prompt,
                        "tokens": null,
                        "duration_ms": null,
                        "ts": ts,
                        "payload": { "prompt": prompt },
                        "children": [],
                    }),
                ));
            }
            "skill.invoked" => {
                // The skill's `args` IS the request text. Skip empty-args
                // invocations (a bare `/status` etc.) — they carry no request to
                // surface and would render an empty prompt node.
                let payload = ev.get("payload");
                let args = payload
                    .and_then(|p| p.get("args"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if args.is_empty() {
                    continue;
                }
                let skill = payload
                    .and_then(|p| p.get("skill"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let ts = ev.get("ts").and_then(Value::as_str).map(str::to_string);
                prompt_nodes.push((
                    ts_ms_of(ev),
                    serde_json::json!({
                        "kind": "prompt",
                        "label": args,
                        "tokens": null,
                        "duration_ms": null,
                        "ts": ts,
                        "payload": { "prompt": args, "skill": skill },
                        "children": [],
                    }),
                ));
            }
            _ => continue,
        }
    }
    prompt_nodes.sort_by_key(|(ts, _)| *ts);

    // Build the tree. Tools with a wave nest spec → wave → agent → tool; the
    // synthetic `NO_WAVE` bucket's agents (orchestrator + unparsed subagents)
    // attach straight under the spec, so the frontend `<ExecutionTrace>` (which
    // recurses over `children` at any depth) never shows a spurious wave node.
    // Prompt nodes (collected above) are prepended before the agent/wave nodes.
    let mut children: Vec<Value> =
        prompt_nodes.into_iter().map(|(_, node)| node).collect();
    for (wave_key, agents) in by_wave {
        let agent_nodes: Vec<Value> = agents
            .into_iter()
            .map(|(agent_name, mut bucket)| {
                // Order each agent's tools by timestamp (ts_ms; None sorts first).
                bucket.tools.sort_by_key(|(ts, _)| *ts);
                let tool_nodes: Vec<Value> =
                    bucket.tools.into_iter().map(|(_, node)| node).collect();
                // Roll-up tokens/cost are keyed on the economy `agent_id`, not the
                // display label — the orchestrator (no `agent_id`) carries neither.
                let (tokens, cost_micros) = bucket
                    .agent_id
                    .as_deref()
                    .map(|id| {
                        (
                            lookup_agent_metric(agent_tokens, id),
                            lookup_agent_metric(agent_cost_micros, id),
                        )
                    })
                    .unwrap_or((None, None));
                // Splice the narration that motivated the SPAWN onto the agent
                // node: the assistant `text` block preceding the `Task(…)` call
                // is keyed in the transcript under the spawn's `tool_use_id`
                // (`agent.start.payload.tool_use_id`). When present, the node
                // carries `{ motivation, tool_use_id }` so the renderer shows a
                // preview under the label; else `payload: null` (today's shape) —
                // fail-open at every step (no id / no narration → null).
                let payload = bucket
                    .spawn_tool_use_id
                    .as_deref()
                    .and_then(|id| motivations.get(id).map(|m| (id, m)))
                    .map(|(id, motivation)| {
                        serde_json::json!({
                            "motivation": motivation,
                            "tool_use_id": id,
                        })
                    })
                    .unwrap_or(Value::Null);
                serde_json::json!({
                    "kind": "agent",
                    "label": agent_name,
                    "subagent_type": bucket.subagent_type,
                    "tokens": tokens,
                    "cost_usd_micros": cost_micros,
                    "duration_ms": null,
                    "ts": null,
                    "payload": payload,
                    "children": tool_nodes,
                })
            })
            .collect();
        if wave_key == NO_WAVE {
            children.extend(agent_nodes);
        } else {
            children.push(serde_json::json!({
                "kind": "wave",
                "label": wave_key,
                "tokens": null,
                "duration_ms": null,
                "ts": null,
                "payload": null,
                "children": agent_nodes,
            }));
        }
    }

    serde_json::json!({
        "kind": root_kind,
        "label": root_label,
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
    fn spec_trace_lists_tool_use_events_under_spec_root() {
        let tmp = TempDir::new().unwrap();
        let lines = concat!(
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:00:00.000Z","spec":"alpha","payload":{"tool":"Read","target":{"file_path":"src/foo.rs"}}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:01:00.000Z","spec":"alpha","payload":{"tool":"Edit","target":{"file_path":"src/bar.rs"}}}"#, "\n",
            r#"{"event":"pipeline.phase","kind":"pipeline","ts":"2026-05-27T09:02:00.000Z","spec":"alpha","payload":{"to":"PLAN"}}"#, "\n",
        );
        write_event(tmp.path(), "alpha", "events.ndjson", lines);
        let trace = dashboard_spec_trace_impl(tmp.path().to_string_lossy().into_owned(), "alpha".to_string());
        assert_eq!(trace["kind"], "spec");
        assert_eq!(trace["label"], "alpha");
        // No `agent.start`/`agent.stop` bracket these tools, so both attribute to
        // the orchestrator: spec → orchestrator agent (no wave) → the two tools.
        let children = trace["children"].as_array().expect("children array");
        assert_eq!(children.len(), 1, "single orchestrator agent under the spec");
        let orch = &children[0];
        assert_eq!(orch["kind"], "agent");
        assert_eq!(orch["label"], "orquestrador");
        let tools = orch["children"].as_array().expect("tool children");
        assert!(tools.iter().any(|c| c["label"].as_str().unwrap_or("").contains("Read")));
        assert!(tools.iter().any(|c| c["label"].as_str().unwrap_or("").contains("Edit")));
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
        // A real session: session.start (carries cwd) + later tool.use events.
        // Two Reads on the SAME file + one Edit on a second file exercise the
        // fold: tools_used=3, files_touched=2 (distinct), breakdown Read>Edit.
        let sess_lines = concat!(
            r#"{"event":"session.start","kind":"session","ts":"2026-05-27T08:00:00.000Z","session_id":"sess-1","spec":null,"payload":{"cwd":"C:\\repo","source":"startup"}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T08:05:00.000Z","session_id":"sess-1","spec":"alpha","payload":{"tool":"Read","target":{"file":"a.rs"}}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T08:06:00.000Z","session_id":"sess-1","spec":"alpha","payload":{"tool":"Read","target":{"file":"a.rs"}}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T08:07:00.000Z","session_id":"sess-1","spec":"alpha","payload":{"tool":"Edit","target":{"file_path":"b.rs"}}}"#, "\n",
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
        assert_eq!(s1.last_activity_at.as_deref(), Some("2026-05-27T08:07:00.000Z"));
        assert_eq!(s1.last_spec.as_deref(), Some("alpha"));
        assert_eq!(s1.cwd.as_deref(), Some("C:\\repo"));
        assert_eq!(s1.event_count, 4);
        assert_eq!(s1.status, "closed"); // 2026 timestamps are far in the past
        assert!(!s1.is_unknown_bucket);
        // The fold: 3 tool.use events over 2 distinct files; Read (2) > Edit (1).
        assert_eq!(s1.tools_used, 3, "three tool.use events");
        assert_eq!(s1.files_touched, 2, "two distinct files (a.rs counted once)");
        assert_eq!(s1.files, vec!["a.rs".to_string(), "b.rs".to_string()]);
        assert_eq!(s1.tool_breakdown.len(), 2);
        assert_eq!(s1.tool_breakdown[0].name, "Read");
        assert_eq!(s1.tool_breakdown[0].count, 2);
        assert_eq!(s1.tool_breakdown[1].name, "Edit");
        assert_eq!(s1.tool_breakdown[1].count, 1);

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

    #[test]
    fn sessions_derive_category_and_title_from_skill_invoked() {
        let tmp = TempDir::new().unwrap();
        // A mustard:task skill carrying the request text → category "task",
        // title from `payload.args`. A later mustard:bugfix must NOT win
        // (earliest mustard skill decides). A non-mustard skill is ignored once
        // a mustard one exists.
        let lines = concat!(
            r#"{"event":"skill.invoked","kind":"skill","ts":"2026-05-27T08:00:00.000Z","session_id":"sess-1","payload":{"skill":"mustard:task","args":"fazer X\nlinha dois ignorada"}}"#, "\n",
            r#"{"event":"skill.invoked","kind":"skill","ts":"2026-05-27T08:10:00.000Z","session_id":"sess-1","payload":{"skill":"mustard:bugfix","args":"corrigir Y"}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T08:11:00.000Z","session_id":"sess-1","payload":{"tool":"Read","target":{"file":"a.rs"}}}"#, "\n",
        );
        write_session_event(tmp.path(), "sess-1", "events.ndjson", lines);

        let rows = dashboard_sessions(tmp.path().to_string_lossy().into_owned(), None);
        let s1 = rows.iter().find(|r| r.id == "sess-1").expect("sess-1 row");
        assert_eq!(s1.category.as_deref(), Some("task"), "earliest mustard skill wins category");
        let title = s1.title.as_deref().expect("title from skill args");
        assert!(title.starts_with("fazer X"), "title from earliest mustard args, got {title:?}");
        assert!(!title.contains('\n'), "title is a single line");
    }

    #[test]
    fn sessions_category_outros_for_non_mustard_skill_and_prompt_title_fallback() {
        let tmp = TempDir::new().unwrap();
        // No mustard skill: a non-mustard skill (no args) → category "outros".
        // No skill args anywhere → title falls back to the user.prompt text.
        let lines = concat!(
            r#"{"event":"user.prompt","kind":"prompt","ts":"2026-05-27T07:59:00.000Z","session_id":"sess-2","payload":{"prompt":"meu pedido livre"}}"#, "\n",
            r#"{"event":"skill.invoked","kind":"skill","ts":"2026-05-27T08:00:00.000Z","session_id":"sess-2","payload":{"skill":"frontend-design:frontend-design","args":""}}"#, "\n",
        );
        write_session_event(tmp.path(), "sess-2", "events.ndjson", lines);

        let rows = dashboard_sessions(tmp.path().to_string_lossy().into_owned(), None);
        let s2 = rows.iter().find(|r| r.id == "sess-2").expect("sess-2 row");
        assert_eq!(s2.category.as_deref(), Some("outros"), "non-mustard skill → outros");
        assert_eq!(s2.title.as_deref(), Some("meu pedido livre"), "title from user.prompt fallback");
    }

    #[test]
    fn sessions_category_none_when_no_skill_invoked() {
        let tmp = TempDir::new().unwrap();
        // Only tool.use — no skill.invoked at all → category None ("avulsa").
        let lines = concat!(
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T08:05:00.000Z","session_id":"sess-3","payload":{"tool":"Read","target":{"file":"a.rs"}}}"#, "\n",
        );
        write_session_event(tmp.path(), "sess-3", "events.ndjson", lines);

        let rows = dashboard_sessions(tmp.path().to_string_lossy().into_owned(), None);
        let s3 = rows.iter().find(|r| r.id == "sess-3").expect("sess-3 row");
        assert_eq!(s3.category, None, "no skill.invoked → avulsa (None)");
    }

    #[test]
    fn sessions_derive_kind_and_scope_from_earliest_pipeline_kind() {
        let tmp = TempDir::new().unwrap();
        // The earliest `pipeline.kind` decides — it is emitted when the router
        // dispatches the flow, so a later one (a second request in the same
        // session) must NOT override the original classification.
        let lines = concat!(
            r#"{"event":"pipeline.kind","kind":"pipeline","ts":"2026-05-27T08:00:00.000Z","session_id":"sess-k","payload":{"kind":"bugfix","scope":"lean"}}"#, "\n",
            r#"{"event":"pipeline.kind","kind":"pipeline","ts":"2026-05-27T08:10:00.000Z","session_id":"sess-k","payload":{"kind":"feature","scope":"full"}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T08:11:00.000Z","session_id":"sess-k","payload":{"tool":"Read","target":{"file":"a.rs"}}}"#, "\n",
        );
        write_session_event(tmp.path(), "sess-k", "events.ndjson", lines);

        let rows = dashboard_sessions(tmp.path().to_string_lossy().into_owned(), None);
        let s = rows.iter().find(|r| r.id == "sess-k").expect("sess-k row");
        assert_eq!(s.kind.as_deref(), Some("bugfix"), "earliest pipeline.kind wins");
        assert_eq!(s.scope.as_deref(), Some("lean"));
    }

    #[test]
    fn sessions_kind_none_when_no_pipeline_kind_event() {
        let tmp = TempDir::new().unwrap();
        // A session with work but no `pipeline.kind` (older session, or untagged
        // work) reports no kind/scope rather than guessing one.
        let lines = concat!(
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T08:05:00.000Z","session_id":"sess-nk","payload":{"tool":"Read","target":{"file":"a.rs"}}}"#, "\n",
        );
        write_session_event(tmp.path(), "sess-nk", "events.ndjson", lines);

        let rows = dashboard_sessions(tmp.path().to_string_lossy().into_owned(), None);
        let s = rows.iter().find(|r| r.id == "sess-nk").expect("sess-nk row");
        assert_eq!(s.kind, None);
        assert_eq!(s.scope, None);
    }

    #[test]
    fn session_trace_groups_session_tool_events() {
        let tmp = TempDir::new().unwrap();
        // Two orchestrator-level tools in a session — no `agent.start`/`agent.stop`
        // bracket, no waves — so the session trace must nest
        // session → orchestrator agent → the two tools, exactly like the spec
        // trace does for the no-wave case (one shared `build_trace_tree`).
        let lines = concat!(
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T08:05:00.000Z","session_id":"sess-1","payload":{"tool":"Read","target":{"file_path":"a.rs"}}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T08:06:00.000Z","session_id":"sess-1","payload":{"tool":"Edit","target":{"file_path":"b.rs"}}}"#, "\n",
        );
        write_session_event(tmp.path(), "sess-1", "events.ndjson", lines);

        let trace = dashboard_session_trace_impl(
            tmp.path().to_string_lossy().into_owned(),
            "sess-1".to_string(),
        );
        assert_eq!(trace["kind"], "session");
        assert_eq!(trace["label"], "sess-1");
        let children = trace["children"].as_array().expect("children array");
        assert_eq!(children.len(), 1, "single orchestrator agent under the session");
        let orch = &children[0];
        assert_eq!(orch["kind"], "agent");
        let tools = orch["children"].as_array().expect("tool children");
        assert!(tools.iter().any(|c| c["label"].as_str().unwrap_or("").contains("Read")));
        assert!(tools.iter().any(|c| c["label"].as_str().unwrap_or("").contains("Edit")));
    }

    #[test]
    fn session_trace_surfaces_user_prompt_node_before_agents() {
        let tmp = TempDir::new().unwrap();
        // A user prompt plus one orchestrator tool in the same session. The
        // `user.prompt` event must surface as a root-level `kind:"prompt"` node
        // carrying the full text, positioned BEFORE the agent node.
        let lines = concat!(
            r#"{"event":"user.prompt","kind":"prompt","ts":"2026-05-27T08:04:00.000Z","session_id":"sess-1","payload":{"prompt":"adicione um campo de status\nna tabela de pedidos"}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T08:05:00.000Z","session_id":"sess-1","payload":{"tool":"Read","target":{"file_path":"a.rs"}}}"#, "\n",
        );
        write_session_event(tmp.path(), "sess-1", "events.ndjson", lines);

        let trace = dashboard_session_trace_impl(
            tmp.path().to_string_lossy().into_owned(),
            "sess-1".to_string(),
        );
        let children = trace["children"].as_array().expect("children array");
        // Prompt node is FIRST (before the orchestrator agent node).
        assert_eq!(children[0]["kind"], "prompt", "prompt node leads children");
        assert_eq!(
            children[0]["label"].as_str().unwrap_or(""),
            "adicione um campo de status\nna tabela de pedidos",
            "prompt label carries the full text"
        );
        assert_eq!(
            children[0]["payload"]["prompt"].as_str().unwrap_or(""),
            "adicione um campo de status\nna tabela de pedidos",
            "prompt payload carries the full text"
        );
        // The agent node follows the prompt.
        assert!(
            children.iter().skip(1).any(|c| c["kind"] == "agent"),
            "agent node appears after the prompt node"
        );
    }

    #[test]
    fn session_trace_fail_open_when_session_missing() {
        let tmp = TempDir::new().unwrap();
        let trace = dashboard_session_trace_impl(
            tmp.path().to_string_lossy().into_owned(),
            "nope".to_string(),
        );
        // Fail-open: missing session dir → empty-children session root, never Err.
        assert_eq!(trace["kind"], "session");
        assert_eq!(trace["label"], "nope");
        assert_eq!(trace["children"].as_array().expect("children array").len(), 0);
    }

    // ── Read-time session→spec attribution ───────────────────────────────────

    #[test]
    fn timeline_attributes_specless_event_to_time_ordered_binding() {
        // session sess-1 bound to spec-X at 09:00 (via pipeline.scope), then to
        // spec-Y at 10:00 (via pipeline.stage). A spec-less tool.use at 09:30
        // must attribute to spec-X; one at 10:30 to spec-Y; one at 08:30 (before
        // any binding) stays unattributed.
        let scope = r#"{"event":"pipeline.scope","kind":"pipeline","ts":"2026-05-27T09:00:00.000Z","session_id":"sess-1","spec":"spec-X","payload":{"scope":"full"}}"#;
        let stage = r#"{"event":"pipeline.stage","kind":"pipeline","ts":"2026-05-27T10:00:00.000Z","session_id":"sess-1","spec":"spec-Y","payload":{"to":"EXECUTE"}}"#;
        let records: Vec<Value> = [scope, stage]
            .iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let timeline = build_session_spec_timeline_from(&records);

        let mk = |ts: &str| -> Value {
            serde_json::from_str(&format!(
                r#"{{"event":"tool.use","kind":"tool","ts":"{ts}","session_id":"sess-1","spec":null,"payload":{{"tool":"Read"}}}}"#
            ))
            .unwrap()
        };
        assert_eq!(
            timeline.attributed_spec(&mk("2026-05-27T09:30:00.000Z")),
            Some("spec-X")
        );
        assert_eq!(
            timeline.attributed_spec(&mk("2026-05-27T10:30:00.000Z")),
            Some("spec-Y")
        );
        // Before the first binding → unattributed.
        assert_eq!(timeline.attributed_spec(&mk("2026-05-27T08:30:00.000Z")), None);
        // Unknown session → unattributed.
        let other = serde_json::from_str::<Value>(
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:30:00.000Z","session_id":"sess-other","spec":null,"payload":{}}"#,
        )
        .unwrap();
        assert_eq!(timeline.attributed_spec(&other), None);
    }

    #[test]
    fn timeline_honours_explicit_spec_without_override() {
        // An event that already carries a non-empty spec is returned verbatim,
        // even if a binding for its session points elsewhere.
        let scope = r#"{"event":"pipeline.scope","kind":"pipeline","ts":"2026-05-27T09:00:00.000Z","session_id":"sess-1","spec":"spec-X","payload":{}}"#;
        let records: Vec<Value> = vec![serde_json::from_str(scope).unwrap()];
        let timeline = build_session_spec_timeline_from(&records);
        let ev = serde_json::from_str::<Value>(
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:30:00.000Z","session_id":"sess-1","spec":"spec-explicit","payload":{}}"#,
        )
        .unwrap();
        assert_eq!(timeline.attributed_spec(&ev), Some("spec-explicit"));
    }

    #[test]
    fn spec_trace_includes_attributed_session_tool_event() {
        // The reported bug: a spec under active EXECUTE shows empty because its
        // session's work events live in `.session/{id}/.events/` with spec=null.
        // With a `pipeline.scope` binding (session=sess-1 → spec=alpha at an
        // EARLIER ts) under the spec's own `.events/`, the spec trace must surface
        // the spec-less session `tool.use`.
        let tmp = TempDir::new().unwrap();
        // Binding event lives under the spec dir (as on disk).
        let binding = r#"{"event":"pipeline.scope","kind":"pipeline","ts":"2026-05-27T09:00:00.000Z","session_id":"sess-1","spec":"alpha","payload":{"scope":"full"}}"#;
        write_event(tmp.path(), "alpha", "scope.ndjson", &format!("{binding}\n"));
        // The spec-less work event lives under the session sink (as on disk).
        let tool = r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:30:00.000Z","session_id":"sess-1","spec":null,"payload":{"tool":"Edit","target":{"file_path":"src/live.rs"}}}"#;
        write_session_event(tmp.path(), "sess-1", "work.ndjson", &format!("{tool}\n"));

        let trace = dashboard_spec_trace_impl(
            tmp.path().to_string_lossy().into_owned(),
            "alpha".to_string(),
        );
        let flat = trace.to_string();
        assert!(
            flat.contains("src/live.rs"),
            "attributed session tool.use must appear in spec alpha's trace; got: {flat}"
        );
    }

    // ── Time-interval agent attribution ──────────────────────────────────────

    fn start_line(session: &str, ts_ms: u64, subagent_type: &str, description: &str) -> Value {
        serde_json::json!({
            "event": "agent.start", "kind": "agent", "ts_ms": ts_ms,
            "session_id": session,
            "payload": { "description": description, "subagentType": subagent_type }
        })
    }
    fn stop_line(session: &str, ts_ms: u64) -> Value {
        serde_json::json!({
            "event": "agent.stop", "kind": "agent", "ts_ms": ts_ms,
            "session_id": session, "payload": { "summary": "{}" }
        })
    }
    fn use_line(session: &str, ts_ms: u64, tool: &str) -> Value {
        serde_json::json!({
            "event": "tool.use", "kind": "tool", "ts_ms": ts_ms,
            "session_id": session, "actor": "metrics-tracker",
            "payload": { "tool": tool }
        })
    }

    #[test]
    fn intervals_pair_sequential_starts_and_stops() {
        // Two sequential dispatches in one session — each start/stop closes a
        // distinct interval carrying its own description + type.
        let events = vec![
            start_line("s", 100, "Explore", "Trace blast radius"),
            stop_line("s", 200),
            start_line("s", 300, "general-purpose", "Wave 1 impl — backend"),
            stop_line("s", 400),
        ];
        let mut ivs = build_agent_intervals(&events, &[]);
        ivs.sort_by_key(|iv| iv.start_ms);
        assert_eq!(ivs.len(), 2);
        assert_eq!(ivs[0].name, "Trace blast radius");
        assert_eq!(ivs[0].subagent_type, "Explore");
        assert_eq!((ivs[0].start_ms, ivs[0].end_ms), (100, 200));
        assert_eq!(ivs[1].name, "Wave 1 impl — backend");
        assert_eq!(ivs[1].subagent_type, "general-purpose");
        assert_eq!(ivs[1].wave, Some(1));
    }

    #[test]
    fn interval_unclosed_start_closes_at_max() {
        // An `agent.start` with no matching stop (subagent still running) gets an
        // open-ended interval so its in-flight tools still attribute.
        let events = vec![start_line("s", 100, "Explore", "Investigate")];
        let ivs = build_agent_intervals(&events, &[]);
        assert_eq!(ivs.len(), 1);
        assert_eq!(ivs[0].end_ms, u64::MAX);
        assert_eq!(ivs[0].name, "Investigate");
    }

    #[test]
    fn interval_ignores_empty_subagent_type_start() {
        // An `agent.start` with an empty subagentType is observer noise — it must
        // not open a frame (and so its paired stop pops nothing).
        let events = vec![
            start_line("s", 100, "", "noise"),
            start_line("s", 110, "Explore", "real work"),
            stop_line("s", 200),
        ];
        let ivs = build_agent_intervals(&events, &[]);
        assert_eq!(ivs.len(), 1, "only the typed start opens an interval");
        assert_eq!(ivs[0].name, "real work");
        assert_eq!((ivs[0].start_ms, ivs[0].end_ms), (110, 200));
    }

    #[test]
    fn interval_nesting_picks_innermost() {
        // A nested dispatch: the inner subagent's tools must attribute to it, not
        // the outer one. Both intervals bracket ts=150, innermost wins.
        let events = vec![
            start_line("s", 100, "general-purpose", "Outer"),
            start_line("s", 120, "Explore", "Inner"),
            stop_line("s", 180),
            stop_line("s", 220),
        ];
        let ivs = build_agent_intervals(&events, &[]);
        let inner = use_line("s", 150, "Read");
        let attr = attribute_tool(&inner, &ivs);
        assert_eq!(attr.agent, "Inner");
        assert_eq!(attr.subagent_type.as_deref(), Some("Explore"));
        // A tool after the inner stop but before the outer stop → the outer one.
        let outer = use_line("s", 200, "Edit");
        assert_eq!(attribute_tool(&outer, &ivs).agent, "Outer");
    }

    #[test]
    fn attribute_tool_orchestrator_vs_subagent() {
        // A tool inside an interval attributes to that subagent; one outside every
        // interval (and one in a different session) falls back to the orchestrator.
        let events = vec![
            start_line("s", 100, "Explore", "Trace payable"),
            stop_line("s", 200),
        ];
        let ivs = build_agent_intervals(&events, &[]);
        let inside = use_line("s", 150, "Grep");
        let a = attribute_tool(&inside, &ivs);
        assert_eq!(a.agent, "Trace payable");
        assert_eq!(a.subagent_type.as_deref(), Some("Explore"));
        assert_eq!(a.wave, None);

        let after = use_line("s", 250, "Bash");
        assert_eq!(attribute_tool(&after, &ivs).agent, ORCHESTRATOR);
        let other_session = use_line("other", 150, "Bash");
        assert_eq!(attribute_tool(&other_session, &ivs).agent, ORCHESTRATOR);
    }

    #[test]
    fn sessionless_events_do_not_correlate() {
        // FIX 1: a session-less `agent.start` (id "unknown") must NOT open an
        // interval, and a later session-less `tool.use` must NOT bucket under it —
        // collapsing both to one "" pseudo-session is exactly the cross-run
        // interleave the fix forbids. The tool stays with the orchestrator.
        let sessionless_start = serde_json::json!({
            "event": "agent.start", "kind": "agent", "ts_ms": 100u64,
            "session_id": "unknown",
            "payload": { "description": "Explore blast radius", "subagentType": "Explore" }
        });
        let empty_session_use = serde_json::json!({
            "event": "tool.use", "kind": "tool", "ts_ms": 150u64,
            "session_id": "", "payload": { "tool": "Read" }
        });
        let null_session_use = serde_json::json!({
            "event": "tool.use", "kind": "tool", "ts_ms": 160u64,
            "session_id": Value::Null, "payload": { "tool": "Grep" }
        });
        let ivs = build_agent_intervals(&[sessionless_start.clone()], &[]);
        assert!(ivs.is_empty(), "a session-less start opens no interval");
        assert_eq!(attribute_tool(&empty_session_use, &ivs).agent, ORCHESTRATOR);
        assert_eq!(attribute_tool(&null_session_use, &ivs).agent, ORCHESTRATOR);
    }

    #[test]
    fn attribute_tool_end_is_exclusive_at_stop_ms() {
        // FIX 3: a tool logged at the exact `agent.stop` ms ran after the dispatch
        // closed — it must NOT fall inside the just-popped interval. With no
        // enclosing interval it lands on the orchestrator; the start ms stays
        // inclusive, and an open (`u64::MAX`) interval keeps capturing later tools.
        let events = vec![
            start_line("s", 100, "Explore", "Trace payable"),
            stop_line("s", 200),
        ];
        let ivs = build_agent_intervals(&events, &[]);
        // Exactly at start → inside.
        assert_eq!(attribute_tool(&use_line("s", 100, "Read"), &ivs).agent, "Trace payable");
        // Exactly at stop → orchestrator (interval closed at this ms).
        assert_eq!(attribute_tool(&use_line("s", 200, "Bash"), &ivs).agent, ORCHESTRATOR);
        // Nested: the inner stop ms hands the tool back to the still-open outer.
        let nested = vec![
            start_line("s", 100, "general-purpose", "Outer"),
            start_line("s", 120, "Explore", "Inner"),
            stop_line("s", 180),
            stop_line("s", 220),
        ];
        let nivs = build_agent_intervals(&nested, &[]);
        assert_eq!(attribute_tool(&use_line("s", 180, "Edit"), &nivs).agent, "Outer");
    }

    #[test]
    fn agent_id_carries_through_to_token_lookup() {
        // FIX 2: the agent node's tokens come from the economy map keyed on
        // `agent_id` (the `agent.start.payload.agent_id`), not the description
        // label. A start carrying `agent_id` propagates it onto the interval and
        // the attribution so the lookup hits.
        let start = serde_json::json!({
            "event": "agent.start", "kind": "agent", "ts_ms": 100u64,
            "session_id": "s",
            "payload": {
                "description": "Wave 1 impl — backend",
                "subagentType": "general-purpose",
                "agent_id": "general-purpose"
            }
        });
        let ivs = build_agent_intervals(&[start, stop_line("s", 300)], &[]);
        assert_eq!(ivs.len(), 1);
        assert_eq!(ivs[0].agent_id, "general-purpose");
        let attr = attribute_tool(&use_line("s", 150, "Edit"), &ivs);
        assert_eq!(attr.agent, "Wave 1 impl — backend");
        assert_eq!(attr.agent_id.as_deref(), Some("general-purpose"));

        // The lookup keys on `agent_id`: an exact economy key hits; a label-keyed
        // map would miss. Prefix fallback resolves a suffixed run-event id.
        let mut tokens: HashMap<String, i64> = HashMap::new();
        tokens.insert("general-purpose".to_string(), 4242);
        assert_eq!(lookup_agent_metric(&tokens, "general-purpose"), Some(4242));

        let mut suffixed: HashMap<String, i64> = HashMap::new();
        suffixed.insert("Explore-1".to_string(), 777);
        assert_eq!(lookup_agent_metric(&suffixed, "Explore"), Some(777));
        // Ambiguous prefix (two runs of the same role) refuses to guess.
        suffixed.insert("Explore-2".to_string(), 888);
        assert_eq!(lookup_agent_metric(&suffixed, "Explore"), None);
        // No match → None (node omits tokens, never fabricates).
        assert_eq!(lookup_agent_metric(&tokens, "mustard-review"), None);
    }

    #[test]
    fn trace_surfaces_agent_tokens_keyed_by_agent_id() {
        // End-to-end: a sessioned Explore dispatch (carrying agent_id "Explore")
        // brackets a tool, and a run event books that agent's tokens under the
        // economy `agent_id` key. The agent node must surface those tokens —
        // proving the lookup keys on agent_id, not the description label.
        let tmp = TempDir::new().unwrap();
        let mut start = start_with_spec("s", 100, "Explore", "Trace blast radius", "alpha");
        start["session_id"] = Value::String("s".to_string());
        start["payload"]["agent_id"] = Value::String("Explore".to_string());
        let lines: Vec<String> = vec![
            serde_json::to_string(&start).unwrap(),
            serde_json::to_string(&use_with_spec("s", 120, "Grep", "alpha")).unwrap(),
            serde_json::to_string(&stop_with_spec("s", 200, "alpha")).unwrap(),
            // The economy run event for this agent (per_agent_costs keys on agent_id).
            r#"{"event":"pipeline.economy.run","kind":"pipeline.economy.run","ts":"2026-06-05T00:00:00.200Z","spec":"alpha","session_id":"s","payload":{"spec":"alpha","agent_id":"Explore","input_tokens":1000,"output_tokens":500,"cost_usd_micros":1234}}"#.to_string(),
        ];
        write_event(tmp.path(), "alpha", "events.ndjson", &format!("{}\n", lines.join("\n")));

        let trace = dashboard_spec_trace_impl(
            tmp.path().to_string_lossy().into_owned(),
            "alpha".to_string(),
        );
        let children = trace["children"].as_array().expect("children");
        let explore = children
            .iter()
            .find(|c| c["label"] == "Trace blast radius")
            .expect("Explore agent node");
        assert_eq!(explore["tokens"].as_i64(), Some(1500), "tokens keyed by agent_id surface");
        assert_eq!(explore["cost_usd_micros"].as_i64(), Some(1234));
    }

    #[test]
    fn parse_wave_reads_wave_and_onda() {
        assert_eq!(parse_wave("Wave 2 — backend optional FK"), Some(2));
        assert_eq!(parse_wave("onda 10 frontend"), Some(10));
        assert_eq!(parse_wave("WAVE 3"), Some(3));
        // No bare-word match: "software 1" / "wavelength" must not match.
        assert_eq!(parse_wave("software 1 release"), None);
        assert_eq!(parse_wave("wavelength 5"), None);
        assert_eq!(parse_wave("just a description"), None);
    }

    /// Create the `wave-{N}-{role}` subdirs for a spec, mirroring a materialised
    /// wave plan on disk (the real painel-financeiro layout).
    fn make_wave_dirs(tmp: &Path, spec: &str, waves: &[(&str, u32)]) {
        for (role, n) in waves {
            std::fs::create_dir_all(
                tmp.join(".claude")
                    .join("spec")
                    .join(spec)
                    .join(format!("wave-{n}-{role}")),
            )
            .unwrap();
        }
    }

    #[test]
    fn read_wave_role_map_parses_dir_names() {
        // The real painel-financeiro wave layout: wave-1-backend-ledger …
        // wave-5-app-caixa. Each dir name yields (role tokens, N).
        let tmp = TempDir::new().unwrap();
        let spec_dir = tmp.path().join(".claude").join("spec").join("pf");
        make_wave_dirs(
            tmp.path(),
            "pf",
            &[
                ("backend-ledger", 1),
                ("backend-cashflow", 2),
                ("core", 3),
                ("app-baixa", 4),
                ("app-caixa", 5),
            ],
        );
        // A noise dir (no `wave-` prefix) and a `.events` dir must be ignored.
        std::fs::create_dir_all(spec_dir.join(".events")).unwrap();
        std::fs::create_dir_all(spec_dir.join("notes")).unwrap();

        let mut map = read_wave_role_map(&spec_dir);
        map.sort_by_key(|(_, w)| *w);
        assert_eq!(
            map,
            vec![
                (vec!["backend".to_string(), "ledger".to_string()], 1),
                (vec!["backend".to_string(), "cashflow".to_string()], 2),
                (vec!["core".to_string()], 3),
                (vec!["app".to_string(), "baixa".to_string()], 4),
                (vec!["app".to_string(), "caixa".to_string()], 5),
            ]
        );
    }

    #[test]
    fn read_wave_role_map_failsoft_on_missing_dir() {
        // A non-wave (Light) spec whose dir doesn't exist → empty map, no panic.
        let tmp = TempDir::new().unwrap();
        assert!(read_wave_role_map(&tmp.path().join("nope")).is_empty());
    }

    #[test]
    fn match_role_wave_resolves_role_token_description() {
        // The role map mirrors the real painel-financeiro dirs.
        let map: Vec<RoleWave> = vec![
            (vec!["backend".to_string(), "ledger".to_string()], 1),
            (vec!["backend".to_string(), "cashflow".to_string()], 2),
            (vec!["core".to_string()], 3),
            (vec!["app".to_string(), "baixa".to_string()], 4),
            (vec!["app".to_string(), "caixa".to_string()], 5),
        ];
        // Real mustard-review dispatches that carry NO "wave N" — they name the role.
        assert_eq!(match_role_wave("Review backend-ledger", "mustard-review", &map), Some(1));
        assert_eq!(match_role_wave("Review backend-cashflow", "mustard-review", &map), Some(2));
        assert_eq!(match_role_wave("Review core", "mustard-review", &map), Some(3));
        assert_eq!(match_role_wave("Review app-baixa", "mustard-review", &map), Some(4));
        assert_eq!(match_role_wave("Review app-caixa", "mustard-review", &map), Some(5));
        // Separator-insensitive: "backend ledger impl" (spaces) matches the role.
        assert_eq!(match_role_wave("backend ledger impl", "general-purpose", &map), Some(1));
        // A bare role name dispatched as the subagent type also resolves.
        assert_eq!(match_role_wave("", "core", &map), Some(3));
        // Most-specific role wins: "backend-cashflow" must not collapse to a
        // hypothetical lone "backend" role — the 2-token role is matched whole.
        assert_eq!(match_role_wave("Review backend-cashflow ledger", "x", &map), Some(2));
    }

    #[test]
    fn match_role_wave_no_match_stays_waveless() {
        let map: Vec<RoleWave> = vec![
            (vec!["backend".to_string(), "ledger".to_string()], 1),
            (vec!["app".to_string(), "caixa".to_string()], 5),
        ];
        // The real orchestrator setup dispatch — names no role token.
        assert_eq!(match_role_wave("Build Painel Financeiro tabbed shell", "general-purpose", &map), None);
        assert_eq!(match_role_wave("Fix settle atomicity", "general-purpose", &map), None);
        // An empty role map (non-wave spec) never matches.
        assert_eq!(match_role_wave("Review backend-ledger", "mustard-review", &[]), None);
    }

    #[test]
    fn build_intervals_resolves_wave_via_role_when_no_wave_token() {
        // A role-named dispatch (no "wave N") gets its wave from the role map; a
        // "Wave 2" dispatch still wins via parse_wave even when its description
        // ALSO contains a role token (parse_wave is the first signal).
        let role_map: Vec<RoleWave> = vec![
            (vec!["backend".to_string(), "ledger".to_string()], 1),
            (vec!["core".to_string()], 3),
        ];
        let events = vec![
            start_line("s", 100, "mustard-review", "Review backend-ledger"),
            stop_line("s", 200),
            // parse_wave wins: "Wave 2" beats the "core" role token in the same text.
            start_line("s", 300, "general-purpose", "Wave 2 core regen"),
            stop_line("s", 400),
            // No wave token, no role token → stays wave-less.
            start_line("s", 500, "general-purpose", "Build tabbed shell"),
            stop_line("s", 600),
        ];
        let mut ivs = build_agent_intervals(&events, &role_map);
        ivs.sort_by_key(|iv| iv.start_ms);
        assert_eq!(ivs[0].wave, Some(1), "role token 'backend-ledger' → wave 1");
        assert_eq!(ivs[1].wave, Some(2), "explicit 'Wave 2' wins over role token");
        assert_eq!(ivs[2].wave, None, "no wave token, no role token → wave-less");
        // With an empty role map the role-named dispatch falls back to wave-less.
        let ivs_norole = build_agent_intervals(&events, &[]);
        let review = ivs_norole.iter().find(|iv| iv.name == "Review backend-ledger").unwrap();
        assert_eq!(review.wave, None, "no role map → role-named dispatch stays wave-less");
    }

    #[test]
    fn trace_attributes_role_named_review_to_wave_via_disk_map() {
        // End-to-end through `dashboard_spec_trace_impl`: the spec has real
        // `wave-{N}-{role}` dirs on disk, so a `Review backend-ledger` dispatch
        // (no "wave N" token) attributes to the `wave-1` node, while an
        // unmatched orchestrator dispatch hangs straight off the spec.
        let tmp = TempDir::new().unwrap();
        make_wave_dirs(tmp.path(), "pf", &[("backend-ledger", 1), ("core", 3)]);
        let lines: Vec<String> = vec![
            serde_json::to_string(&start_with_spec("s", 100, "mustard-review", "Review backend-ledger", "pf")).unwrap(),
            serde_json::to_string(&use_with_spec("s", 120, "Read", "pf")).unwrap(),
            serde_json::to_string(&stop_with_spec("s", 200, "pf")).unwrap(),
            serde_json::to_string(&start_with_spec("s", 300, "general-purpose", "Build tabbed shell", "pf")).unwrap(),
            serde_json::to_string(&use_with_spec("s", 320, "Edit", "pf")).unwrap(),
            serde_json::to_string(&stop_with_spec("s", 400, "pf")).unwrap(),
        ];
        write_event(tmp.path(), "pf", "events.ndjson", &format!("{}\n", lines.join("\n")));

        let trace = dashboard_spec_trace_impl(
            tmp.path().to_string_lossy().into_owned(),
            "pf".to_string(),
        );
        let children = trace["children"].as_array().expect("children");

        // The review nests under a wave-1 node (its tool Read is inside).
        let wave1 = children
            .iter()
            .find(|c| c["kind"] == "wave" && c["label"] == "wave-1")
            .expect("wave-1 node for the role-matched review");
        let review = wave1["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["label"] == "Review backend-ledger")
            .expect("review agent under wave-1");
        assert_eq!(review["children"].as_array().unwrap().len(), 1, "the Read tool");

        // The unmatched orchestrator-style dispatch hangs off the spec, not a wave.
        assert!(
            children.iter().any(|c| c["kind"] == "agent" && c["label"] == "Build tabbed shell"),
            "the role-less dispatch attaches straight under the spec"
        );
    }

    #[test]
    fn trace_groups_tools_under_named_subagent_then_wave() {
        // End-to-end on the validated real shape: an Explore dispatch brackets two
        // tools, and a `Wave 2` general-purpose dispatch brackets one more. The
        // Explore tools nest under its description (NOT "metrics-tracker") with no
        // wave; the Wave 2 tool nests under a `wave-2` node.
        let tmp = TempDir::new().unwrap();
        let lines: Vec<String> = vec![
            serde_json::to_string(&use_with_spec("s", 50, "Read", "alpha")).unwrap(),
            serde_json::to_string(&start_with_spec("s", 100, "Explore", "Trace payable blast radius", "alpha")).unwrap(),
            serde_json::to_string(&use_with_spec("s", 120, "Grep", "alpha")).unwrap(),
            serde_json::to_string(&use_with_spec("s", 140, "Read", "alpha")).unwrap(),
            serde_json::to_string(&stop_with_spec("s", 200, "alpha")).unwrap(),
            serde_json::to_string(&start_with_spec("s", 300, "general-purpose", "Wave 2 impl — frontend", "alpha")).unwrap(),
            serde_json::to_string(&use_with_spec("s", 320, "Edit", "alpha")).unwrap(),
            serde_json::to_string(&stop_with_spec("s", 400, "alpha")).unwrap(),
        ];
        write_event(tmp.path(), "alpha", "events.ndjson", &format!("{}\n", lines.join("\n")));

        let trace = dashboard_spec_trace_impl(
            tmp.path().to_string_lossy().into_owned(),
            "alpha".to_string(),
        );
        let children = trace["children"].as_array().expect("children");

        // The Explore subagent node hangs off the spec (no wave), labelled by its
        // description and carrying the type badge.
        let explore = children
            .iter()
            .find(|c| c["label"] == "Trace payable blast radius")
            .expect("Explore agent node under spec");
        assert_eq!(explore["kind"], "agent");
        assert_eq!(explore["subagent_type"], "Explore");
        let explore_tools = explore["children"].as_array().unwrap();
        assert_eq!(explore_tools.len(), 2, "Grep + Read inside Explore");
        // NOT attributed to the hardcoded observer name.
        assert!(children.iter().all(|c| c["label"] != "metrics-tracker"));

        // The orchestrator's pre-dispatch Read (ts=50) hangs off the spec too.
        let orch = children
            .iter()
            .find(|c| c["label"] == ORCHESTRATOR)
            .expect("orchestrator node");
        assert_eq!(orch["children"].as_array().unwrap().len(), 1);

        // The Wave 2 dispatch lives under a `wave-2` node.
        let wave2 = children
            .iter()
            .find(|c| c["kind"] == "wave" && c["label"] == "wave-2")
            .expect("wave-2 node");
        let agent = &wave2["children"].as_array().unwrap()[0];
        assert_eq!(agent["label"], "Wave 2 impl — frontend");
        assert_eq!(agent["subagent_type"], "general-purpose");
        assert_eq!(agent["children"].as_array().unwrap().len(), 1);
    }

    fn use_with_spec(session: &str, ts_ms: u64, tool: &str, spec: &str) -> Value {
        let mut v = use_line(session, ts_ms, tool);
        v["spec"] = Value::String(spec.to_string());
        v["ts"] = Value::String(format!("2026-06-05T00:00:{ts_ms:03}Z"));
        v
    }
    fn start_with_spec(session: &str, ts_ms: u64, ty: &str, desc: &str, spec: &str) -> Value {
        let mut v = start_line(session, ts_ms, ty, desc);
        v["spec"] = Value::String(spec.to_string());
        v
    }
    fn stop_with_spec(session: &str, ts_ms: u64, spec: &str) -> Value {
        let mut v = stop_line(session, ts_ms);
        v["spec"] = Value::String(spec.to_string());
        v
    }

    #[test]
    fn attributed_counts_fold_specless_session_work_onto_bound_spec() {
        // Binding pins sess-1 → alpha at 09:00; two spec-less session tool.use
        // events (distinct files) after the binding attribute to alpha, plus one
        // explicit-spec tool.use also under alpha. Counts must aggregate all
        // three tools and two distinct session files without double-counting.
        let records: Vec<Value> = [
            r#"{"event":"pipeline.scope","kind":"pipeline","ts":"2026-05-27T09:00:00.000Z","session_id":"sess-1","spec":"alpha","payload":{"scope":"full"}}"#,
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:30:00.000Z","session_id":"sess-1","spec":null,"payload":{"tool":"Edit","target":{"file_path":"src/live.rs"}}}"#,
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:31:00.000Z","session_id":"sess-1","spec":null,"payload":{"tool":"Read","target":{"file_path":"src/other.rs"}}}"#,
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:32:00.000Z","session_id":"sess-1","spec":"alpha","payload":{"tool":"Edit","target":{"file_path":"src/live.rs"}}}"#,
        ]
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();

        let counts = attributed_spec_counts_from(&records);
        let alpha = counts.get("alpha").expect("alpha bucket");
        assert_eq!(alpha.tools_used, 3, "all attributed tool.use events");
        assert_eq!(alpha.files_touched, 2, "src/live.rs + src/other.rs distinct");
        assert_eq!(alpha.events, 4, "scope binding + three tool.use");
        assert_eq!(alpha.last_event_at.as_deref(), Some("2026-05-27T09:32:00.000Z"));

        // A session event BEFORE its first binding stays unattributed.
        let pre: Vec<Value> = [
            r#"{"event":"pipeline.scope","kind":"pipeline","ts":"2026-05-27T09:00:00.000Z","session_id":"sess-2","spec":"beta","payload":{}}"#,
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T08:00:00.000Z","session_id":"sess-2","spec":null,"payload":{"tool":"Read"}}"#,
        ]
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
        let pre_counts = attributed_spec_counts_from(&pre);
        // beta only sees its own binding event; the pre-binding tool.use is dropped.
        assert_eq!(pre_counts.get("beta").map(|c| c.tools_used), Some(0));
        assert_eq!(pre_counts.get("beta").map(|c| c.events), Some(1));
    }

    // ── DEFECT 5a: tool.result → tool.use pairing ────────────────────────────

    fn ev_line(value: serde_json::Value) -> Value {
        value
    }

    #[test]
    fn pairing_matches_by_tool_use_id() {
        // When the `tool.use` carries a `tool_use_id` the result echoes, the
        // pairing is exact regardless of ordering / interleaving.
        let events = vec![
            ev_line(serde_json::json!({
                "event": "tool.result", "ts_ms": 200,
                "payload": { "tool_use_id": "tu-1", "tool": "Bash", "stdout_excerpt": "hi" }
            })),
            ev_line(serde_json::json!({
                "event": "tool.use", "ts_ms": 100,
                "payload": { "tool": "Bash", "tool_use_id": "tu-1", "target": { "command": "echo hi" } }
            })),
        ];
        let mut pairing = ResultPairing::build(&events);
        let use_ev = &events[1];
        let result = pairing.pair_for(use_ev, "Bash").expect("paired by id");
        assert_eq!(result.get("stdout_excerpt").and_then(Value::as_str), Some("hi"));
    }

    #[test]
    fn pairing_id_match_wins_over_chronological() {
        // Parallel-agent interleave: the use heartbeat now carries a
        // `tool_use_id`, and its result is NOT the chronologically-nearest one.
        // Tier-1 must pick the id-matched result, not the earlier-by-time slot
        // that a pure chronological match (tier-2) would have grabbed. This is
        // the precise misattribution the heartbeat `tool_use_id` propagation
        // fixes.
        let events = vec![
            ev_line(serde_json::json!({
                // Other agent's result lands first chronologically.
                "event": "tool.result", "ts_ms": 150,
                "payload": { "tool_use_id": "other", "tool": "Bash", "stdout_excerpt": "WRONG" }
            })),
            ev_line(serde_json::json!({
                // This use's own result lands later.
                "event": "tool.result", "ts_ms": 250,
                "payload": { "tool_use_id": "mine", "tool": "Bash", "stdout_excerpt": "RIGHT" }
            })),
            ev_line(serde_json::json!({
                "event": "tool.use", "ts_ms": 100,
                "payload": { "tool": "Bash", "tool_use_id": "mine", "target": { "command": "echo x" } }
            })),
        ];
        let mut pairing = ResultPairing::build(&events);
        let use_ev = &events[2];
        let result = pairing.pair_for(use_ev, "Bash").expect("paired by id");
        assert_eq!(
            result.get("stdout_excerpt").and_then(Value::as_str),
            Some("RIGHT"),
            "tier-1 id match must beat the chronologically-earlier result"
        );
    }

    #[test]
    fn pairing_falls_back_to_chronological_order() {
        // The real-world case: `tool.use` heartbeats carry NO `tool_use_id`,
        // while each `tool.result` DOES. Tier-1 can't fire (the use has no id),
        // so pairing must go by timestamp order + tool name. Two Bash calls must
        // not alias — the first use gets the first result, the second the second.
        let events = vec![
            ev_line(serde_json::json!({
                "event": "tool.use", "ts_ms": 100,
                "payload": { "tool": "Bash", "target": { "command": "first" } }
            })),
            ev_line(serde_json::json!({
                "event": "tool.result", "ts_ms": 101,
                "payload": { "tool_use_id": "x1", "tool": "Bash", "stdout_excerpt": "out-first" }
            })),
            ev_line(serde_json::json!({
                "event": "tool.use", "ts_ms": 200,
                "payload": { "tool": "Bash", "target": { "command": "second" } }
            })),
            ev_line(serde_json::json!({
                "event": "tool.result", "ts_ms": 201,
                "payload": { "tool_use_id": "x2", "tool": "Bash", "stdout_excerpt": "out-second" }
            })),
        ];
        let mut pairing = ResultPairing::build(&events);
        let first = pairing.pair_for(&events[0], "Bash").expect("first paired");
        assert_eq!(first.get("stdout_excerpt").and_then(Value::as_str), Some("out-first"));
        let second = pairing.pair_for(&events[2], "Bash").expect("second paired");
        assert_eq!(second.get("stdout_excerpt").and_then(Value::as_str), Some("out-second"));
    }

    #[test]
    fn pairing_returns_none_when_no_result() {
        let events = vec![ev_line(serde_json::json!({
            "event": "tool.use", "ts_ms": 100,
            "payload": { "tool": "Read", "target": { "file_path": "/x" } }
        }))];
        let mut pairing = ResultPairing::build(&events);
        assert!(pairing.pair_for(&events[0], "Read").is_none());
    }

    #[test]
    fn trace_splices_result_payload_onto_tool_node() {
        // End-to-end: a spec dir with a paired tool.use + tool.result must
        // surface `payload.result.content_excerpt` on the tool node so the
        // frontend stops rendering "tool_result pendente".
        let tmp = TempDir::new().unwrap();
        let lines = format!(
            "{}\n{}\n",
            r##"{"event":"tool.use","kind":"tool","ts":"2026-06-05T10:00:00.000Z","ts_ms":1000,"session_id":"s","spec":"alpha","wave":1,"actor":"metrics-tracker","payload":{"tool":"Read","target":{"file_path":"/tmp/r.md"}}}"##,
            r##"{"event":"tool.result","kind":"tool","ts":"2026-06-05T10:00:00.100Z","ts_ms":1001,"session_id":"s","spec":"alpha","actor":"tool_result","payload":{"tool_use_id":"tu","tool":"Read","content_excerpt":"# hi"}}"##,
        );
        write_event(tmp.path(), "alpha", "events.ndjson", &lines);

        let tree = dashboard_spec_trace_impl(
            tmp.path().to_string_lossy().to_string(),
            "alpha".to_string(),
        );
        // Walk to the first tool node and assert the spliced result.
        let found = find_tool_node(&tree).expect("a tool node exists");
        let result = found
            .get("payload")
            .and_then(|p| p.get("result"))
            .expect("result spliced onto payload");
        assert_eq!(
            result.get("content_excerpt").and_then(Value::as_str),
            Some("# hi")
        );
    }

    #[test]
    fn encode_cwd_for_projects_rewrites_only_separators() {
        // The four characters `:` `\` `/` `.` become `-`; existing hyphens and
        // letters survive. Mirrors Claude Code's `~/.claude/projects/<dir>` name.
        assert_eq!(encode_cwd_for_projects(r"C:\Atiz\sialia"), "C--Atiz-sialia");
        assert_eq!(
            encode_cwd_for_projects(r"C:\Atiz\mustard\.claude\worktrees\x"),
            "C--Atiz-mustard--claude-worktrees-x"
        );
    }

    #[test]
    fn transcript_motivations_maps_narration_to_tool_and_resets_on_user() {
        // A JSONL with: assistant text → tool_use (captures it) → another
        // assistant text → tool_use (captures the NEW text), then a real user
        // record clears narration so a following tool_use gets nothing.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("session.jsonl");
        let lines = [
            // Turn 1: narration "first" motivates tu-A.
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"first reason"}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu-A","name":"Bash","input":{}}]}}"#,
            // Still turn 1: narration "second" replaces, motivates tu-B.
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"second reason"}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu-B","name":"Read","input":{}}]}}"#,
            // A real user turn resets the narration.
            r#"{"type":"user","message":{"role":"user","content":"do more"}}"#,
            // tu-C has no preceding assistant text → no motivation recorded.
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu-C","name":"Grep","input":{}}]}}"#,
        ];
        std::fs::write(&path, lines.join("\n")).unwrap();

        let map = transcript_motivations(&path);
        assert_eq!(map.get("tu-A").map(String::as_str), Some("first reason"));
        assert_eq!(map.get("tu-B").map(String::as_str), Some("second reason"));
        assert!(
            !map.contains_key("tu-C"),
            "a user turn must reset narration; tu-C inherits nothing"
        );
    }

    #[test]
    fn transcript_motivations_concatenates_consecutive_text_blocks() {
        // Two text blocks before a tool_use (a single assistant turn split into
        // multiple records) join with a newline.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("session.jsonl");
        let lines = [
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"line one"}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"private"},{"type":"text","text":"line two"}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu-X","name":"Bash","input":{}}]}}"#,
        ];
        std::fs::write(&path, lines.join("\n")).unwrap();

        let map = transcript_motivations(&path);
        assert_eq!(
            map.get("tu-X").map(String::as_str),
            Some("line one\nline two"),
            "consecutive text blocks join with newline; thinking is ignored"
        );
    }

    #[test]
    fn transcript_motivations_consecutive_tools_share_one_narration() {
        // The real transcript pattern: one rationale text, then several tool_use
        // blocks with no text between them — all share that narration.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("session.jsonl");
        let lines = [
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"shared why"}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu-1","name":"Bash","input":{}}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu-2","name":"Bash","input":{}}]}}"#,
        ];
        std::fs::write(&path, lines.join("\n")).unwrap();

        let map = transcript_motivations(&path);
        assert_eq!(map.get("tu-1").map(String::as_str), Some("shared why"));
        assert_eq!(
            map.get("tu-2").map(String::as_str),
            Some("shared why"),
            "a second tool with no intervening text shares the rationale"
        );
    }

    #[test]
    fn transcript_motivations_fail_open_on_missing_file() {
        let tmp = TempDir::new().unwrap();
        let map = transcript_motivations(&tmp.path().join("does-not-exist.jsonl"));
        assert!(map.is_empty(), "a missing transcript yields an empty map");
    }

    #[test]
    fn build_trace_tree_splices_motivation_onto_matching_tool_node() {
        // A tool.use whose payload.tool_use_id is in the motivations map gets
        // `payload.motivation`; a non-matching id stays bare.
        let events = vec![ev_line(serde_json::json!({
            "event": "tool.use", "ts": "2026-06-20T05:07:09.381Z", "ts_ms": 1000,
            "session_id": "s",
            "payload": { "tool": "Bash", "tool_use_id": "toolu_match", "target": { "command": "echo hi" } }
        }))];
        let mut motivations = HashMap::new();
        motivations.insert("toolu_match".to_string(), "porque sim".to_string());

        let tree = build_trace_tree(
            &events,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "session",
            "s",
            &motivations,
        );
        let node = find_tool_node(&tree).expect("a tool node exists");
        assert_eq!(
            node.get("payload")
                .and_then(|p| p.get("motivation"))
                .and_then(Value::as_str),
            Some("porque sim"),
            "the motivation is spliced onto the matching tool node's payload"
        );
    }

    /// Depth-first search for the first `kind == "tool"` node in a trace tree.
    fn find_tool_node(node: &Value) -> Option<Value> {
        if node.get("kind").and_then(Value::as_str) == Some("tool") {
            return Some(node.clone());
        }
        for child in node.get("children").and_then(Value::as_array)? {
            if let Some(hit) = find_tool_node(child) {
                return Some(hit);
            }
        }
        None
    }

    /// Depth-first search for the first `kind == "agent"` node in a trace tree.
    fn find_agent_node(node: &Value) -> Option<Value> {
        if node.get("kind").and_then(Value::as_str) == Some("agent") {
            return Some(node.clone());
        }
        for child in node.get("children").and_then(Value::as_array)? {
            if let Some(hit) = find_agent_node(child) {
                return Some(hit);
            }
        }
        None
    }

    #[test]
    fn build_trace_tree_injects_skill_invoked_as_leading_prompt_node() {
        // A `skill.invoked` (e.g. `/feature`) carries the request as `payload.args`.
        // It must surface as a leading `kind:"prompt"` node — the retroactive path
        // for OLD sessions that predate `user.prompt`. The agent node follows it.
        let events = vec![
            ev_line(serde_json::json!({
                "event": "skill.invoked", "ts": "2026-06-18T08:00:00.000Z", "ts_ms": 1000,
                "session_id": "s",
                "payload": { "skill": "mustard:feature", "args": "redesenhar o datatable de Contratos" }
            })),
            start_line("s", 2000, "Explore", "Mapeia estrutura"),
            use_line("s", 2100, "Read"),
            stop_line("s", 2200),
        ];
        let tree = build_trace_tree(
            &events,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "session",
            "s",
            &HashMap::new(),
        );
        let children = tree["children"].as_array().expect("children array");
        // The prompt node leads, carrying the args as both label and payload.prompt.
        assert_eq!(children[0]["kind"], "prompt", "skill.invoked → leading prompt node");
        assert_eq!(
            children[0]["label"].as_str().unwrap_or(""),
            "redesenhar o datatable de Contratos",
            "the skill args become the prompt label"
        );
        assert_eq!(
            children[0]["payload"]["prompt"].as_str().unwrap_or(""),
            "redesenhar o datatable de Contratos",
        );
        assert_eq!(
            children[0]["payload"]["skill"].as_str().unwrap_or(""),
            "mustard:feature",
        );
        // The agent node appears AFTER the prompt.
        assert!(
            children.iter().skip(1).any(|c| c["kind"] == "agent"),
            "agent node follows the skill prompt node"
        );
    }

    #[test]
    fn build_trace_tree_skips_empty_args_skill_invoked() {
        // A bare `/status` (empty args) carries no request — inject nothing.
        let events = vec![ev_line(serde_json::json!({
            "event": "skill.invoked", "ts": "2026-06-18T08:00:00.000Z", "ts_ms": 1000,
            "session_id": "s",
            "payload": { "skill": "mustard:status", "args": "" }
        }))];
        let tree = build_trace_tree(
            &events,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "session",
            "s",
            &HashMap::new(),
        );
        assert!(
            tree["children"].as_array().expect("children array").is_empty(),
            "empty-args skill.invoked injects no prompt node"
        );
    }

    #[test]
    fn build_trace_tree_splices_motivation_onto_agent_node_via_spawn_id() {
        // An `agent.start` carrying a `tool_use_id` whose narration is in the
        // motivations map → the AGENT node gets `payload.motivation` (+ tool_use_id),
        // so the spawning "why" is visible on the collapsed agent header.
        let events = vec![
            serde_json::json!({
                "event": "agent.start", "kind": "agent", "ts_ms": 1000u64,
                "session_id": "s",
                "payload": {
                    "description": "Mapeia estrutura", "subagentType": "Explore",
                    "tool_use_id": "toolu_spawn"
                }
            }),
            use_line("s", 1100, "Read"),
            stop_line("s", 1200),
        ];
        let mut motivations = HashMap::new();
        motivations.insert(
            "toolu_spawn".to_string(),
            "preciso mapear antes de mexer".to_string(),
        );
        let tree = build_trace_tree(
            &events,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "session",
            "s",
            &motivations,
        );
        let agent = find_agent_node(&tree).expect("an agent node exists");
        assert_eq!(
            agent["payload"]["motivation"].as_str().unwrap_or(""),
            "preciso mapear antes de mexer",
            "the spawn motivation is spliced onto the agent node"
        );
        assert_eq!(
            agent["payload"]["tool_use_id"].as_str().unwrap_or(""),
            "toolu_spawn",
        );
    }

    #[test]
    fn build_trace_tree_agent_payload_null_without_motivation() {
        // No matching narration → the agent node keeps `payload: null` (today's
        // shape) — fail-open.
        let events = vec![
            start_line("s", 1000, "Explore", "Mapeia estrutura"),
            use_line("s", 1100, "Read"),
            stop_line("s", 1200),
        ];
        let tree = build_trace_tree(
            &events,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "session",
            "s",
            &HashMap::new(),
        );
        let agent = find_agent_node(&tree).expect("an agent node exists");
        assert!(agent["payload"].is_null(), "no narration → payload stays null");
    }

    #[test]
    fn events_cache_hit_reuses_arc_then_invalidate_reparses() {
        // Each test gets a fresh TempDir, so its path is a unique cache key —
        // no contamination from the process-global `EVENTS_CACHE` across tests.
        let tmp = TempDir::new().unwrap();
        let line = r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:00:00.000Z","spec":"a","payload":{"tool":"Read"}}"#;
        write_event(tmp.path(), "a", "events.ndjson", &format!("{line}\n"));

        // First call parses + caches; the slice has the one event.
        let first = walk_ndjson_events_cached(tmp.path());
        assert_eq!(first.len(), 1);

        // Second call is a hit: the SAME Arc allocation, not a re-parse.
        let second = walk_ndjson_events_cached(tmp.path());
        assert!(
            std::sync::Arc::ptr_eq(&first, &second),
            "a cache hit must hand back the same Arc, not re-parse"
        );

        // A fresh event lands on disk but the cache still serves the stale slice.
        let line2 = r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:01:00.000Z","spec":"a","payload":{"tool":"Edit"}}"#;
        write_event(tmp.path(), "a", "events2.ndjson", &format!("{line2}\n"));
        let stale = walk_ndjson_events_cached(tmp.path());
        assert_eq!(stale.len(), 1, "without invalidation the cache stays warm");
        assert!(std::sync::Arc::ptr_eq(&first, &stale));

        // Invalidate → the next call re-parses (new Arc) and sees BOTH events.
        invalidate_events_cache(&tmp.path().to_string_lossy());
        let fresh = walk_ndjson_events_cached(tmp.path());
        assert!(
            !std::sync::Arc::ptr_eq(&first, &fresh),
            "after invalidation the slice must be a fresh allocation"
        );
        assert_eq!(fresh.len(), 2, "the re-parse must pick up the new event");
    }

    #[test]
    fn events_cache_warm_hit_reads_no_shard_and_dirty_path_rereads_only_it() {
        // The incremental contract (wave-1 task 6): with a warm cache the
        // second call performs ZERO shard reads; after touching ONE shard,
        // exactly that shard is re-read — asserted via the parse counter.
        let tmp = TempDir::new().unwrap();
        let line1 = r#"{"event":"tool.use","kind":"tool","ts":"2026-06-10T09:00:00.000Z","spec":"a","payload":{"tool":"Read"}}"#;
        let line2 = r#"{"event":"tool.use","kind":"tool","ts":"2026-06-10T09:01:00.000Z","spec":"a","payload":{"tool":"Edit"}}"#;
        write_event(tmp.path(), "a", "one.ndjson", &format!("{line1}\n"));
        write_event(tmp.path(), "a", "two.ndjson", &format!("{line2}\n"));

        // Cold: both shards parsed once.
        let first = walk_ndjson_events_cached(tmp.path());
        assert_eq!(first.len(), 2);
        let cold = events_cache_parsed_files(tmp.path());
        assert_eq!(cold, 2, "cold start parses each shard exactly once");

        // Warm: same Arc, zero additional shard reads.
        let second = walk_ndjson_events_cached(tmp.path());
        assert!(std::sync::Arc::ptr_eq(&first, &second));
        assert_eq!(
            events_cache_parsed_files(tmp.path()),
            cold,
            "a warm hit must not touch the disk"
        );

        // Append to ONE shard + watcher-style per-path invalidation.
        let one_path = tmp
            .path()
            .join(".claude")
            .join("spec")
            .join("a")
            .join(".events")
            .join("one.ndjson");
        let line3 = r#"{"event":"tool.use","kind":"tool","ts":"2026-06-10T09:02:00.000Z","spec":"a","payload":{"tool":"Bash"}}"#;
        std::fs::write(&one_path, format!("{line1}\n{line3}\n")).unwrap();
        invalidate_events_cache_path(&tmp.path().to_string_lossy(), &one_path);

        let fresh = walk_ndjson_events_cached(tmp.path());
        assert_eq!(fresh.len(), 3, "the appended event must surface");
        assert_eq!(
            events_cache_parsed_files(tmp.path()),
            cold + 1,
            "only the touched shard may be re-read"
        );
    }

    #[test]
    fn full_invalidation_sweeps_but_reparses_only_changed_fingerprints() {
        // The generic sweep re-enumerates + re-stats, but an unchanged shard
        // keeps its parsed chunk — only the NEW shard costs a parse.
        let tmp = TempDir::new().unwrap();
        let line = r#"{"event":"tool.use","kind":"tool","ts":"2026-06-10T09:00:00.000Z","spec":"a","payload":{"tool":"Read"}}"#;
        write_event(tmp.path(), "a", "one.ndjson", &format!("{line}\n"));
        assert_eq!(walk_ndjson_events_cached(tmp.path()).len(), 1);
        let cold = events_cache_parsed_files(tmp.path());

        write_event(tmp.path(), "a", "new.ndjson", &format!("{line}\n"));
        invalidate_events_cache(&tmp.path().to_string_lossy());
        assert_eq!(walk_ndjson_events_cached(tmp.path()).len(), 2);
        assert_eq!(
            events_cache_parsed_files(tmp.path()),
            cold + 1,
            "sweep must re-parse only the new shard, not the unchanged one"
        );
    }

    #[test]
    fn harness_cache_converts_once_and_tracks_value_invalidation() {
        let tmp = TempDir::new().unwrap();
        let line = r#"{"event":"pipeline.phase","kind":"pipeline","ts":"2026-06-10T09:00:00.000Z","spec":"a","payload":{"to":"EXECUTE"}}"#;
        write_event(tmp.path(), "a", "one.ndjson", &format!("{line}\n"));

        let h1 = workspace_harness_events_cached(tmp.path());
        assert_eq!(h1.len(), 1);
        assert_eq!(h1[0].event, "pipeline.phase");
        assert_eq!(h1[0].spec.as_deref(), Some("a"));

        // Warm: the converted slice is shared, not rebuilt.
        let h2 = workspace_harness_events_cached(tmp.path());
        assert!(std::sync::Arc::ptr_eq(&h1, &h2), "warm harness hit shares the Arc");

        // A new shard invalidates BOTH the value snapshot and the converted slice.
        let two_path = tmp
            .path()
            .join(".claude")
            .join("spec")
            .join("a")
            .join(".events")
            .join("two.ndjson");
        let line2 = r#"{"event":"qa.result","kind":"qa","ts":"2026-06-10T09:05:00.000Z","spec":"a","payload":{"criteria":[]}}"#;
        std::fs::write(&two_path, format!("{line2}\n")).unwrap();
        invalidate_events_cache_path(&tmp.path().to_string_lossy(), &two_path);

        let h3 = workspace_harness_events_cached(tmp.path());
        assert_eq!(h3.len(), 2, "the converted slice must follow the value snapshot");
        assert!(!std::sync::Arc::ptr_eq(&h1, &h3));
    }

    #[test]
    fn non_ndjson_path_invalidation_is_ignored() {
        // Only parsed `.ndjson` shards feed the snapshot — a write to any
        // other file must not dirty the cache.
        let tmp = TempDir::new().unwrap();
        let line = r#"{"event":"tool.use","kind":"tool","ts":"2026-06-10T09:00:00.000Z","spec":"a","payload":{"tool":"Read"}}"#;
        write_event(tmp.path(), "a", "one.ndjson", &format!("{line}\n"));
        let first = walk_ndjson_events_cached(tmp.path());
        invalidate_events_cache_path(
            &tmp.path().to_string_lossy(),
            &tmp.path().join(".claude").join(".harness").join("notes.txt"),
        );
        let second = walk_ndjson_events_cached(tmp.path());
        assert!(
            std::sync::Arc::ptr_eq(&first, &second),
            "a non-ndjson write must not invalidate the snapshot"
        );
    }
}
