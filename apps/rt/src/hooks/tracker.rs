//! `tracker` — the consolidated agent/tool telemetry + counter module.
//!
//! ## Scope (b3 Wave 3, Subagent/Task family)
//!
//! This module consolidates five JavaScript hooks. Each is a distinct
//! *concern* kept as its own internal section — consolidation regroups, it
//! does not merge logic:
//!
//! - `tool-use-counter.js` — a **`Check`** that caps tool uses per active
//!   Explore subagent (deny at 15, warn at 12). Owns the per-agent counter
//!   files under `.claude/.agent-state/*.counter.json`.
//! - `main-context-counter.js` — a **`Check`** enforcing L0 (Universal
//!   Delegation) on the orchestrator: counts main-context work tools between
//!   Task dispatches, denies (strict mode) past `DENY_AT`.
//! - `subagent-tracker.js` — an **`Observer`**: emits `agent.start` /
//!   `agent.stop` telemetry. (The explorer-dedup `deny` of the JS hook is a
//!   Wave-4 concern — see the `CONCERN` note in [`SubagentTracker`].)
//! - `metrics-tracker.js` — an **`Observer`**: emits a `tool.use` heartbeat.
//! - `skill-usage-tracker.js` — an **`Observer`**: emits a `skill.invoked`
//!   event.
//!
//! ## Budget↔tracker boundary (b3 spec § Arquitetura overlap)
//!
//! The spec table lists "tool-use / main-context caps" under **both** `budget`
//! and `tracker`. The counting/cap logic lives **here**, in `tracker` — the
//! two hooks that own it (`tool-use-counter.js`, `main-context-counter.js`)
//! were grouped into this Subagent/Task family, and the counter *state files*
//! are agent-lifecycle state, not prompt size. [`crate::hooks::budget`] owns
//! only prompt-size and return-size; it has no counting logic. There is no
//! duplication.
//!
//! ## Modules and contracts
//!
//! [`ToolUseCounter`] and [`MainContextCounter`] are `Check`s. The three
//! telemetry concerns are folded into [`SubagentTracker`] /
//! [`MetricsTracker`] / [`SkillUsageTracker`], all `Observer`s. The registry
//! wires each to its own `(event, tool)` pairs.

use crate::run::current_spec;
use crate::util::now_iso8601;
use mustard_core::economy::estimator;
use mustard_core::economy::writer;
use mustard_core::economy::{ApiCostFrame, SpanRecord};
use mustard_core::error::Error;
use mustard_core::fs;
use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use rusqlite::Connection;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Resolve the harness SQLite path the same way [`SqliteEventStore::for_project`]
/// does internally — env override `MUSTARD_DB_PATH` wins, else
/// `{project_dir}/.claude/.harness/mustard.db`. Mirrored privately here to
/// keep the `mustard-core` surface unchanged for W2.
fn economy_db_path(project_dir: &str) -> PathBuf {
    if let Ok(value) = std::env::var("MUSTARD_DB_PATH") {
        if !value.trim().is_empty() {
            return PathBuf::from(value);
        }
    }
    Path::new(project_dir)
        .join(".claude")
        .join(".harness")
        .join("mustard.db")
}

/// Open a raw [`Connection`] to the harness DB, applying schema/migrations
/// via [`SqliteEventStore::for_project`] first. Returns `None` on failure;
/// tracker telemetry is best-effort.
fn open_economy_conn(project_dir: &str) -> Option<Connection> {
    let _ = SqliteEventStore::for_project(project_dir).ok()?;
    Connection::open(economy_db_path(project_dir)).ok()
}

/// Finalise an agent's Task dispatch as one `spans` row + one `api_cost`
/// frame, derived from the PostToolUse(Task) payload. `model` is the model id
/// the dispatch ran under (may be empty — pricing falls through to `(0, 0)`).
/// `input_bytes` and `output_bytes` are the byte sizes of `tool_input` /
/// `tool_response`; `input_tokens` / `output_tokens` come from the API usage
/// payload when present, else estimated from the byte size. Fail-open.
fn record_task_span(
    project_dir: &str,
    session_id: Option<&str>,
    span_id: String,
    model: &str,
    spec: Option<String>,
    input_text: &str,
    output_text: &str,
    api_input_tokens: Option<i64>,
    api_output_tokens: Option<i64>,
    is_error: bool,
) {
    let Some(conn) = open_economy_conn(project_dir) else {
        return;
    };
    let input_tokens = api_input_tokens
        .unwrap_or_else(|| i64::from(estimator::estimate_input_tokens(input_text, model)));
    let output_tokens = api_output_tokens
        .unwrap_or_else(|| i64::from(estimator::estimate_output_tokens(output_text, model)));
    let (in_micros_per_m, out_micros_per_m) =
        estimator::model_pricing_usd_micros_per_million(model);
    // Saturating arithmetic — keeps the writer safe even when an adapter
    // ships an absurd token count (`i64::MAX`).
    let cost_usd_micros = in_micros_per_m
        .saturating_mul(input_tokens)
        .saturating_add(out_micros_per_m.saturating_mul(output_tokens))
        / 1_000_000;
    let ts = now_iso8601();
    let rec = SpanRecord {
        ts: ts.clone(),
        session_id: session_id.map(str::to_string),
        span_id,
        model: if model.is_empty() {
            None
        } else {
            Some(model.to_string())
        },
        spec,
        phase: None,
        input_tokens: Some(input_tokens),
        output_tokens: Some(output_tokens),
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
        cost_usd_micros: Some(cost_usd_micros),
        is_error,
        extra: Map::new(),
    };
    // Writer side: one `spans` insert (the internal estimator path), then a
    // second alias call to mark provenance — they share the same row via
    // `INSERT OR REPLACE` on `span_id`.
    let _ = writer::record_span(&conn, rec.clone());
    let _ = writer::record_api_cost(&conn, rec as ApiCostFrame);
}

// ===========================================================================
// Shared helpers
// ===========================================================================

/// Now, as milliseconds since the Unix epoch — for counter `createdAt` staleness.
fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
}

/// Resolve the project dir for an invocation: the harness `cwd`, else `.`.
/// Mirrors the JS `data.cwd || process.cwd()`.
fn project_dir(input: &HookInput) -> String {
    match input.cwd.as_deref() {
        Some(cwd) if !cwd.is_empty() => cwd.to_string(),
        _ => ".".to_string(),
    }
}

/// Emit one harness event, best-effort. Telemetry is never load-bearing.
fn emit_event(project_dir: &str, hook_id: &str, event: &str, payload: Value) {
    let harness_event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: "unknown".to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some(hook_id.to_string()),
            actor_type: None,
        },
        event: event.to_string(),
        payload,
        spec: current_spec(project_dir),
    };
    let _ = SqliteEventStore::for_project(project_dir)
        .and_then(|store| store.append(&harness_event));
}

// ===========================================================================
// tool-use-counter — Check on (.*) PreToolUse + Subagent lifecycle
// ===========================================================================

/// Hard tool-use cap and warn threshold for a generic enforced agent type.
const HARD_LIMIT: u32 = 20;
const WARN_THRESHOLD: u32 = 15;
/// Explore agents get a tighter budget — deny at 15, warn at 12.
const EXPLORE_LIMIT: u32 = 15;
const EXPLORE_WARN: u32 = 12;
/// A counter file older than this is stale and is deleted on read.
const COUNTER_STALE_MS: u128 = 10 * 60 * 1000;

/// `tool-use-counter`: caps tool uses per active Explore subagent.
///
/// This is a `Check` because [`Self::handle_pre_tool_use`] can return a
/// blocking `Deny`. The other three events it handles (`SubagentStart`,
/// `SubagentStop`, `SessionStart`) are pure file-state side effects that
/// resolve to `Allow`.
pub struct ToolUseCounter;

/// One on-disk tool-use counter, `<agent_id>.counter.json`.
///
/// The `createdAt` field is *not* modelled here: it is carried verbatim as an
/// ISO string by [`Counter::to_json`], and staleness is computed inline from
/// the parsed string in [`ToolUseCounter::handle_pre_tool_use`].
#[derive(Debug)]
struct Counter {
    /// The enforced agent type, e.g. `"Explore"`.
    agent_type: String,
    /// Hard deny limit.
    limit: u32,
    /// Warn threshold.
    warn_at: u32,
    /// Current tool-use count.
    count: u32,
}

impl Counter {
    /// Serialise to the JSON shape `tool-use-counter.js` writes. The `createdAt`
    /// field round-trips as an ISO string only when the counter was created in
    /// this process; on disk a counter created elsewhere keeps its own string.
    /// To stay faithful, the counter stores `created_at_iso` verbatim.
    fn to_json(&self, created_at_iso: &str) -> Value {
        json!({
            "type": self.agent_type,
            "limit": self.limit,
            "warnAt": self.warn_at,
            "count": self.count,
            "createdAt": created_at_iso,
        })
    }
}

/// Parse an ISO-8601 timestamp into epoch millis. Returns `0` on any failure —
/// matching the JS `new Date(... || 0).getTime()` fallback (where an absent
/// `createdAt` yields the epoch). Conservative: only the `YYYY-MM-DDThh:mm:ss`
/// prefix is parsed; sub-second precision is ignored (does not affect the
/// 10-minute staleness window).
fn parse_iso_millis(iso: &str) -> u128 {
    // Expect at least `YYYY-MM-DDThh:mm:ss`.
    let bytes = iso.as_bytes();
    if bytes.len() < 19 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return 0;
    }
    let num = |s: &str| -> Option<i64> { s.parse().ok() };
    let (Some(year), Some(month), Some(day), Some(hh), Some(mm), Some(ss)) = (
        num(&iso[0..4]),
        num(&iso[5..7]),
        num(&iso[8..10]),
        num(&iso[11..13]),
        num(&iso[14..16]),
        num(&iso[17..19]),
    ) else {
        return 0;
    };
    // Days since 1970-01-01 via Howard Hinnant's `days_from_civil`.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + hh * 3600 + mm * 60 + ss;
    if secs < 0 {
        0
    } else {
        u128::try_from(secs).unwrap_or(0) * 1000
    }
}

impl ToolUseCounter {
    /// The `.claude/.agent-state` directory for a project.
    fn state_dir(project_dir: &str) -> std::path::PathBuf {
        Path::new(project_dir).join(".claude").join(".agent-state")
    }

    /// `SubagentStart`: create a counter file for an enforced agent type
    /// (`Explore`). Returns the budget-reminder advisory, or `Allow` for a
    /// non-enforced type.
    fn handle_start(input: &HookInput, project_dir: &str) -> Verdict {
        let agent_id = input
            .raw
            .get("agent_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let agent_type = input
            .raw
            .get("agent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        // Only `Explore` is an enforced type (`ENFORCED_TYPES`).
        if agent_type != "Explore" {
            return Verdict::Allow;
        }
        let agent_id = if agent_id.is_empty() {
            // The JS uses `unknown-${Date.now()}`; a deterministic-enough id.
            &format!("unknown-{}", now_millis())
        } else {
            agent_id
        };

        let dir = Self::state_dir(project_dir);
        let _ = fs::create_dir_all(&dir);

        let limit = EXPLORE_LIMIT; // agent_type == "Explore"
        let warn_at = EXPLORE_WARN;
        let counter = Counter {
            agent_type: agent_type.to_string(),
            limit,
            warn_at,
            count: 0,
        };
        let iso = now_iso8601();
        let file = dir.join(format!("{agent_id}.counter.json"));
        let _ = fs::write_atomic(
            &file,
            serde_json::to_string_pretty(&counter.to_json(&iso)).unwrap_or_default().as_bytes(),
        );

        Verdict::Inject {
            context: format!(
                "[Tool Budget] This agent has a {limit}-tool-use budget. \
                 Use Grep over Read where possible. Return findings as soon as \
                 root cause is clear."
            ),
        }
    }

    /// `SubagentStop`: remove the stopped agent's counter file.
    fn handle_stop(input: &HookInput, project_dir: &str) -> Verdict {
        let agent_id = input
            .raw
            .get("agent_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if !agent_id.is_empty() {
            let file = Self::state_dir(project_dir).join(format!("{agent_id}.counter.json"));
            let _ = fs::remove_file(&file);
        }
        Verdict::Allow
    }

    /// `SessionStart`: delete every `*.counter.json` — a fresh session starts
    /// with clean counters.
    fn handle_session_start(project_dir: &str) -> Verdict {
        let dir = Self::state_dir(project_dir);
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries {
                if entry.file_name.ends_with(".counter.json") {
                    let _ = fs::remove_file(&entry.path);
                }
            }
        }
        Verdict::Allow
    }

    /// `PreToolUse`: increment every active counter, enforce the limits.
    ///
    /// A counter that reaches its `limit` denies (deny dominates). The first
    /// counter to hit `warn_at` warns. A stale counter is deleted and skipped.
    fn handle_pre_tool_use(project_dir: &str) -> Verdict {
        let dir = Self::state_dir(project_dir);
        let Ok(entries) = fs::read_dir(&dir) else {
            return Verdict::Allow; // no state dir → no active Explore agents
        };
        let counter_files: Vec<std::path::PathBuf> = entries
            .into_iter()
            .filter(|e| e.file_name.ends_with(".counter.json"))
            .map(|e| e.path)
            .collect();
        if counter_files.is_empty() {
            return Verdict::Allow;
        }

        let now = now_millis();
        let mut deny: Option<Verdict> = None;
        let mut warn: Option<Verdict> = None;

        for file in counter_files {
            let Ok(text) = fs::read_to_string(&file) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(&text) else {
                continue; // corrupt counter — skip
            };
            let created_at_iso = value
                .get("createdAt")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let created_at_ms = parse_iso_millis(created_at_iso);

            // Staleness: delete and skip.
            if now.saturating_sub(created_at_ms) > COUNTER_STALE_MS {
                let _ = fs::remove_file(&file);
                continue;
            }

            let count = value
                .get("count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32
                + 1;
            let limit = value
                .get("limit")
                .and_then(serde_json::Value::as_u64)
                .map_or(HARD_LIMIT, |n| n as u32);
            let warn_at = value
                .get("warnAt")
                .and_then(serde_json::Value::as_u64)
                .map_or(WARN_THRESHOLD, |n| n as u32);
            let agent_type = value
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            // Persist the incremented count, preserving the original
            // `createdAt` string.
            let updated = Counter {
                agent_type,
                limit,
                warn_at,
                count,
            };
            let _ = fs::write_atomic(
                &file,
                serde_json::to_string_pretty(&updated.to_json(created_at_iso))
                    .unwrap_or_default()
                    .as_bytes(),
            );

            if count >= limit {
                deny = Some(Verdict::Deny {
                    reason: format!(
                        "[Tool Budget] Explore agent reached {limit} tool uses \
                         (limit). Wrap up your findings."
                    ),
                });
                // Deny dominates — stop scanning the remaining counters.
                break;
            }
            if count == warn_at && warn.is_none() {
                warn = Some(Verdict::Warn {
                    message: format!(
                        "[Tool Budget] {count}/{limit} tool uses. Begin wrapping \
                         up — return findings after completing current \
                         investigation."
                    ),
                });
            }
        }

        deny.or(warn).unwrap_or(Verdict::Allow)
    }
}

impl Check for ToolUseCounter {
    /// Dispatch by trigger to the matching handler. The JS hook runs on four
    /// events (`SubagentStart` / `SubagentStop` / `PreToolUse` /
    /// `SessionStart`); any other trigger self-allows.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        let project = if ctx.project_dir.is_empty() {
            project_dir(input)
        } else {
            ctx.project_dir.clone()
        };
        let verdict = match ctx.trigger {
            Some(Trigger::SubagentStart) => Self::handle_start(input, &project),
            Some(Trigger::SubagentStop) => Self::handle_stop(input, &project),
            Some(Trigger::PreToolUse) => Self::handle_pre_tool_use(&project),
            Some(Trigger::SessionStart) => Self::handle_session_start(&project),
            _ => Verdict::Allow,
        };
        Ok(verdict)
    }
}

// ===========================================================================
// main-context-counter — Check on (.*) PreToolUse + Subagent lifecycle
// ===========================================================================

/// Warn / deny thresholds for un-delegated main-context tool calls.
const MAIN_WARN_AT: u32 = 8;
const MAIN_DENY_AT: u32 = 12;
/// Tools that count as main-context "work" (`COUNTED_TOOLS`).
const COUNTED_TOOLS: &[&str] = &[
    "Read", "Edit", "Write", "Bash", "Grep", "Glob", "NotebookEdit",
];
/// The counter file name under `.claude/.agent-state`.
const MAIN_COUNTER_FILE: &str = "main-context.counter.json";

/// The `MUSTARD_MAIN_BUDGET_MODE` mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainBudgetMode {
    Off,
    Warn,
    Strict,
}

/// Resolve the main-budget mode. Port of `getMode`: lowercased,
/// **default `warn`** (not strict — this gate is advisory by default, see
/// `settings.json`'s `MUSTARD_MAIN_BUDGET_MODE: "warn"`). An unrecognised
/// value also resolves to `warn`.
fn main_budget_mode() -> MainBudgetMode {
    match std::env::var("MUSTARD_MAIN_BUDGET_MODE")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "off" => MainBudgetMode::Off,
        "strict" => MainBudgetMode::Strict,
        _ => MainBudgetMode::Warn,
    }
}

/// The persisted main-context counter state.
#[derive(Debug, Clone, Default)]
struct MainState {
    main_count: u32,
    subagent_depth: u32,
}

/// `main-context-counter`: enforces L0 on the orchestrator.
///
/// A `Check`: in strict mode it can `Deny` once `MAIN_DENY_AT` un-delegated
/// tool calls accumulate. The JS hook's warn path prints to stderr; a Rust
/// hook expresses the advisory as a `Verdict::Warn`.
pub struct MainContextCounter;

impl MainContextCounter {
    /// The counter-file path.
    fn counter_path(project_dir: &str) -> std::path::PathBuf {
        Path::new(project_dir)
            .join(".claude")
            .join(".agent-state")
            .join(MAIN_COUNTER_FILE)
    }

    /// Read the persisted state. Fail-open: any error → a zeroed state.
    fn read_state(project_dir: &str) -> MainState {
        let Ok(text) = fs::read_to_string(&Self::counter_path(project_dir)) else {
            return MainState::default();
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            return MainState::default();
        };
        MainState {
            main_count: value
                .get("mainCount")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32,
            subagent_depth: value
                .get("subagentDepth")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32,
        }
    }

    /// Persist the state. Fail-open.
    fn write_state(project_dir: &str, state: &MainState) {
        let dir = Path::new(project_dir).join(".claude").join(".agent-state");
        let _ = fs::create_dir_all(&dir);
        let body = json!({
            "mainCount": state.main_count,
            "subagentDepth": state.subagent_depth,
            "updatedAt": now_iso8601(),
        });
        let _ = fs::write_atomic(&Self::counter_path(project_dir), body.to_string().as_bytes());
    }
}

impl Check for MainContextCounter {
    /// Count an un-delegated main-context tool call and enforce L0.
    ///
    /// `mode` is resolved from `MUSTARD_MAIN_BUDGET_MODE` (default `warn`).
    /// `Off` short-circuits. Lifecycle events keep the `subagentDepth` gauge
    /// honest; a `Task`/`Agent` dispatch resets `mainCount` (work delegated).
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        let mode = main_budget_mode();
        if mode == MainBudgetMode::Off {
            return Ok(Verdict::Allow);
        }
        let project = if ctx.project_dir.is_empty() {
            project_dir(input)
        } else {
            ctx.project_dir.clone()
        };
        let mut state = Self::read_state(&project);

        match ctx.trigger {
            Some(Trigger::SessionStart) => {
                Self::write_state(&project, &MainState::default());
                return Ok(Verdict::Allow);
            }
            Some(Trigger::SubagentStart) => {
                state.subagent_depth += 1;
                Self::write_state(&project, &state);
                return Ok(Verdict::Allow);
            }
            Some(Trigger::SubagentStop) => {
                state.subagent_depth = state.subagent_depth.saturating_sub(1);
                Self::write_state(&project, &state);
                return Ok(Verdict::Allow);
            }
            Some(Trigger::PreToolUse) => {}
            _ => return Ok(Verdict::Allow),
        }

        let tool = input.tool_name.as_deref().unwrap_or_default();

        // A Task/Agent dispatch IS delegation — reset the counter.
        if tool == "Task" || tool == "Agent" {
            state.main_count = 0;
            Self::write_state(&project, &state);
            return Ok(Verdict::Allow);
        }

        // Only count main-context work tools, and only outside a subagent.
        if !COUNTED_TOOLS.contains(&tool) {
            return Ok(Verdict::Allow);
        }
        if state.subagent_depth > 0 {
            return Ok(Verdict::Allow);
        }

        state.main_count += 1;
        let count = state.main_count;
        Self::write_state(&project, &state);

        if mode == MainBudgetMode::Strict && count >= MAIN_DENY_AT {
            return Ok(Verdict::Deny {
                reason: format!(
                    "[main-context-counter] {count} tool calls in the main context \
                     without a Task dispatch (L0 Universal Delegation). Stop and \
                     delegate: dispatch a Task agent for this work so the \
                     orchestrator context stays lean. Set \
                     MUSTARD_MAIN_BUDGET_MODE=warn to allow with a warning."
                ),
            });
        }

        // Warn at WARN_AT, then every 4 calls past it.
        if count == MAIN_WARN_AT
            || (count > MAIN_WARN_AT && (count - MAIN_WARN_AT) % 4 == 0)
        {
            return Ok(Verdict::Warn {
                message: format!(
                    "[main-context-counter] {count} tool calls in the main context \
                     without delegating (L0). Consider a Task dispatch — each direct \
                     Read/Edit inflates the orchestrator context."
                ),
            });
        }

        Ok(Verdict::Allow)
    }
}

// ===========================================================================
// subagent-tracker — Observer on Task/Agent + Subagent lifecycle
// ===========================================================================

/// `subagent-tracker`: emits `agent.start` / `agent.stop` telemetry.
///
/// CONCERN (Wave 4): the JS `subagent-tracker.js` *also* denies a duplicate
/// explorer dispatch within 60s (the `explorer-dedup` path) and inspects
/// pipeline-state / wave-slice byte measurements. Those depend on a
/// `session_id` / wave on `Ctx` that the contract does not yet carry (see the
/// `Ctx` doc comment — "Wave 1 placeholder"). The dedup `deny` is therefore
/// **not ported here**; it is registered as a Wave-4/5 concern. This module
/// ports only the verdict-free `agent.start` / `agent.stop` emission, which is
/// the dominant behaviour and never affects a verdict.
pub struct SubagentTracker;

impl mustard_core::model::contract::Observer for SubagentTracker {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        let project = if ctx.project_dir.is_empty() {
            project_dir(input)
        } else {
            ctx.project_dir.clone()
        };
        let tool_input = &input.tool_input;
        let is_dispatch =
            matches!(input.tool_name.as_deref(), Some("Task") | Some("Agent"));

        match ctx.trigger {
            Some(Trigger::PreToolUse) if is_dispatch => {
                let description = tool_input
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let subagent_type = tool_input
                    .get("subagent_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let model = tool_input.get("model").cloned().unwrap_or(Value::Null);
                emit_event(
                    &project,
                    "subagent-tracker",
                    "agent.start",
                    json!({
                        "description": description,
                        "model": model,
                        "subagentType": subagent_type,
                    }),
                );
            }
            Some(Trigger::PostToolUse) if is_dispatch => {
                let tool_response = input
                    .raw
                    .get("tool_response")
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default();
                let summary: String = tool_response.chars().take(800).collect();
                emit_event(
                    &project,
                    "subagent-tracker",
                    "agent.stop",
                    json!({ "summary": summary }),
                );
                // Finalise the dispatch into the economy spans table (W2):
                // one `record_span` + one `record_api_cost` derived from the
                // payload. Token counts come from the Anthropic `usage`
                // payload when the harness forwards it, else are estimated
                // from byte sizes. Best-effort — never blocks the verdict.
                let tool_input_text = serde_json::to_string(tool_input).unwrap_or_default();
                let model_str = match tool_input.get("model").unwrap_or(&Value::Null) {
                    Value::String(s) => s.clone(),
                    Value::Null => String::new(),
                    other => other.to_string(),
                };
                let usage = input.raw.get("tool_response").and_then(|r| r.get("usage"));
                let api_input_tokens = usage
                    .and_then(|u| u.get("input_tokens"))
                    .and_then(serde_json::Value::as_i64);
                let api_output_tokens = usage
                    .and_then(|u| u.get("output_tokens"))
                    .and_then(serde_json::Value::as_i64);
                let is_error = input
                    .raw
                    .get("tool_response")
                    .and_then(|r| r.get("is_error"))
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                // Synthesise a span id when the harness doesn't supply one —
                // `request_id` is preferred, then a `{session}-{ts}-task`
                // composite that stays unique under `INSERT OR REPLACE`.
                let span_id = input
                    .raw
                    .get("request_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        let sid = input.session_id.as_deref().unwrap_or("unknown");
                        format!("{sid}-{}-task", now_iso8601())
                    });
                record_task_span(
                    &project,
                    input.session_id.as_deref(),
                    span_id,
                    &model_str,
                    current_spec(&project),
                    &tool_input_text,
                    &tool_response,
                    api_input_tokens,
                    api_output_tokens,
                    is_error,
                );
            }
            _ => {}
        }
    }
}

// ===========================================================================
// metrics-tracker — Observer on Bash/Write/Edit/Task/Agent/Read PostToolUse
// ===========================================================================

/// `metrics-tracker`: emits a `tool.use` heartbeat after a tool completes.
///
/// CONCERN: the JS hook resolves the active pipeline-state to tag the event
/// with `phase` / `spec` / `wave`. That depends on pipeline-state access that
/// the `Ctx` does not yet expose (Wave-4/5 concern). This port emits the
/// verdict-free heartbeat with the salient `target` fields; the `phase` /
/// `spec` tags are left `null`, exactly as the JS does when no state is found.
pub struct MetricsTracker;

impl mustard_core::model::contract::Observer for MetricsTracker {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return;
        }
        let project = if ctx.project_dir.is_empty() {
            project_dir(input)
        } else {
            ctx.project_dir.clone()
        };
        let tool_name = input.tool_name.as_deref().unwrap_or_default();
        let tool_input = &input.tool_input;

        // Salient `target` fields, capped — mirrors the JS `target` object.
        let mut target = serde_json::Map::new();
        if let Some(file) = tool_input
            .get("file_path")
            .or_else(|| tool_input.get("notebook_path"))
            .and_then(|v| v.as_str())
        {
            target.insert("file".into(), json!(file));
        }
        if let Some(cmd) = tool_input.get("command").and_then(|v| v.as_str()) {
            target.insert("command".into(), json!(cap(cmd, 120)));
        }
        if let Some(pat) = tool_input.get("pattern").and_then(|v| v.as_str()) {
            target.insert("pattern".into(), json!(cap(pat, 80)));
        }
        if let Some(desc) = tool_input.get("description").and_then(|v| v.as_str()) {
            target.insert("description".into(), json!(cap(desc, 100)));
        }
        if let Some(sub) = tool_input.get("subagent_type").and_then(|v| v.as_str()) {
            target.insert("subagent".into(), json!(sub));
        }
        if let Some(url) = tool_input.get("url").and_then(|v| v.as_str()) {
            target.insert("url".into(), json!(cap(url, 120)));
        }

        let payload = json!({
            "tool": tool_name,
            "phase": Value::Null,
            "target": if target.is_empty() { Value::Null } else { Value::Object(target) },
        });
        emit_event(&project, "metrics-tracker", "tool.use", payload);
    }
}

/// Truncate `s` to `max` chars (char-boundary safe).
fn cap(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

// ===========================================================================
// skill-usage-tracker — Observer on Skill PostToolUse
// ===========================================================================

/// `skill-usage-tracker`: records every Skill invocation as a `skill.invoked`
/// event.
pub struct SkillUsageTracker;

impl mustard_core::model::contract::Observer for SkillUsageTracker {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return;
        }
        if input.tool_name.as_deref() != Some("Skill") {
            return;
        }
        let project = if ctx.project_dir.is_empty() {
            project_dir(input)
        } else {
            ctx.project_dir.clone()
        };
        let tool_input = &input.tool_input;
        let skill = tool_input
            .get("skill")
            .or_else(|| tool_input.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let args = tool_input
            .get("args")
            .map(|v| match v {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            })
            .unwrap_or_default();
        let mut payload = serde_json::Map::new();
        payload.insert("skill".into(), json!(skill));
        payload.insert("args".into(), json!(cap(&args, 200)));
        // `is_error` only when the Skill tool reported a failure.
        if input
            .raw
            .get("tool_response")
            .and_then(|r| r.get("is_error"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            payload.insert("is_error".into(), json!(true));
        }
        emit_event(
            &project,
            "skill-usage-tracker",
            "skill.invoked",
            Value::Object(payload),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn ctx(trigger: Trigger, dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(trigger),
        }
    }

    // --- parse_iso_millis --------------------------------------------------

    #[test]
    fn parse_iso_millis_round_trips_a_known_timestamp() {
        // 2026-05-19T00:00:00Z → ms since epoch.
        let ms = parse_iso_millis("2026-05-19T00:00:00.000Z");
        assert!(ms > 0);
        // The 1970 epoch parses to 0.
        assert_eq!(parse_iso_millis("1970-01-01T00:00:00.000Z"), 0);
        // Garbage → 0 (fail-open).
        assert_eq!(parse_iso_millis("not a date"), 0);
    }

    // --- tool-use-counter parity (hooks.test.js "tool-use-counter.js") -----

    #[test]
    fn start_creates_counter_for_explore_with_15_budget() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            hook_event_name: Some("SubagentStart".to_string()),
            raw: json!({ "agent_id": "explore-123", "agent_type": "Explore" }),
            ..HookInput::default()
        };
        let verdict = ToolUseCounter
            .evaluate(&input, &ctx(Trigger::SubagentStart, project))
            .unwrap();
        // The budget reminder is injected.
        match verdict {
            Verdict::Inject { context } => {
                assert!(context.contains("Tool Budget"));
                assert!(context.contains("15"));
            }
            other => panic!("expected Inject, got {other:?}"),
        }
        // The counter file exists with the Explore budget.
        let f = dir
            .path()
            .join(".claude")
            .join(".agent-state")
            .join("explore-123.counter.json");
        let counter: Value =
            serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        assert_eq!(counter["type"], json!("Explore"));
        assert_eq!(counter["limit"], json!(15));
        assert_eq!(counter["warnAt"], json!(12));
        assert_eq!(counter["count"], json!(0));
    }

    #[test]
    fn start_does_not_create_counter_for_non_explore() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            hook_event_name: Some("SubagentStart".to_string()),
            raw: json!({ "agent_id": "impl-1", "agent_type": "general-purpose" }),
            ..HookInput::default()
        };
        let verdict = ToolUseCounter
            .evaluate(&input, &ctx(Trigger::SubagentStart, project))
            .unwrap();
        assert_eq!(verdict, Verdict::Allow);
        let f = dir
            .path()
            .join(".claude")
            .join(".agent-state")
            .join("impl-1.counter.json");
        assert!(!f.exists());
    }

    #[test]
    fn pre_tool_use_with_no_counters_allows() {
        let dir = tempdir().unwrap();
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Read".to_string()),
            ..HookInput::default()
        };
        let verdict = ToolUseCounter
            .evaluate(&input, &ctx(Trigger::PreToolUse, dir.path().to_str().unwrap()))
            .unwrap();
        assert_eq!(verdict, Verdict::Allow);
    }

    #[test]
    fn pre_tool_use_denies_when_counter_reaches_limit() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let state_dir = dir.path().join(".claude").join(".agent-state");
        std::fs::create_dir_all(&state_dir).unwrap();
        // A counter at count=14, limit=15 — the next PreToolUse hits 15.
        let counter = json!({
            "type": "Explore",
            "limit": 15,
            "warnAt": 12,
            "count": 14,
            "createdAt": now_iso8601(),
        });
        std::fs::write(
            state_dir.join("explore-x.counter.json"),
            counter.to_string(),
        )
        .unwrap();
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Grep".to_string()),
            ..HookInput::default()
        };
        let verdict = ToolUseCounter
            .evaluate(&input, &ctx(Trigger::PreToolUse, project))
            .unwrap();
        match verdict {
            Verdict::Deny { reason } => assert!(reason.contains("Tool Budget")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn pre_tool_use_warns_at_threshold() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let state_dir = dir.path().join(".claude").join(".agent-state");
        std::fs::create_dir_all(&state_dir).unwrap();
        // count=11, warnAt=12 — the next PreToolUse hits exactly 12.
        let counter = json!({
            "type": "Explore", "limit": 15, "warnAt": 12,
            "count": 11, "createdAt": now_iso8601(),
        });
        std::fs::write(
            state_dir.join("explore-w.counter.json"),
            counter.to_string(),
        )
        .unwrap();
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Grep".to_string()),
            ..HookInput::default()
        };
        let verdict = ToolUseCounter
            .evaluate(&input, &ctx(Trigger::PreToolUse, project))
            .unwrap();
        match verdict {
            Verdict::Warn { message } => assert!(message.contains("12/15")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn pre_tool_use_deletes_stale_counter() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let state_dir = dir.path().join(".claude").join(".agent-state");
        std::fs::create_dir_all(&state_dir).unwrap();
        // createdAt well over 10 minutes ago → stale.
        let counter = json!({
            "type": "Explore", "limit": 15, "warnAt": 12, "count": 14,
            "createdAt": "2000-01-01T00:00:00.000Z",
        });
        let file = state_dir.join("explore-stale.counter.json");
        std::fs::write(&file, counter.to_string()).unwrap();
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Grep".to_string()),
            ..HookInput::default()
        };
        let verdict = ToolUseCounter
            .evaluate(&input, &ctx(Trigger::PreToolUse, project))
            .unwrap();
        // Stale counter is skipped → no deny — and the file is gone.
        assert_eq!(verdict, Verdict::Allow);
        assert!(!file.exists());
    }

    #[test]
    fn session_start_clears_counters() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let state_dir = dir.path().join(".claude").join(".agent-state");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(state_dir.join("a.counter.json"), "{}").unwrap();
        std::fs::write(state_dir.join("b.counter.json"), "{}").unwrap();
        ToolUseCounter
            .evaluate(
                &HookInput {
                    hook_event_name: Some("SessionStart".to_string()),
                    ..HookInput::default()
                },
                &ctx(Trigger::SessionStart, project),
            )
            .unwrap();
        assert!(!state_dir.join("a.counter.json").exists());
        assert!(!state_dir.join("b.counter.json").exists());
    }

    #[test]
    fn stop_removes_counter_file() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let state_dir = dir.path().join(".claude").join(".agent-state");
        std::fs::create_dir_all(&state_dir).unwrap();
        let file = state_dir.join("explore-s.counter.json");
        std::fs::write(&file, "{}").unwrap();
        ToolUseCounter
            .evaluate(
                &HookInput {
                    hook_event_name: Some("SubagentStop".to_string()),
                    raw: json!({ "agent_id": "explore-s" }),
                    ..HookInput::default()
                },
                &ctx(Trigger::SubagentStop, project),
            )
            .unwrap();
        assert!(!file.exists());
    }

    // --- main-context-counter parity --------------------------------------

    #[test]
    fn main_counter_task_dispatch_resets_count() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        // Seed a non-zero count.
        MainContextCounter::write_state(
            project,
            &MainState {
                main_count: 9,
                subagent_depth: 0,
            },
        );
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Task".to_string()),
            ..HookInput::default()
        };
        let verdict = MainContextCounter
            .evaluate(&input, &ctx(Trigger::PreToolUse, project))
            .unwrap();
        assert_eq!(verdict, Verdict::Allow);
        assert_eq!(MainContextCounter::read_state(project).main_count, 0);
    }

    #[test]
    fn main_counter_increments_on_counted_tool() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Read".to_string()),
            ..HookInput::default()
        };
        MainContextCounter
            .evaluate(&input, &ctx(Trigger::PreToolUse, project))
            .unwrap();
        assert_eq!(MainContextCounter::read_state(project).main_count, 1);
    }

    #[test]
    fn main_counter_does_not_count_inside_subagent() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        MainContextCounter::write_state(
            project,
            &MainState {
                main_count: 0,
                subagent_depth: 1,
            },
        );
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Read".to_string()),
            ..HookInput::default()
        };
        MainContextCounter
            .evaluate(&input, &ctx(Trigger::PreToolUse, project))
            .unwrap();
        // subagentDepth > 0 → not counted.
        assert_eq!(MainContextCounter::read_state(project).main_count, 0);
    }

    // --- Observer smoke tests (infallible, never panic) --------------------

    #[test]
    fn subagent_tracker_observe_is_infallible() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            tool_name: Some("Task".to_string()),
            tool_input: json!({ "subagent_type": "Explore", "description": "x" }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        use mustard_core::model::contract::Observer;
        SubagentTracker.observe(&input, &ctx(Trigger::PreToolUse, project));
    }

    #[test]
    fn metrics_tracker_observe_is_infallible() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "git status" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        use mustard_core::model::contract::Observer;
        MetricsTracker.observe(&input, &ctx(Trigger::PostToolUse, project));
    }

    #[test]
    fn skill_usage_tracker_emits_skill_invoked() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            tool_name: Some("Skill".to_string()),
            tool_input: json!({ "skill": "karpathy-guidelines", "args": "" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        use mustard_core::model::contract::Observer;
        SkillUsageTracker.observe(&input, &ctx(Trigger::PostToolUse, project));
        // The event log must now carry a `skill.invoked` line.
        let events = SqliteEventStore::for_project(project)
            .and_then(|s| s.replay())
            .unwrap();
        assert!(events.iter().any(|e| e.event == "skill.invoked"));
    }

    #[test]
    fn skill_usage_tracker_ignores_non_skill_tool() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        use mustard_core::model::contract::Observer;
        SkillUsageTracker.observe(&input, &ctx(Trigger::PostToolUse, project));
        let events = SqliteEventStore::for_project(project)
            .and_then(|s| s.replay())
            .unwrap();
        assert!(events.is_empty());
    }
}
