//! The `mcp` face of `mustard-rt` — the Rust re-port of `mustard-memory`.
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
//! exposes exactly the same five tools as the TypeScript original, with the
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

use rmcp::handler::server::router::tool::ToolRouter;
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
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetRunSummaryArgs {
    /// Optional spec filter.
    #[serde(default)]
    spec: Option<String>,
    /// Optional pipeline-phase filter.
    #[serde(default)]
    phase: Option<String>,
    /// Maximum runs to aggregate (`1..=5000`, default `1000`).
    #[serde(default)]
    limit: Option<u32>,
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
    /// The `rmcp` tool dispatch table, generated by `#[tool_router]`.
    ///
    /// `#[allow(dead_code)]`: the field *is* consumed — by the `call_tool`
    /// dispatch the `#[tool_handler]` macro generates — but that use sits in
    /// macro-expanded code the dead-code pass does not attribute back here.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
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
            tool_router: Self::tool_router(),
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
        } else if let Ok(entries) = std::fs::read_dir(&specs_root) {
            for entry in entries.flatten() {
                if !entry.path().is_dir() {
                    continue;
                }
                collect_ndjson_under(&entry.path().join(".events"), &mut events);
            }
        }

        // Chronological sort so the lexical `since` comparison stays correct.
        events.sort_by(|a, b| event_ts(a).cmp(&event_ts(b)));

        let rows: Vec<EventOut> = events
            .into_iter()
            // Internal meta-telemetry emitted by the NDJSON writer for the
            // `/economia` dashboard — filter it out at the boundary.
            .filter(|ev| event_name(ev) != "pipeline.economy.event.written")
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
        let Ok(entries) = std::fs::read_dir(&specs_root) else {
            return json_result(&matches);
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name_os) = path.file_name() else { continue };
            let name = name_os.to_string_lossy().to_string();
            let spec_md = path.join("spec.md");
            let body = std::fs::read_to_string(&spec_md).unwrap_or_default();
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

    /// Tool 5 — aggregated token/duration summary from `pipeline.telemetry.run`
    /// NDJSON events (written by the W5A OTEL collector).
    #[tool(description = "Aggregated token/duration summary from pipeline.telemetry.run events; groups by model")]
    fn get_run_summary(
        &self,
        Parameters(args): Parameters<GetRunSummaryArgs>,
    ) -> CallToolResult {
        let limit = args.limit.unwrap_or(1000).clamp(1, 5000) as usize;
        let Some(paths) = self.claude_paths() else {
            return json_result(&empty_run_summary());
        };
        // Cross-spec walk: include both per-spec and the cross-session sink.
        let mut events: Vec<Event> = Vec::new();
        collect_ndjson_under(&paths.spec_dir(), &mut events);
        collect_ndjson_under(&paths.claude_dir().join(".session"), &mut events);

        let runs: Vec<&Event> = events
            .iter()
            .filter(|e| e.kind == "pipeline.telemetry.run")
            .filter(|e| match args.spec.as_deref() {
                Some(s) => e.payload.get("spec").and_then(Value::as_str) == Some(s),
                None => true,
            })
            .filter(|e| match args.phase.as_deref() {
                Some(p) => e.payload.get("phase").and_then(Value::as_str) == Some(p),
                None => true,
            })
            .take(limit)
            .collect();
        json_result(&summarize_runs(&runs))
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
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
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

/// Aggregate `pipeline.telemetry.run` event payloads into the
/// `get_run_summary` output shape.
fn summarize_runs(runs: &[&Event]) -> RunSummary {
    let mut by_model: Map<String, Value> = Map::new();
    let mut buckets: std::collections::BTreeMap<String, ModelBucket> =
        std::collections::BTreeMap::new();
    let mut total_input = 0_i64;
    let mut total_output = 0_i64;
    let mut total_duration = 0_i64;

    for run in runs {
        let p = &run.payload;
        let input = p.get("input_tokens").and_then(Value::as_i64).unwrap_or(0);
        let output = p.get("output_tokens").and_then(Value::as_i64).unwrap_or(0);
        let duration = p.get("duration_ms").and_then(Value::as_i64).unwrap_or(0);
        total_input += input;
        total_output += output;
        total_duration += duration;

        let model = p
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| "unknown".to_string());
        let bucket = buckets.entry(model).or_default();
        bucket.count += 1;
        bucket.r#in += input;
        bucket.out += output;
        bucket.duration_ms += duration;
    }

    for (model, bucket) in buckets {
        if let Ok(value) = serde_json::to_value(&bucket) {
            by_model.insert(model, value);
        }
    }

    RunSummary {
        count: runs.len(),
        total_input_tokens: total_input,
        total_output_tokens: total_output,
        total_duration_ms: total_duration,
        by_model,
    }
}

/// The `get_run_summary` output for an empty / unavailable run set.
fn empty_run_summary() -> RunSummary {
    summarize_runs(&[])
}
