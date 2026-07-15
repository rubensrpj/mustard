#![forbid(unsafe_code)]
// `clippy::unwrap_used` / `expect_used` are `deny` workspace-wide so the
// fail-open server can never panic on the protocol path. Clippy does not exempt
// `#[cfg(test)]` code, so — matching `mustard-rt`'s `main.rs` and
// `mustard-core`'s `lib.rs` — the carve-out is applied explicitly: under
// `cfg(test)`, `.unwrap()` / `.expect()` are allowed (a panicking assertion
// *is* a test failure). Non-test code keeps the `deny`.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
    )
)]
//! The `mustard-mcp` binary — the Rust re-port of `mustard-memory`.
//!
//! Extracted from `mustard-rt` into its own crate so the long-lived MCP server
//! Claude Code spawns holds `mustard-mcp.exe`, not `mustard-rt.exe` — decoupling
//! it from `mustard-rt` rebuilds. `mustard-rt mcp` is kept as a thin compat
//! alias that delegates to [`run`].
//!
//! `mustard-rt on` / `check` are the enforcement faces (stdin-driven), and
//! `mustard-rt run` ports the standalone utility scripts. This fourth face,
//! `mustard-rt mcp`, ports the last non-Rust runtime artifact: the
//! `mustard-memory` MCP server that used to be a TypeScript program spawned
//! by `bun` (`apps/cli/src/mcp/mustard-memory.ts`).
//!
//! It is a Model Context Protocol server speaking JSON-RPC over stdio. It is
//! **read-only by design**: writes happen in the hooks, where session / wave /
//! spec attribution is authentic; the MCP face exposes queries only. It
//! exposes five tools (the same five as the TypeScript original), with the
//! same input schemas and output shapes:
//!
//! - `search_knowledge`   — substring search over `.claude/knowledge/*.md`.
//! - `query_events`       — filter the per-spec NDJSON event log by spec /
//!   event / since.
//! - `find_similar_specs` — rank specs by token overlap on a description.
//! - `get_spec_metrics`   — projected metrics for a spec from NDJSON events.
//! - `get_run_summary`    — aggregated token/duration totals from
//!   `pipeline.telemetry.run` events.
//!
//! ## Persistence (post-W5B)
//!
//! No SQLite. Every read is filesystem-backed:
//!
//! - knowledge → `.claude/knowledge/*.md` via [`mustard_core::io::atomic_md::MarkdownStore`].
//! - events    → `.claude/spec/<spec>/.events/*.ndjson` via [`mustard_core::EventReader`].
//! - specs     → `.claude/spec/<spec>/spec.md` header walk (name + body).
//! - metrics   → projected from events via the same NDJSON channel.
//! - runs      → `pipeline.telemetry.run` events written by W5A's OTEL collector.
//!
//! ## Runtime scoping
//!
//! `rmcp` is async-only, so the `mcp` face needs a `tokio` runtime — but the
//! `on` / `run` / `check` faces stay fully synchronous. [`run`] therefore
//! builds a **`current_thread`** runtime locally, inside the handler, rather
//! than annotating `main` with `#[tokio::main]`. `tokio` never touches the
//! enforcement path.
//!
//! ## Fail-open
//!
//! Every tool degrades a read failure to an empty result, matching the rest
//! of the `mustard-rt` codebase: telemetry is never load-bearing.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo,
};
use rmcp::{
    ServerHandler, ServiceExt, schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

use mustard_core::io::atomic_md::{MarkdownDoc, MarkdownStore};
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use mustard_core::{Event, EventReader};

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run the `mcp` face: serve the `mustard-memory` MCP server over stdio.
///
/// Builds a local `current_thread` `tokio` runtime, registers the five tools,
/// and serves JSON-RPC over stdin/stdout until the transport closes. Diagnostics
/// go to **stderr only** — stdout is reserved for the MCP protocol channel.
pub fn run() {
    // A `current_thread` runtime keeps `tokio` scoped to this face: no worker
    // threads, no global runtime. Building it can only fail on a catastrophic
    // OS resource shortage; treat that as fail-soft (exit, log to stderr).
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            eprintln!("[mustard-memory] failed to build tokio runtime: {err}");
            return;
        }
    };

    runtime.block_on(async {
        let server = MustardMemory::new(resolve_project_dir());
        // `serve` performs the MCP `initialize` handshake; `waiting` blocks
        // until the peer disconnects. A serve error is logged to stderr and
        // ends the process cleanly — never a panic on the protocol path.
        match server.serve(rmcp::transport::stdio()).await {
            Ok(service) => {
                if let Err(err) = service.waiting().await {
                    eprintln!("[mustard-memory] service error: {err}");
                }
            }
            Err(err) => {
                eprintln!("[mustard-memory] failed to start MCP server: {err}");
            }
        }
    });
}

/// Resolve the project root the server reads from. The harness layout under
/// `.claude/` is rooted at the process cwd.
fn resolve_project_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

// ---------------------------------------------------------------------------
// Tool input schemas — 1:1 with the TypeScript `zod` schemas
// ---------------------------------------------------------------------------

/// Input for `search_knowledge`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchKnowledgeArgs {
    /// Free-text query (non-empty). Substring match, case-insensitive.
    query: String,
    /// Optional knowledge-kind filter: `pattern`, `convention`, or `entity`.
    #[serde(default)]
    r#type: Option<String>,
    /// Maximum rows to return (`1..=50`, default `10`).
    #[serde(default)]
    limit: Option<u32>,
}

/// Input for `query_events`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct QueryEventsArgs {
    /// Optional spec filter.
    #[serde(default)]
    spec: Option<String>,
    /// Optional event-name filter.
    #[serde(default)]
    event: Option<String>,
    /// Optional ISO-8601 lower bound on the event timestamp (`ts >= since`).
    #[serde(default)]
    since: Option<String>,
    /// Maximum rows to return (`1..=500`, default `100`).
    #[serde(default)]
    limit: Option<u32>,
}

/// Input for `find_similar_specs`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FindSimilarSpecsArgs {
    /// Free-text description scored against each spec.
    description: String,
    /// Maximum rows to return (`1..=20`, default `5`).
    #[serde(default)]
    limit: Option<u32>,
}

/// Input for `get_spec_metrics`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetSpecMetricsArgs {
    /// Spec name (non-empty).
    spec: String,
}

/// Input for `get_run_summary`.
///
/// `phase` is now WIRED: when set, the tool delegates to the core
/// `per_phase_token_summary` reader, which correlates the phase-less OTEL token
/// metric channel against the `pipeline.phase` transition timeline and returns
/// only the requested phase's input/output token totals (per-model breakdown is
/// not available at phase granularity, so `byModel` is empty under a phase
/// filter). The reader aggregates the full datapoint set rather than capping
/// rows.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetRunSummaryArgs {
    /// Optional spec filter.
    #[serde(default)]
    spec: Option<String>,
    /// Optional pipeline-phase filter (e.g. `"EXECUTE"`). When set, the totals
    /// are narrowed to tokens attributed to that phase by timestamp correlation.
    #[serde(default)]
    phase: Option<String>,
}


// ---------------------------------------------------------------------------
// Output shapes — serialized to JSON text exactly like the TS `jsonResult`
// ---------------------------------------------------------------------------

/// One knowledge row in `search_knowledge` output.
#[derive(Debug, Serialize)]
struct KnowledgeOut {
    id: String,
    r#type: Option<String>,
    name: Option<String>,
    description: Option<String>,
    confidence: Option<f64>,
}

/// One event row in `query_events` output. Mirrors the TS `EventRecord`.
#[derive(Debug, Serialize)]
struct EventOut {
    ts: String,
    event: String,
    payload: Value,
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wave: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    spec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    actor: Option<Value>,
}

/// One spec row in `find_similar_specs` output.
#[derive(Debug, Serialize)]
struct SpecOut {
    name: String,
    status: Option<String>,
    phase: Option<String>,
    #[serde(rename = "startedAt", skip_serializing_if = "Option::is_none")]
    started_at: Option<String>,
    #[serde(rename = "completedAt", skip_serializing_if = "Option::is_none")]
    completed_at: Option<String>,
    #[serde(rename = "affectedFiles", skip_serializing_if = "Option::is_none")]
    affected_files: Option<Vec<String>>,
}

/// A scored spec match in `find_similar_specs` output.
#[derive(Debug, Serialize)]
struct SpecMatch {
    spec: SpecOut,
    score: u32,
}

/// The `metrics_projection` row in `get_spec_metrics` output.
#[derive(Debug, Serialize)]
struct MetricsOut {
    spec: String,
    #[serde(rename = "apiCalls")]
    api_calls: i64,
    retries: i64,
    pass1: bool,
    #[serde(rename = "toolBreakdown")]
    tool_breakdown: Value,
    #[serde(rename = "dispatchFailuresByPhase")]
    dispatch_failures_by_phase: Value,
    #[serde(rename = "agentCount")]
    agent_count: i64,
    #[serde(rename = "updatedAt")]
    updated_at: String,
}

/// Per-model aggregate bucket in `get_run_summary` output.
#[derive(Debug, Default, Serialize)]
struct ModelBucket {
    count: u64,
    r#in: i64,
    out: i64,
    #[serde(rename = "durationMs")]
    duration_ms: i64,
}

/// Aggregated `get_run_summary` output.
#[derive(Debug, Serialize)]
struct RunSummary {
    count: usize,
    #[serde(rename = "totalInputTokens")]
    total_input_tokens: i64,
    #[serde(rename = "totalOutputTokens")]
    total_output_tokens: i64,
    #[serde(rename = "totalDurationMs")]
    total_duration_ms: i64,
    #[serde(rename = "byModel")]
    by_model: Map<String, Value>,
}

// ---------------------------------------------------------------------------
// The MCP server
// ---------------------------------------------------------------------------

/// The `mustard-memory` MCP server — a read-only view over the filesystem-
/// backed harness state.
#[derive(Clone)]
struct MustardMemory {
    /// Project root; resolved to `.claude/` on each open.
    project_dir: PathBuf,
}

/// Wrap a value as the MCP `CallToolResult` carrying its pretty JSON text.
///
/// The TypeScript original returned `{ content: [{ type: 'text', text: ... }] }`
/// with `JSON.stringify(data, null, 2)`; this is the byte-for-byte equivalent.
fn json_result<T: Serialize>(data: &T) -> CallToolResult {
    let text = serde_json::to_string_pretty(data)
        .unwrap_or_else(|_| "null".to_string());
    CallToolResult::success(vec![Content::text(text)])
}

#[tool_router]
impl MustardMemory {
    /// Construct the server for a project root.
    fn new(project_dir: PathBuf) -> Self {
        Self {
            project_dir,
        }
    }

    /// Resolve the canonical `.claude/` paths for this project, fail-open.
    fn claude_paths(&self) -> Option<ClaudePaths> {
        ClaudePaths::for_project(&self.project_dir).ok()
    }

    /// Tool 1 — substring search past learnings / decisions / patterns.
    ///
    /// Reads `.claude/knowledge/*.md` via `MarkdownStore::scan_dir`. The
    /// optional `type` filter narrows by the frontmatter `kind` field. The
    /// substring match is case-insensitive over `name + description + body`.
    #[tool(
        description = "Substring search past learnings/decisions/patterns from .claude/knowledge/*.md"
    )]
    fn search_knowledge(
        &self,
        Parameters(args): Parameters<SearchKnowledgeArgs>,
    ) -> CallToolResult {
        let limit = args.limit.unwrap_or(10).clamp(1, 50) as usize;
        let Some(paths) = self.claude_paths() else {
            return json_result(&Vec::<KnowledgeOut>::new());
        };
        let knowledge_dir = paths.claude_dir().join("knowledge");
        let docs = MarkdownStore::scan_dir(&knowledge_dir);
        let needle = args.query.to_lowercase();
        let type_filter = args.r#type.as_deref();
        let mut hits: Vec<(usize, KnowledgeOut)> = docs
            .into_iter()
            .filter_map(|doc| {
                let row = doc_to_knowledge_out(&doc);
                if let Some(t) = type_filter {
                    if row.r#type.as_deref() != Some(t) {
                        return None;
                    }
                }
                let hay = format!(
                    "{} {} {}",
                    row.name.as_deref().unwrap_or(""),
                    row.description.as_deref().unwrap_or(""),
                    doc.body,
                )
                .to_lowercase();
                if !hay.contains(&needle) {
                    return None;
                }
                let score = hay.matches(&needle).count();
                Some((score, row))
            })
            .collect();
        hits.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        let rows: Vec<KnowledgeOut> = hits.into_iter().take(limit).map(|(_, r)| r).collect();
        json_result(&rows)
    }

    /// Tool 2 — filter events by spec / event / since across the per-spec
    /// NDJSON event log.
    #[tool(
        description = "Filter events by spec/event/since (ISO ts). Returns up to `limit` rows."
    )]
    fn query_events(
        &self,
        Parameters(args): Parameters<QueryEventsArgs>,
    ) -> CallToolResult {
        let limit = args.limit.unwrap_or(100).clamp(1, 500) as usize;
        let Some(paths) = self.claude_paths() else {
            return json_result(&Vec::<EventOut>::new());
        };
        let specs_root = paths.spec_dir();

        let mut events: Vec<Event> = Vec::new();
        if let Some(spec) = args.spec.as_deref() {
            collect_ndjson_under(&specs_root.join(spec).join(".events"), &mut events);
        } else if let Ok(entries) = fs::read_dir(&specs_root) {
            for entry in entries {
                if !entry.path.is_dir() {
                    continue;
                }
                collect_ndjson_under(&entry.path.join(".events"), &mut events);
            }
        }

        // Chronological sort so the lexical `since` comparison stays correct.
        events.sort_by_key(event_ts);

        let rows: Vec<EventOut> = events
            .into_iter()
            .filter_map(|ev| {
                let out = event_to_out(ev)?;
                if let Some(s) = args.spec.as_deref() {
                    if out.spec.as_deref() != Some(s) {
                        return None;
                    }
                }
                if let Some(e) = args.event.as_deref() {
                    if out.event != e {
                        return None;
                    }
                }
                if let Some(since) = args.since.as_deref() {
                    if out.ts.as_str() < since {
                        return None;
                    }
                }
                Some(out)
            })
            .take(limit)
            .collect();
        json_result(&rows)
    }

    /// Tool 3 — rank specs by token overlap against a free-text description.
    ///
    /// Walks `.claude/spec/*/spec.md` (filesystem) and scores each spec on
    /// lowercased token overlap against `name + body` (the body is read once
    /// per spec; this is intended for interactive `mcp` use, not hot paths).
    #[tool(
        description = "Rank specs by token overlap against a free-text description (name + body)"
    )]
    fn find_similar_specs(
        &self,
        Parameters(args): Parameters<FindSimilarSpecsArgs>,
    ) -> CallToolResult {
        let limit = args.limit.unwrap_or(5).clamp(1, 20) as usize;
        let tokens: Vec<String> = args
            .description
            .to_lowercase()
            .split_whitespace()
            .map(str::to_string)
            .collect();
        if tokens.is_empty() {
            return json_result(&Vec::<SpecMatch>::new());
        }
        let Some(paths) = self.claude_paths() else {
            return json_result(&Vec::<SpecMatch>::new());
        };
        let specs_root = paths.spec_dir();
        let mut matches: Vec<SpecMatch> = Vec::new();
        let Ok(entries) = fs::read_dir(&specs_root) else {
            return json_result(&matches);
        };
        for entry in entries {
            let path = entry.path;
            if !path.is_dir() {
                continue;
            }
            let Some(name_os) = path.file_name() else { continue };
            let name = name_os.to_string_lossy().to_string();
            let spec_md = path.join("spec.md");
            let body = fs::read_to_string(&spec_md).unwrap_or_default();
            let haystack = format!("{name} {body}").to_lowercase();
            let score = tokens
                .iter()
                .filter(|tok| haystack.contains(tok.as_str()))
                .count() as u32;
            if score == 0 {
                continue;
            }
            matches.push(SpecMatch {
                spec: SpecOut {
                    name,
                    status: None,
                    phase: None,
                    started_at: None,
                    completed_at: None,
                    affected_files: None,
                },
                score,
            });
        }
        matches.sort_by_key(|m| std::cmp::Reverse(m.score));
        matches.truncate(limit);
        json_result(&matches)
    }

    /// Tool 4 — projected metrics for a spec, or `{ error }`.
    ///
    /// Reads `.claude/spec/<spec>/.events/*.ndjson` and derives a minimal
    /// metrics shape (api_calls / retries / agent_count) by counting events
    /// of the relevant kinds. Returns `{ error, spec }` when no events exist.
    #[tool(description = "Return the metrics projection for a spec, or { error } if missing")]
    fn get_spec_metrics(
        &self,
        Parameters(args): Parameters<GetSpecMetricsArgs>,
    ) -> CallToolResult {
        let Some(paths) = self.claude_paths() else {
            return json_result(&missing_metrics(&args.spec));
        };
        let events_dir = paths.spec_dir().join(&args.spec).join(".events");
        let mut events: Vec<Event> = Vec::new();
        collect_ndjson_under(&events_dir, &mut events);
        if events.is_empty() {
            return json_result(&missing_metrics(&args.spec));
        }
        let metrics = derive_metrics(&args.spec, &events);
        json_result(&metrics)
    }


    /// Tool 5 — aggregated token summary from the MEASURED OTEL token channel,
    /// grouped by model.
    ///
    /// Delegates to the canonical core reader
    /// [`mustard_core::domain::economy::metric_token_summary`], which folds the
    /// `pipeline.telemetry.metric` / `claude_code.token.usage` datapoints (the
    /// only place the real billed token counts live, split by `token_type`).
    /// The tool previously hand-rolled a filter over `pipeline.telemetry.run`
    /// events, which carry no token datapoints — so it always reported zero.
    /// The output JSON shape (count / totalInputTokens /
    /// totalOutputTokens / totalDurationMs / byModel) is unchanged; only the
    /// data source moved to the source of truth.
    #[tool(description = "Aggregated token summary from the OTEL token-usage metric channel; groups by model, or narrows to one pipeline phase when `phase` is set")]
    fn get_run_summary(
        &self,
        Parameters(args): Parameters<GetRunSummaryArgs>,
    ) -> CallToolResult {
        let scope = run_summary_scope(&self.project_dir, args.spec.as_deref());
        // `phase` set → correlate the phase-less metric channel against the
        // `pipeline.phase` timeline and return ONLY that phase's totals.
        if let Some(phase) = args.phase.as_deref().filter(|s| !s.is_empty()) {
            let per_phase = mustard_core::domain::economy::per_phase_token_summary(
                &self.project_dir,
                scope,
            )
            .unwrap_or_default();
            return json_result(&run_summary_for_phase(&per_phase, phase));
        }
        let summary = mustard_core::domain::economy::metric_token_summary(
            &self.project_dir,
            scope,
        )
        .unwrap_or_default();
        json_result(&run_summary_from_metrics(&summary))
    }
}

/// Build the [`EconomyScope`] for a `get_run_summary` call. A `spec` filter
/// maps to [`EconomyScope::Spec`]; absent, the unfiltered project scope (which
/// is the only scope under which the metric channel reports anything, since the
/// datapoints carry no spec dimension).
fn run_summary_scope(
    project_dir: &Path,
    spec: Option<&str>,
) -> mustard_core::domain::economy::EconomyScope {
    use mustard_core::domain::economy::scope::{EconomyScope, ProjectPath, SpecId};
    match spec {
        Some(s) => EconomyScope::Spec {
            project: ProjectPath::new(project_dir),
            spec: SpecId::new(s),
        },
        None => EconomyScope::Project(ProjectPath::new(project_dir)),
    }
}

/// Map a core [`MetricTokenSummary`] onto the MCP `RunSummary` output shape.
///
/// `count` mirrors the contributing datapoint count, and `byModel` is keyed by
/// model name with the same `{count, in, out, durationMs}` bucket shape as
/// before. `durationMs` is always `0` — the OTEL token channel carries token
/// counts only, no span duration — preserving the field for shape stability.
fn run_summary_from_metrics(
    summary: &mustard_core::domain::economy::MetricTokenSummary,
) -> RunSummary {
    let mut by_model: Map<String, Value> = Map::new();
    for b in &summary.by_model {
        let bucket = ModelBucket {
            count: u64::try_from(b.datapoint_count).unwrap_or(0),
            r#in: b.input_tokens,
            out: b.output_tokens,
            duration_ms: 0,
        };
        if let Ok(value) = serde_json::to_value(&bucket) {
            by_model.insert(b.model.clone(), value);
        }
    }
    RunSummary {
        count: usize::try_from(summary.datapoint_count).unwrap_or(0),
        total_input_tokens: summary.input_tokens,
        total_output_tokens: summary.output_tokens,
        total_duration_ms: 0,
        by_model,
    }
}

/// Map a [`PerPhaseTokenSummary`] onto the MCP `RunSummary` output, narrowed to
/// a single `phase`. Returns the matching phase bucket's totals, or an empty
/// summary when the phase has no attributed tokens. `byModel` is empty:
/// per-phase attribution is timestamp-correlated against the metric channel,
/// which carries no model split at phase granularity.
fn run_summary_for_phase(
    summary: &mustard_core::domain::economy::PerPhaseTokenSummary,
    phase: &str,
) -> RunSummary {
    let bucket = summary.by_phase.iter().find(|b| b.phase == phase);
    RunSummary {
        count: bucket.map_or(0, |b| usize::try_from(b.datapoint_count).unwrap_or(0)),
        total_input_tokens: bucket.map_or(0, |b| b.input_tokens),
        total_output_tokens: bucket.map_or(0, |b| b.output_tokens),
        total_duration_ms: 0,
        by_model: Map::new(),
    }
}

#[tool_handler]
impl ServerHandler for MustardMemory {
    /// Advertise the server identity and the single declared capability
    /// (tools). The name / version match the TypeScript original.
    ///
    /// `ServerInfo` and `Implementation` are both `#[non_exhaustive]`, so
    /// they cannot be built with a struct literal: start from `default()` /
    /// `from_build_env()` and mutate the individual public fields.
    fn get_info(&self) -> ServerInfo {
        let mut server_info = Implementation::from_build_env();
        server_info.name = "mustard-memory".to_string();
        server_info.version = "2.0.0".to_string();

        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = server_info;
        info.instructions = Some(
            "Read-only query access to the Mustard harness state \
             (events, knowledge, specs, metrics, runs) backed by .claude/."
                .to_string(),
        );
        info
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Convert a `MarkdownDoc` to a `KnowledgeOut` row.
fn doc_to_knowledge_out(doc: &MarkdownDoc) -> KnowledgeOut {
    let fm = doc.frontmatter.as_ref();
    let id = doc
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    KnowledgeOut {
        id,
        r#type: fm
            .and_then(|f| f.get("kind"))
            .and_then(Value::as_str)
            .map(str::to_string),
        name: fm
            .and_then(|f| f.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string),
        description: fm
            .and_then(|f| f.get("description"))
            .and_then(Value::as_str)
            .map(str::to_string),
        confidence: fm
            .and_then(|f| f.get("confidence"))
            .and_then(Value::as_f64),
    }
}

/// Recursively collect `.ndjson` files under `dir` into `out`. Fail-open: a
/// missing directory or unreadable file is silently skipped.
fn collect_ndjson_under(dir: &Path, out: &mut Vec<Event>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd {
        let path = entry.path;
        if path.is_dir() {
            collect_ndjson_under(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("ndjson") {
            out.extend(EventReader::stream(&path));
        }
    }
}

/// Pull the top-level `event` name off a raw NDJSON record.
fn event_name(ev: &Event) -> &str {
    ev.raw.get("event").and_then(Value::as_str).unwrap_or("")
}

/// Pull the top-level `ts` off a raw NDJSON record (ISO-8601).
fn event_ts(ev: &Event) -> String {
    ev.raw
        .get("ts")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Project an `Event` to the MCP-output `EventOut` shape.
fn event_to_out(ev: Event) -> Option<EventOut> {
    let raw = &ev.raw;
    let ts = raw.get("ts").and_then(Value::as_str)?.to_string();
    let event = raw.get("event").and_then(Value::as_str)?.to_string();
    let session_id = raw
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let wave = raw
        .get("wave")
        .and_then(Value::as_u64)
        .map(|n| u32::try_from(n).unwrap_or(0))
        .filter(|n| *n != 0);
    let spec = raw.get("spec").and_then(Value::as_str).map(str::to_string);
    let actor = raw.get("actor").cloned();
    Some(EventOut {
        ts,
        event,
        payload: ev.payload,
        session_id,
        wave,
        spec,
        actor,
    })
}

/// Derive a minimal `MetricsOut` from the spec's events.
fn derive_metrics(spec: &str, events: &[Event]) -> MetricsOut {
    let api_calls = events
        .iter()
        .filter(|e| event_name(e) == "tool.use")
        .count() as i64;
    let retries = events
        .iter()
        .filter(|e| event_name(e) == "retry.attempt")
        .count() as i64;
    let agent_count = events
        .iter()
        .filter(|e| event_name(e) == "pipeline.task.dispatch")
        .count() as i64;
    let updated_at = events
        .iter()
        .filter_map(|e| e.raw.get("ts").and_then(Value::as_str))
        .max()
        .unwrap_or("")
        .to_string();
    MetricsOut {
        spec: spec.to_string(),
        api_calls,
        retries,
        pass1: retries == 0,
        tool_breakdown: json!({}),
        dispatch_failures_by_phase: json!({}),
        agent_count,
        updated_at,
    }
}

/// The `{ error, spec }` object `get_spec_metrics` returns when no row exists.
fn missing_metrics(spec: &str) -> Value {
    json!({ "error": "no metrics for spec", "spec": spec })
}


// ---------------------------------------------------------------------------
// Tests — `get_run_summary` consolidation onto the core economy reader
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::economy::{metric_token_summary, EconomyScope, ProjectPath};
    use std::fs;

    /// Plant cross-spec session metric rows at
    /// `<root>/.claude/.session/<id>/.events/seed.ndjson`. This mirrors where
    /// the OTEL collector writes token datapoints (the cross-session sink).
    fn plant_session_metrics(root: &Path, id: &str, lines: &[String]) {
        let dir = root
            .join(".claude")
            .join(".session")
            .join(id)
            .join(".events");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("seed.ndjson"), lines.join("\n")).unwrap();
    }

    /// A `claude_code.token.usage` metric NDJSON line for one model + type.
    fn token_metric_line(model: &str, token_type: &str, sum: i64) -> String {
        json!({
            "kind": "pipeline.telemetry.metric",
            "event": "pipeline.telemetry.metric",
            "payload": {
                "metric": "claude_code.token.usage",
                "session_id": "sess-1",
                "model": model,
                "token_type": token_type,
                "sum": sum,
            }
        })
        .to_string()
    }

    /// AC1: a fixture of `pipeline.telemetry.metric` token datapoints (input /
    /// output / cacheRead / cacheCreation) for one model must surface nonzero
    /// token totals grouped by that model. Before the fix the tool filtered on
    /// `pipeline.telemetry.run` and reported zero.
    #[test]
    fn run_summary_includes_metric_events() {
        let dir = tempfile::tempdir().unwrap();
        plant_session_metrics(
            dir.path(),
            "sess-1",
            &[
                token_metric_line("opus", "input", 100),
                token_metric_line("opus", "output", 40),
                token_metric_line("opus", "cacheRead", 1000),
                token_metric_line("opus", "cacheCreation", 7),
            ],
        );

        let summary = metric_token_summary(
            dir.path(),
            EconomyScope::Project(ProjectPath::new(dir.path())),
        )
        .unwrap();
        let out = run_summary_from_metrics(&summary);

        // input + cacheRead + cacheCreation = 100 + 1000 + 7 = 1107; output = 40.
        assert_eq!(out.total_input_tokens, 1107);
        assert_eq!(out.total_output_tokens, 40);
        assert_eq!(out.count, 4);
        // Grouped by model — the single "opus" bucket carries the same totals.
        let opus = out.by_model.get("opus").expect("opus bucket present");
        assert_eq!(opus["in"].as_i64(), Some(1107));
        assert_eq!(opus["out"].as_i64(), Some(40));
        assert_eq!(opus["count"].as_u64(), Some(4));
        // No span duration on the token channel — the field stays present at 0.
        assert_eq!(out.total_duration_ms, 0);
        assert_eq!(opus["durationMs"].as_i64(), Some(0));
    }

    /// AC2: the MCP summary totals must be consistent with calling the core
    /// `economy_summary`/`metric_token_summary` reader directly on the same
    /// fixture — the tool is a thin mapping over the source of truth, not a
    /// reimplementation.
    #[test]
    fn run_summary_matches_core_economy() {
        let dir = tempfile::tempdir().unwrap();
        plant_session_metrics(
            dir.path(),
            "sess-1",
            &[
                token_metric_line("opus", "input", 200),
                token_metric_line("opus", "output", 50),
                token_metric_line("sonnet", "input", 30),
                token_metric_line("sonnet", "output", 10),
            ],
        );

        let core = metric_token_summary(
            dir.path(),
            EconomyScope::Project(ProjectPath::new(dir.path())),
        )
        .unwrap();
        let out = run_summary_from_metrics(&core);

        // Totals match the core aggregate exactly.
        assert_eq!(out.total_input_tokens, core.input_tokens);
        assert_eq!(out.total_output_tokens, core.output_tokens);
        assert_eq!(out.count as i64, core.datapoint_count);
        // Per-model split matches too — two distinct models surface.
        assert_eq!(out.by_model.len(), core.by_model.len());
        assert_eq!(out.by_model.len(), 2);
        let opus = out.by_model.get("opus").expect("opus bucket");
        assert_eq!(opus["in"].as_i64(), Some(200));
        assert_eq!(opus["out"].as_i64(), Some(50));
    }

    /// A spec-scoped query cannot match the phase/spec-less metric channel, so
    /// it yields an all-zero summary rather than crashing — preserving the
    /// fail-empty contract for filters that have no matching datapoints.
    #[test]
    fn run_summary_spec_scope_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        plant_session_metrics(
            dir.path(),
            "sess-1",
            &[token_metric_line("opus", "input", 100)],
        );
        let core = metric_token_summary(
            dir.path(),
            run_summary_scope(dir.path(), Some("some-spec")),
        )
        .unwrap();
        let out = run_summary_from_metrics(&core);
        assert_eq!(out.count, 0);
        assert_eq!(out.total_input_tokens, 0);
        assert!(out.by_model.is_empty());
    }

}

