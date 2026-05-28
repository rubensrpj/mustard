//! NDJSON-backed economy readers.
//!
//! W7A of [[2026-05-26-no-sqlite-git-source-of-truth]] migrated every reader
//! off the legacy SQLite connection. Each function now takes the project
//! root [`Path`] + an [`EconomyScope`] and walks the per-spec NDJSON event
//! log under `<project_root>/.claude/spec/*/.events/*.ndjson` (plus the
//! cross-spec session sink at `<project_root>/.claude/.session/*/.events/*.ndjson`).
//!
//! Event kinds consumed by each function:
//!
//! | Function | Kinds |
//! |---|---|
//! | [`economy_summary`] | `pipeline.telemetry.metric` (measured), `pipeline.telemetry.run` + `pipeline.economy.run` (estimated), `pipeline.economy.savings.*` (savings) |
//! | [`per_agent_costs`] | `pipeline.telemetry.run` + `pipeline.economy.run` |
//! | [`per_spec_costs`] | `pipeline.telemetry.run` + `pipeline.economy.run` |
//! | [`per_wave_costs`] | `pipeline.telemetry.run` + `pipeline.economy.run` |
//! | [`savings_breakdown`] | `pipeline.economy.savings.*` |
//! | [`context_routing_quality`] | `pipeline.telemetry.run` (cache hit), `pipeline.economy.context.frame` (prefix/retry ratios) |
//!
//! The two `*.run` kinds are aliases: the OTEL collector writes
//! `pipeline.telemetry.run` for measured spans, the tracker hook writes
//! `pipeline.economy.run` for estimated spans. The shape is identical, so
//! a single union filter picks up both.
//!
//! ## Fail-open contract
//!
//! Missing event directories, unreadable NDJSON lines, and parse failures
//! all degrade silently — every read returns the partial aggregate it could
//! compute. The only way a reader returns `Err` is via the
//! [`MultiProjectReader`] fan-out's closure contract, which is itself
//! fail-open per project.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::platform::error::Result;
use crate::io::events::reader::EventReader;
use crate::io::events::types::Event;

use super::model::{
    AgentCost, ContextRoutingMetrics, EconomySummary, SavingsBreakdown, SavingsBySource,
    SavingsSource, SessionCost, SpecCost, WaveCost,
};
use super::multi_project::MultiProjectReader;
use super::scope::{AgentId, EconomyScope, ProjectPath, SpecId, WaveId};

// ===========================================================================
// Internal event-name helpers
// ===========================================================================

/// Event names that carry a run-usage payload (estimated or measured).
const RUN_KINDS: &[&str] = &["pipeline.telemetry.run", "pipeline.economy.run"];

/// Event-name prefix for the savings family.
const SAVINGS_PREFIX: &str = "pipeline.economy.savings.";

/// Event name for the context-frame channel.
const CONTEXT_FRAME_KIND: &str = "pipeline.economy.context.frame";

/// Event name for the OTEL metric channel (cost.usage / token.usage / etc.).
const TELEMETRY_METRIC_KIND: &str = "pipeline.telemetry.metric";

/// Read the canonical event name from an [`Event`]. NDJSON writers emit a
/// top-level `"event"` field; if absent (older payloads), fall back to the
/// `kind` discriminator which the OTEL collector sets to the same value.
fn event_name(ev: &Event) -> &str {
    ev.raw
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or(ev.kind.as_str())
}

// ===========================================================================
// Filesystem walk
// ===========================================================================

/// All `.ndjson` files under `<root>/.claude/spec/*/.events/`,
/// `<root>/.claude/spec/*/wave-*/events/`, and
/// `<root>/.claude/.session/*/.events/` — the three canonical event sinks.
///
/// Returns paths in stable filesystem order (no sort guarantee — readers
/// must not depend on ordering across files).
fn ndjson_paths(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let claude_dir = root.join(".claude");

    // Per-spec channel.
    let spec_root = claude_dir.join("spec");
    if let Ok(specs) = std::fs::read_dir(&spec_root) {
        for spec_entry in specs.flatten() {
            let spec_path = spec_entry.path();
            if !spec_path.is_dir() {
                continue;
            }
            collect_events_in(&spec_path.join(".events"), &mut out);
            // Wave subdirs — `wave-N-{role}/events/` shape.
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
                    collect_events_in(&wp.join("events"), &mut out);
                    collect_events_in(&wp.join(".events"), &mut out);
                }
            }
        }
    }

    // Cross-spec session sink (used by the OTEL collector when no spec is
    // active).
    let session_root = claude_dir.join(".session");
    if let Ok(sessions) = std::fs::read_dir(&session_root) {
        for session_entry in sessions.flatten() {
            let sp = session_entry.path();
            if !sp.is_dir() {
                continue;
            }
            collect_events_in(&sp.join(".events"), &mut out);
        }
    }

    out
}

/// Push every `*.ndjson` file in `dir` onto `out`. No-op when `dir` is
/// absent or unreadable.
fn collect_events_in(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) == Some("ndjson") {
            out.push(p);
        }
    }
}

/// Collect every NDJSON event under `root`. The walk is bounded — each
/// file is read line-by-line via [`EventReader::stream`] (no full-file
/// load), and only the parsed `Event`s are kept in memory.
///
/// Returned as a `Vec` rather than an iterator because the path borrow
/// inside [`EventReader::stream`] does not survive the closure (Rust 2024
/// `impl Trait` lifetime capture).
fn walk_events(root: &Path) -> Vec<Event> {
    let mut out: Vec<Event> = Vec::new();
    for path in ndjson_paths(root) {
        for ev in EventReader::stream(&path) {
            out.push(ev);
        }
    }
    out
}

// ===========================================================================
// Scope filtering helpers
// ===========================================================================

/// `(spec_filter, wave_filter)` derived from a scope. Project / AllProjects
/// scopes return `(None, None)` — readers then aggregate everything they see.
fn scope_filters(scope: &EconomyScope) -> (Option<&str>, Option<&str>) {
    match scope {
        EconomyScope::Project(_) | EconomyScope::AllProjects(_) => (None, None),
        EconomyScope::Spec { spec, .. } => (Some(spec.0.as_str()), None),
        EconomyScope::Wave { spec, wave, .. } => (Some(spec.0.as_str()), Some(wave.0.as_str())),
    }
}

/// Test whether an event's payload matches the spec/wave filters.
fn matches_scope(payload: &Value, spec_f: Option<&str>, wave_f: Option<&str>) -> bool {
    if let Some(want) = spec_f {
        let got = payload
            .get("spec")
            .or_else(|| payload.get("spec_id"))
            .and_then(Value::as_str);
        if got != Some(want) {
            return false;
        }
    }
    if let Some(want) = wave_f {
        let got = payload
            .get("wave_id")
            .or_else(|| payload.get("wave"))
            .and_then(Value::as_str);
        if got != Some(want) {
            return false;
        }
    }
    true
}

// `scope_project_path` was deleted along with the SQLite reader — callers
// receive the project root explicitly as the first argument now.

// ===========================================================================
// Public readers
// ===========================================================================

/// Top-level summary — total cost, total tokens, total savings, top 3 agents.
///
/// Mirrors the legacy SQLite path:
///
/// * **Unfiltered (Project / AllProjects)** — headline cost + tokens come
///   from MEASURED OTEL metrics (`claude_code.cost.usage` /
///   `.token.usage`); `by_session` is populated from the same metrics
///   enriched with the spec list from `pipeline.{telemetry,economy}.run`.
/// * **Spec / Wave** — headline cost + tokens come from ESTIMATED run
///   events filtered by the scope.
///
/// # Errors
///
/// Returns `Ok` always — every IO failure degrades to the partial aggregate.
#[allow(clippy::too_many_lines)]
pub fn economy_summary(project_root: &Path, scope: EconomyScope) -> Result<EconomySummary> {
    if let EconomyScope::AllProjects(ref projects) = scope {
        let reader = MultiProjectReader::new();
        let per_project = reader.fan_out(projects, |root, proj| {
            economy_summary(root, EconomyScope::Project(proj.clone()))
        });
        let mut acc = EconomySummary::default();
        for s in per_project.values() {
            acc.total_cost_usd_micros += s.total_cost_usd_micros;
            acc.total_tokens += s.total_tokens;
            acc.total_tokens_saved += s.total_tokens_saved;
            acc.span_count += s.span_count;
            acc.top_agents_by_cost.extend(s.top_agents_by_cost.clone());
            acc.by_session.extend(s.by_session.clone());
            acc.last_updated_ms = acc.last_updated_ms.max(s.last_updated_ms);
            acc.last_estimated_ms = acc.last_estimated_ms.max(s.last_estimated_ms);
        }
        acc.top_agents_by_cost
            .sort_by_key(|b| std::cmp::Reverse(b.cost_usd_micros));
        acc.top_agents_by_cost.truncate(3);
        acc.by_session
            .sort_by(|a, b| b.usd.partial_cmp(&a.usd).unwrap_or(std::cmp::Ordering::Equal));
        acc.by_session.truncate(8);
        return Ok(acc);
    }

    let (spec_f, wave_f) = scope_filters(&scope);
    let unfiltered = spec_f.is_none() && wave_f.is_none();

    // Collect once — economy_summary touches every aggregate, so a single
    // pass keeps the IO bound at one walk per call.
    let events: Vec<Event> = walk_events(project_root);

    // ── Estimated run-usage totals (always computed; used for span_count and
    //    as the cost fallback at filtered scope) ──
    let mut est_cost_micros: i64 = 0;
    let mut est_tokens: i64 = 0;
    let mut span_count: i64 = 0;
    let mut last_estimated_ms: Option<i64> = None;
    for ev in &events {
        if !RUN_KINDS.contains(&event_name(ev)) {
            continue;
        }
        if !matches_scope(&ev.payload, spec_f, wave_f) {
            continue;
        }
        span_count += 1;
        est_cost_micros += ev
            .payload
            .get("cost_usd_micros")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        est_tokens += ev
            .payload
            .get("input_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            + ev.payload
                .get("output_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
        if let Some(ts_ms) = run_event_started_at_ms(ev) {
            last_estimated_ms = Some(last_estimated_ms.map_or(ts_ms, |cur| cur.max(ts_ms)));
        }
    }

    // ── Measured totals (only at unfiltered scope) ──
    let (measured_cost_micros, measured_tokens, last_updated_ms, by_session): (
        i64,
        i64,
        Option<i64>,
        Vec<SessionCost>,
    ) = if unfiltered {
        measured_totals_and_sessions(&events)
    } else {
        (0, 0, None, Vec::new())
    };

    let (total_cost, total_tokens) = if unfiltered {
        (measured_cost_micros, measured_tokens)
    } else {
        (est_cost_micros, est_tokens)
    };

    // ── Savings total ──
    let mut total_saved: i64 = 0;
    for ev in &events {
        if !event_name(ev).starts_with(SAVINGS_PREFIX) {
            continue;
        }
        if !matches_scope(&ev.payload, spec_f, wave_f) {
            continue;
        }
        total_saved += ev
            .payload
            .get("tokens_saved")
            .and_then(Value::as_i64)
            .unwrap_or(0);
    }

    // ── Top 3 agents by cost (reuses per_agent_costs to stay DRY) ──
    let top = per_agent_costs(project_root, scope.clone())?
        .into_iter()
        .take(3)
        .collect::<Vec<_>>();

    Ok(EconomySummary {
        total_cost_usd_micros: total_cost,
        total_tokens,
        total_tokens_saved: total_saved,
        span_count,
        top_agents_by_cost: top,
        by_session,
        last_updated_ms: if unfiltered { last_updated_ms } else { None },
        last_estimated_ms: if unfiltered { last_estimated_ms } else { None },
    })
}

/// Aggregate measured totals + per-session enrichment from a pre-collected
/// event slice. Returns `(cost_micros, tokens, last_updated_ms, by_session)`.
///
/// Measured metrics carry float USD / float token counters; we round to the
/// integer transport units (`micro-USD` and `i64 tokens`) the dashboard
/// expects.
fn measured_totals_and_sessions(events: &[Event]) -> (i64, i64, Option<i64>, Vec<SessionCost>) {
    let mut cost_usd: f64 = 0.0;
    let mut tokens: f64 = 0.0;
    let mut last_updated_ms: Option<i64> = None;
    let mut per_session_cost: BTreeMap<String, f64> = BTreeMap::new();
    let mut per_session_last_at: BTreeMap<String, i64> = BTreeMap::new();
    let mut per_session_specs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for ev in events {
        let name = event_name(ev);
        if name == TELEMETRY_METRIC_KIND {
            let metric = ev.payload.get("metric").and_then(Value::as_str).unwrap_or("");
            let sum = ev.payload.get("sum").and_then(Value::as_f64).unwrap_or(0.0);
            let session_id = ev
                .payload
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let updated_at = ev
                .payload
                .get("updated_at")
                .or_else(|| ev.payload.get("ts_bucket"))
                .and_then(Value::as_i64);

            if metric == "claude_code.cost.usage" {
                cost_usd += sum;
                if !session_id.is_empty() {
                    *per_session_cost.entry(session_id.clone()).or_insert(0.0) += sum;
                    if let Some(ts) = updated_at {
                        let entry = per_session_last_at.entry(session_id.clone()).or_insert(0);
                        if ts > *entry {
                            *entry = ts;
                        }
                    }
                }
                if let Some(ts) = updated_at {
                    last_updated_ms = Some(last_updated_ms.map_or(ts, |cur| cur.max(ts)));
                }
            } else if metric == "claude_code.token.usage" {
                tokens += sum;
                if let Some(ts) = updated_at {
                    last_updated_ms = Some(last_updated_ms.map_or(ts, |cur| cur.max(ts)));
                }
            }
        } else if RUN_KINDS.contains(&name) {
            // Enrich per-session spec list from run events.
            let session_id = ev
                .payload
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if session_id.is_empty() {
                continue;
            }
            if let Some(spec) = ev.payload.get("spec").and_then(Value::as_str) {
                per_session_specs
                    .entry(session_id)
                    .or_default()
                    .insert(spec.to_string());
            }
        }
    }

    let cost_micros = (cost_usd * 1_000_000.0).round() as i64;
    let tokens_int = tokens.round() as i64;

    // Build the `by_session` vector ordered by USD descending, capped to 8.
    let mut sessions: Vec<SessionCost> = per_session_cost
        .into_iter()
        .filter(|(_, usd)| *usd > 0.0)
        .map(|(session_id, usd)| {
            let last_at_ms = per_session_last_at.get(&session_id).copied();
            let specs: Vec<String> = per_session_specs
                .get(&session_id)
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default();
            SessionCost {
                session_id,
                usd,
                last_at_ms,
                specs,
            }
        })
        .collect();
    sessions.sort_by(|a, b| b.usd.partial_cmp(&a.usd).unwrap_or(std::cmp::Ordering::Equal));
    sessions.truncate(8);

    (cost_micros, tokens_int, last_updated_ms, sessions)
}

/// Extract an epoch-ms timestamp from a run event's payload, trying
/// `started_at` (the canonical column) then `ts` (ISO-8601 string).
fn run_event_started_at_ms(ev: &Event) -> Option<i64> {
    if let Some(v) = ev.payload.get("started_at").and_then(Value::as_i64) {
        return Some(v);
    }
    let ts = ev.payload.get("ts").and_then(Value::as_str)?;
    let ms = crate::platform::time::parse_iso_millis(ts).unwrap_or(0);
    if ms == 0 {
        None
    } else {
        Some(ms)
    }
}

/// Per-agent cost roll-up. Ordered by cost descending.
///
/// Reads `pipeline.{telemetry,economy}.run` events, grouping by
/// `payload.agent_id` (set at write time by both the OTEL collector and the
/// tracker hook). Rows missing an agent id are dropped.
///
/// # Errors
///
/// Returns `Ok` always — every IO failure degrades to the partial aggregate.
pub fn per_agent_costs(project_root: &Path, scope: EconomyScope) -> Result<Vec<AgentCost>> {
    if let EconomyScope::AllProjects(ref projects) = scope {
        let reader = MultiProjectReader::new();
        let per_project = reader.fan_out(projects, |root, proj| {
            per_agent_costs(root, EconomyScope::Project(proj.clone()))
        });
        let mut merged: HashMap<String, AgentCost> = HashMap::new();
        for rows in per_project.values() {
            for row in rows {
                let entry = merged.entry(row.agent_id.0.clone()).or_insert(AgentCost {
                    agent_id: row.agent_id.clone(),
                    cost_usd_micros: 0,
                    tokens: 0,
                    span_count: 0,
                });
                entry.cost_usd_micros += row.cost_usd_micros;
                entry.tokens += row.tokens;
                entry.span_count += row.span_count;
            }
        }
        let mut out: Vec<AgentCost> = merged.into_values().collect();
        out.sort_by_key(|b| std::cmp::Reverse(b.cost_usd_micros));
        return Ok(out);
    }

    let (spec_f, wave_f) = scope_filters(&scope);
    let mut by_agent: HashMap<String, AgentCost> = HashMap::new();
    for ev in walk_events(project_root) {
        if !RUN_KINDS.contains(&event_name(&ev)) {
            continue;
        }
        if !matches_scope(&ev.payload, spec_f, wave_f) {
            continue;
        }
        let Some(agent_id) = payload_agent_id(&ev.payload) else {
            continue;
        };
        let cost = ev
            .payload
            .get("cost_usd_micros")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let tokens = ev
            .payload
            .get("input_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            + ev.payload
                .get("output_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
        let entry = by_agent.entry(agent_id.clone()).or_insert(AgentCost {
            agent_id: AgentId(agent_id),
            cost_usd_micros: 0,
            tokens: 0,
            span_count: 0,
        });
        entry.cost_usd_micros += cost;
        entry.tokens += tokens;
        entry.span_count += 1;
    }
    let mut out: Vec<AgentCost> = by_agent.into_values().collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.cost_usd_micros));
    Ok(out)
}

/// Per-spec cost roll-up. Newest spec first; cost desc on tie.
///
/// # Errors
///
/// Returns `Ok` always.
pub fn per_spec_costs(project_root: &Path, scope: EconomyScope) -> Result<Vec<SpecCost>> {
    if let EconomyScope::AllProjects(ref projects) = scope {
        let reader = MultiProjectReader::new();
        let per_project = reader.fan_out(projects, |root, proj| {
            per_spec_costs(root, EconomyScope::Project(proj.clone()))
        });
        let mut merged: HashMap<String, SpecCost> = HashMap::new();
        for rows in per_project.values() {
            for row in rows {
                let entry = merged.entry(row.spec_id.0.clone()).or_insert(SpecCost {
                    spec_id: row.spec_id.clone(),
                    cost_usd_micros: 0,
                    tokens: 0,
                    span_count: 0,
                    last_started_at: None,
                });
                entry.cost_usd_micros += row.cost_usd_micros;
                entry.tokens += row.tokens;
                entry.span_count += row.span_count;
                entry.last_started_at = entry.last_started_at.max(row.last_started_at);
            }
        }
        let mut out: Vec<SpecCost> = merged.into_values().collect();
        out.sort_by(|a, b| {
            b.last_started_at
                .cmp(&a.last_started_at)
                .then_with(|| b.cost_usd_micros.cmp(&a.cost_usd_micros))
        });
        return Ok(out);
    }

    let (_, wave_f) = scope_filters(&scope);
    let mut by_spec: HashMap<String, SpecCost> = HashMap::new();
    for ev in walk_events(project_root) {
        if !RUN_KINDS.contains(&event_name(&ev)) {
            continue;
        }
        if !matches_scope(&ev.payload, None, wave_f) {
            continue;
        }
        let Some(spec) = ev.payload.get("spec").and_then(Value::as_str) else {
            continue;
        };
        let cost = ev
            .payload
            .get("cost_usd_micros")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let tokens = ev
            .payload
            .get("input_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            + ev.payload
                .get("output_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
        let started_at = run_event_started_at_ms(&ev);
        let entry = by_spec.entry(spec.to_string()).or_insert(SpecCost {
            spec_id: SpecId(spec.to_string()),
            cost_usd_micros: 0,
            tokens: 0,
            span_count: 0,
            last_started_at: None,
        });
        entry.cost_usd_micros += cost;
        entry.tokens += tokens;
        entry.span_count += 1;
        entry.last_started_at = match (entry.last_started_at, started_at) {
            (Some(cur), Some(new)) => Some(cur.max(new)),
            (None, Some(new)) => Some(new),
            (cur, None) => cur,
        };
    }
    let mut out: Vec<SpecCost> = by_spec.into_values().collect();
    out.sort_by(|a, b| {
        b.last_started_at
            .cmp(&a.last_started_at)
            .then_with(|| b.cost_usd_micros.cmp(&a.cost_usd_micros))
    });
    Ok(out)
}

/// Per-wave cost roll-up. Ordered by cost desc.
///
/// # Errors
///
/// Returns `Ok` always.
pub fn per_wave_costs(project_root: &Path, scope: EconomyScope) -> Result<Vec<WaveCost>> {
    if let EconomyScope::AllProjects(ref projects) = scope {
        let reader = MultiProjectReader::new();
        let per_project = reader.fan_out(projects, |root, proj| {
            per_wave_costs(root, EconomyScope::Project(proj.clone()))
        });
        let mut merged: HashMap<(String, String), WaveCost> = HashMap::new();
        for rows in per_project.values() {
            for row in rows {
                let key = (row.spec_id.0.clone(), row.wave_id.0.clone());
                let entry = merged.entry(key).or_insert(WaveCost {
                    spec_id: row.spec_id.clone(),
                    wave_id: row.wave_id.clone(),
                    cost_usd_micros: 0,
                    tokens: 0,
                    span_count: 0,
                });
                entry.cost_usd_micros += row.cost_usd_micros;
                entry.tokens += row.tokens;
                entry.span_count += row.span_count;
            }
        }
        let mut out: Vec<WaveCost> = merged.into_values().collect();
        out.sort_by_key(|b| std::cmp::Reverse(b.cost_usd_micros));
        return Ok(out);
    }

    let (_, wave_f) = scope_filters(&scope);
    let mut by_wave: HashMap<(String, String), WaveCost> = HashMap::new();
    for ev in walk_events(project_root) {
        if !RUN_KINDS.contains(&event_name(&ev)) {
            continue;
        }
        let Some(spec) = ev.payload.get("spec").and_then(Value::as_str) else {
            continue;
        };
        let Some(wave) = ev
            .payload
            .get("wave_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        if let Some(want) = wave_f {
            if wave != want {
                continue;
            }
        }
        let cost = ev
            .payload
            .get("cost_usd_micros")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let tokens = ev
            .payload
            .get("input_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            + ev.payload
                .get("output_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
        let entry = by_wave
            .entry((spec.to_string(), wave.to_string()))
            .or_insert(WaveCost {
                spec_id: SpecId(spec.to_string()),
                wave_id: WaveId(wave.to_string()),
                cost_usd_micros: 0,
                tokens: 0,
                span_count: 0,
            });
        entry.cost_usd_micros += cost;
        entry.tokens += tokens;
        entry.span_count += 1;
    }
    let mut out: Vec<WaveCost> = by_wave.into_values().collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.cost_usd_micros));
    Ok(out)
}

/// Savings breakdown by [`SavingsSource`].
///
/// # Errors
///
/// Returns `Ok` always.
pub fn savings_breakdown(
    project_root: &Path,
    scope: EconomyScope,
) -> Result<SavingsBreakdown> {
    if let EconomyScope::AllProjects(ref projects) = scope {
        let reader = MultiProjectReader::new();
        let per_project = reader.fan_out(projects, |root, proj| {
            savings_breakdown(root, EconomyScope::Project(proj.clone()))
        });
        let mut total = 0i64;
        let mut per_source: HashMap<SavingsSource, (i64, i64)> = HashMap::new();
        for b in per_project.values() {
            total += b.total_tokens_saved;
            for row in &b.per_source {
                let entry = per_source.entry(row.source).or_insert((0, 0));
                entry.0 += row.tokens_saved;
                entry.1 += row.event_count;
            }
        }
        let mut rows: Vec<SavingsBySource> = per_source
            .into_iter()
            .map(|(source, (tokens_saved, event_count))| SavingsBySource {
                source,
                tokens_saved,
                event_count,
            })
            .collect();
        rows.sort_by_key(|b| std::cmp::Reverse(b.tokens_saved));
        return Ok(SavingsBreakdown {
            total_tokens_saved: total,
            per_source: rows,
        });
    }

    let (spec_f, wave_f) = scope_filters(&scope);
    let mut total = 0i64;
    let mut per_source: HashMap<SavingsSource, (i64, i64)> = HashMap::new();
    for ev in walk_events(project_root) {
        let name = event_name(&ev);
        if !name.starts_with(SAVINGS_PREFIX) {
            continue;
        }
        if !matches_scope(&ev.payload, spec_f, wave_f) {
            continue;
        }
        let source_str = ev
            .payload
            .get("source")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| name.strip_prefix(SAVINGS_PREFIX).map(|s| s.replace('-', "_")));
        let Some(source) = source_str.and_then(|s| SavingsSource::from_str_opt(&s)) else {
            continue;
        };
        let saved = ev
            .payload
            .get("tokens_saved")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        total += saved;
        let entry = per_source.entry(source).or_insert((0, 0));
        entry.0 += saved;
        entry.1 += 1;
    }
    let mut rows: Vec<SavingsBySource> = per_source
        .into_iter()
        .map(|(source, (tokens_saved, event_count))| SavingsBySource {
            source,
            tokens_saved,
            event_count,
        })
        .collect();
    rows.sort_by_key(|b| std::cmp::Reverse(b.tokens_saved));
    Ok(SavingsBreakdown {
        total_tokens_saved: total,
        per_source: rows,
    })
}

/// Context-routing quality metrics (cache hit, prefix stable, retry overhead).
///
/// Cache-hit ratio comes from run events
/// (`cache_read_input_tokens / (input_tokens + cache_read_input_tokens)`).
/// Prefix-stable and retry-overhead ratios come from
/// `pipeline.economy.context.frame` events — when no frame events exist
/// (today's reality, because no production caller emits them yet), those
/// ratios are 0, matching the SQLite-era behaviour where the table was empty.
///
/// # Errors
///
/// Returns `Ok` always.
pub fn context_routing_quality(
    project_root: &Path,
    scope: EconomyScope,
) -> Result<ContextRoutingMetrics> {
    if let EconomyScope::AllProjects(ref projects) = scope {
        let reader = MultiProjectReader::new();
        let per_project = reader.fan_out(projects, |root, proj| {
            context_routing_quality(root, EconomyScope::Project(proj.clone()))
        });
        let mut acc = ContextRoutingMetrics::default();
        let mut weight_total = 0i64;
        for m in per_project.values() {
            let w = m.frame_count.max(1);
            acc.prefix_stable_ratio_permille += m.prefix_stable_ratio_permille * w;
            acc.cache_hit_ratio_permille += m.cache_hit_ratio_permille * w;
            acc.retry_overhead_ratio_permille += m.retry_overhead_ratio_permille * w;
            acc.frame_count += m.frame_count;
            weight_total += w;
        }
        if weight_total > 0 {
            acc.prefix_stable_ratio_permille /= weight_total;
            acc.cache_hit_ratio_permille /= weight_total;
            acc.retry_overhead_ratio_permille /= weight_total;
        }
        return Ok(acc);
    }

    let (spec_f, wave_f) = scope_filters(&scope);
    let mut prompt_sum: i64 = 0;
    let mut prefix_sum: i64 = 0;
    let mut retry_sum: i64 = 0;
    let mut frame_count: i64 = 0;
    let mut input_sum: i64 = 0;
    let mut cache_sum: i64 = 0;

    for ev in walk_events(project_root) {
        let name = event_name(&ev);
        if name == CONTEXT_FRAME_KIND {
            if !matches_scope(&ev.payload, spec_f, wave_f) {
                continue;
            }
            prompt_sum += ev
                .payload
                .get("prompt_size_bytes")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            prefix_sum += ev
                .payload
                .get("prefix_stable_bytes")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            retry_sum += ev
                .payload
                .get("retry_overhead_bytes")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            frame_count += 1;
        } else if RUN_KINDS.contains(&name) {
            // Cache hit ratio uses spec filter only (run events do not carry
            // wave_id reliably across all writers — match legacy behaviour).
            if !matches_scope(&ev.payload, spec_f, None) {
                continue;
            }
            input_sum += ev
                .payload
                .get("input_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            cache_sum += ev
                .payload
                .get("cache_read_input_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
        }
    }

    let permille = |num: i64, den: i64| -> i64 {
        if den <= 0 {
            0
        } else {
            #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
            let v = ((num as f64) * 1000.0 / (den as f64)) as i64;
            v
        }
    };
    let cache_den = input_sum + cache_sum;

    Ok(ContextRoutingMetrics {
        prefix_stable_ratio_permille: permille(prefix_sum, prompt_sum),
        cache_hit_ratio_permille: permille(cache_sum, cache_den),
        retry_overhead_ratio_permille: permille(retry_sum, prompt_sum),
        frame_count,
    })
}

// ===========================================================================
// Payload helpers
// ===========================================================================

/// Pull the agent id out of a run payload. Tries the top-level `agent_id`
/// first (the shape `pipeline.economy.run` writes), then the lenient
/// `extra.agent_id` (OTEL `pipeline.telemetry.run`).
fn payload_agent_id(payload: &Value) -> Option<String> {
    if let Some(id) = payload
        .get("agent_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        return Some(id.to_string());
    }
    payload
        .get("extra")
        .and_then(|e| e.get("agent_id"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

// Suppress the unused-import lint when `payload_agent_id` is the only thing
// using `ProjectPath` (it isn't today, but the marker keeps the import path
// stable as readers evolve).
#[allow(dead_code)]
fn _project_path_alive(p: &ProjectPath) -> &Path {
    p.as_path()
}

// ===========================================================================
// Tests — inline fixtures, no SQLite, no external test crate
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Plant `lines` (NDJSON content) at `<root>/.claude/spec/{spec}/.events/seed.ndjson`.
    fn plant_spec_events(root: &Path, spec: &str, lines: &[&str]) {
        let dir = root.join(".claude").join("spec").join(spec).join(".events");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("seed.ndjson"), lines.join("\n")).unwrap();
    }

    /// Plant cross-spec session events at `<root>/.claude/.session/{slug}/.events/seed.ndjson`.
    fn plant_session_events(root: &Path, slug: &str, lines: &[&str]) {
        let dir = root.join(".claude").join(".session").join(slug).join(".events");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("seed.ndjson"), lines.join("\n")).unwrap();
    }

    #[test]
    fn summary_reads_measured_totals_from_ndjson() {
        let dir = tempdir().unwrap();
        // Measured: cost.usage = $49, token.usage = 1234 tokens — cross-spec
        // (session sink, matching OTEL collector behaviour).
        plant_session_events(
            dir.path(),
            "sess-A",
            &[
                r#"{"kind":"pipeline.telemetry.metric","event":"pipeline.telemetry.metric","payload":{"metric":"claude_code.cost.usage","session_id":"sess-A","sum":49.0,"updated_at":2000}}"#,
                r#"{"kind":"pipeline.telemetry.metric","event":"pipeline.telemetry.metric","payload":{"metric":"claude_code.token.usage","session_id":"sess-A","sum":1234.0,"updated_at":2000}}"#,
            ],
        );
        let scope = EconomyScope::Project(ProjectPath::new(dir.path()));
        let s = economy_summary(dir.path(), scope).unwrap();
        assert_eq!(s.total_cost_usd_micros, 49_000_000);
        assert_eq!(s.total_tokens, 1234);
    }

    #[test]
    fn savings_breakdown_reads_ndjson() {
        let dir = tempdir().unwrap();
        plant_spec_events(
            dir.path(),
            "spec-A",
            &[
                r#"{"kind":"pipeline.economy.savings.rtk-rewrite","event":"pipeline.economy.savings.rtk-rewrite","payload":{"source":"rtk_rewrite","tokens_saved":100}}"#,
                r#"{"kind":"pipeline.economy.savings.rtk-rewrite","event":"pipeline.economy.savings.rtk-rewrite","payload":{"source":"rtk_rewrite","tokens_saved":200}}"#,
                r#"{"kind":"pipeline.economy.savings.bash-guard-block","event":"pipeline.economy.savings.bash-guard-block","payload":{"source":"bash_guard_block","tokens_saved":50}}"#,
            ],
        );
        let scope = EconomyScope::Project(ProjectPath::new(dir.path()));
        let b = savings_breakdown(dir.path(), scope).unwrap();
        assert_eq!(b.total_tokens_saved, 350);
        assert_eq!(b.per_source.len(), 2);
        let rtk = &b.per_source[0]; // RtkRewrite is the larger one
        assert_eq!(rtk.source, SavingsSource::RtkRewrite);
        assert_eq!(rtk.tokens_saved, 300);
        assert_eq!(rtk.event_count, 2);
    }

    #[test]
    fn per_spec_costs_groups_run_events_by_spec() {
        let dir = tempdir().unwrap();
        plant_spec_events(
            dir.path(),
            "spec-A",
            &[
                r#"{"kind":"pipeline.telemetry.run","event":"pipeline.telemetry.run","payload":{"spec":"spec-A","cost_usd_micros":1000,"input_tokens":50,"output_tokens":50,"agent_id":"explore","started_at":1000}}"#,
                r#"{"kind":"pipeline.economy.run","event":"pipeline.economy.run","payload":{"spec":"spec-A","cost_usd_micros":2000,"input_tokens":100,"output_tokens":100,"agent_id":"plan","started_at":2000}}"#,
            ],
        );
        let scope = EconomyScope::Project(ProjectPath::new(dir.path()));
        let rows = per_spec_costs(dir.path(), scope).unwrap();
        assert_eq!(rows.len(), 1);
        let spec_a = &rows[0];
        assert_eq!(spec_a.spec_id.0, "spec-A");
        assert_eq!(spec_a.cost_usd_micros, 3000);
        assert_eq!(spec_a.tokens, 300);
        assert_eq!(spec_a.span_count, 2);
        assert_eq!(spec_a.last_started_at, Some(2000));
    }

    #[test]
    fn per_agent_costs_groups_run_events_by_agent() {
        let dir = tempdir().unwrap();
        plant_spec_events(
            dir.path(),
            "spec-A",
            &[
                r#"{"kind":"pipeline.telemetry.run","event":"pipeline.telemetry.run","payload":{"spec":"spec-A","cost_usd_micros":1000,"input_tokens":50,"output_tokens":50,"agent_id":"explore"}}"#,
                r#"{"kind":"pipeline.telemetry.run","event":"pipeline.telemetry.run","payload":{"spec":"spec-A","cost_usd_micros":2000,"input_tokens":100,"output_tokens":100,"agent_id":"explore"}}"#,
                r#"{"kind":"pipeline.telemetry.run","event":"pipeline.telemetry.run","payload":{"spec":"spec-A","cost_usd_micros":500,"input_tokens":25,"output_tokens":25,"agent_id":"plan"}}"#,
            ],
        );
        let scope = EconomyScope::Project(ProjectPath::new(dir.path()));
        let rows = per_agent_costs(dir.path(), scope).unwrap();
        assert_eq!(rows.len(), 2);
        // Sorted by cost desc — explore (3000) first.
        assert_eq!(rows[0].agent_id.0, "explore");
        assert_eq!(rows[0].cost_usd_micros, 3000);
        assert_eq!(rows[0].tokens, 300);
        assert_eq!(rows[0].span_count, 2);
        assert_eq!(rows[1].agent_id.0, "plan");
        assert_eq!(rows[1].cost_usd_micros, 500);
    }

    #[test]
    fn per_wave_costs_groups_run_events_by_wave() {
        let dir = tempdir().unwrap();
        plant_spec_events(
            dir.path(),
            "spec-A",
            &[
                r#"{"kind":"pipeline.telemetry.run","event":"pipeline.telemetry.run","payload":{"spec":"spec-A","wave_id":"w1","cost_usd_micros":1000,"input_tokens":50,"output_tokens":50,"agent_id":"explore"}}"#,
                r#"{"kind":"pipeline.telemetry.run","event":"pipeline.telemetry.run","payload":{"spec":"spec-A","wave_id":"w2","cost_usd_micros":2000,"input_tokens":100,"output_tokens":100,"agent_id":"plan"}}"#,
            ],
        );
        let scope = EconomyScope::Project(ProjectPath::new(dir.path()));
        let rows = per_wave_costs(dir.path(), scope).unwrap();
        assert_eq!(rows.len(), 2);
        // Sorted by cost desc — w2 first.
        assert_eq!(rows[0].wave_id.0, "w2");
        assert_eq!(rows[0].cost_usd_micros, 2000);
        assert_eq!(rows[1].wave_id.0, "w1");
    }

    #[test]
    fn context_routing_cache_hit_from_run_events() {
        let dir = tempdir().unwrap();
        // input_tokens=200, cache_read_input_tokens=800 → cache_hit ratio = 800/(200+800) = 800
        plant_spec_events(
            dir.path(),
            "spec-A",
            &[r#"{"kind":"pipeline.telemetry.run","event":"pipeline.telemetry.run","payload":{"spec":"spec-A","cost_usd_micros":0,"input_tokens":200,"output_tokens":0,"cache_read_input_tokens":800,"agent_id":"explore"}}"#],
        );
        let scope = EconomyScope::Project(ProjectPath::new(dir.path()));
        let m = context_routing_quality(dir.path(), scope).unwrap();
        assert_eq!(m.cache_hit_ratio_permille, 800);
        assert_eq!(m.frame_count, 0); // no context-frame events planted
    }

    #[test]
    fn savings_breakdown_at_spec_scope_filters_by_payload_spec_id() {
        let dir = tempdir().unwrap();
        plant_spec_events(
            dir.path(),
            "spec-A",
            &[
                r#"{"kind":"pipeline.economy.savings.rtk-rewrite","event":"pipeline.economy.savings.rtk-rewrite","payload":{"source":"rtk_rewrite","tokens_saved":100,"spec_id":"spec-A"}}"#,
                r#"{"kind":"pipeline.economy.savings.rtk-rewrite","event":"pipeline.economy.savings.rtk-rewrite","payload":{"source":"rtk_rewrite","tokens_saved":999,"spec_id":"spec-OTHER"}}"#,
            ],
        );
        let scope = EconomyScope::Spec {
            project: ProjectPath::new(dir.path()),
            spec: SpecId::new("spec-A"),
        };
        let b = savings_breakdown(dir.path(), scope).unwrap();
        assert_eq!(b.total_tokens_saved, 100);
        assert_eq!(b.per_source.len(), 1);
    }

    #[test]
    fn economy_summary_aggregates_savings_runs_and_measured() {
        let dir = tempdir().unwrap();
        plant_spec_events(
            dir.path(),
            "spec-A",
            &[
                r#"{"kind":"pipeline.economy.run","event":"pipeline.economy.run","payload":{"spec":"spec-A","cost_usd_micros":1000,"input_tokens":50,"output_tokens":50,"agent_id":"explore"}}"#,
                r#"{"kind":"pipeline.economy.run","event":"pipeline.economy.run","payload":{"spec":"spec-A","cost_usd_micros":2000,"input_tokens":100,"output_tokens":100,"agent_id":"plan"}}"#,
                r#"{"kind":"pipeline.economy.savings.rtk-rewrite","event":"pipeline.economy.savings.rtk-rewrite","payload":{"source":"rtk_rewrite","tokens_saved":500}}"#,
            ],
        );
        plant_session_events(
            dir.path(),
            "sess-A",
            &[r#"{"kind":"pipeline.telemetry.metric","event":"pipeline.telemetry.metric","payload":{"metric":"claude_code.cost.usage","session_id":"sess-A","sum":0.003,"updated_at":1234}}"#],
        );
        let scope = EconomyScope::Project(ProjectPath::new(dir.path()));
        let s = economy_summary(dir.path(), scope).unwrap();
        // unfiltered → measured cost
        assert_eq!(s.total_cost_usd_micros, 3000);
        assert_eq!(s.span_count, 2);
        assert_eq!(s.total_tokens_saved, 500);
        assert_eq!(s.top_agents_by_cost.len(), 2);
        assert_eq!(s.top_agents_by_cost[0].agent_id.0, "plan");
    }

    #[test]
    fn economy_summary_at_spec_scope_uses_estimated_run_usage() {
        let dir = tempdir().unwrap();
        plant_spec_events(
            dir.path(),
            "spec-A",
            &[
                r#"{"kind":"pipeline.economy.run","event":"pipeline.economy.run","payload":{"spec":"spec-A","cost_usd_micros":1000,"input_tokens":50,"output_tokens":50,"agent_id":"explore"}}"#,
                r#"{"kind":"pipeline.economy.run","event":"pipeline.economy.run","payload":{"spec":"spec-A","cost_usd_micros":2000,"input_tokens":100,"output_tokens":100,"agent_id":"plan"}}"#,
            ],
        );
        // Plant measured at session scope — must NOT leak into spec-scoped summary.
        plant_session_events(
            dir.path(),
            "sess-A",
            &[r#"{"kind":"pipeline.telemetry.metric","event":"pipeline.telemetry.metric","payload":{"metric":"claude_code.cost.usage","session_id":"sess-A","sum":99.0,"updated_at":2000}}"#],
        );
        let scope = EconomyScope::Spec {
            project: ProjectPath::new(dir.path()),
            spec: SpecId::new("spec-A"),
        };
        let s = economy_summary(dir.path(), scope).unwrap();
        // Estimated, NOT measured.
        assert_eq!(s.total_cost_usd_micros, 3000);
        assert_eq!(s.total_tokens, 300);
        assert_eq!(s.span_count, 2);
        assert_eq!(s.by_session.len(), 0); // empty at filtered scope
    }
}
