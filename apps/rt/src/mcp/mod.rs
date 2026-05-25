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
//! - `search_knowledge`   — FTS5 search over the `knowledge` table.
//! - `query_events`       — filter the event log by spec / event / since.
//! - `find_similar_specs` — rank specs by token overlap on a description.
//! - `get_spec_metrics`   — the `metrics_projection` row for a spec.
//! - `get_run_summary`    — aggregated token / duration totals from `run_usage`.
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
//! Every tool opens the store fresh and degrades a query failure to an empty
//! result (or an `{ "error": ... }` object), matching the rest of the
//! `mustard-rt` codebase: telemetry is never load-bearing.

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
use std::path::PathBuf;

use mustard_core::store::sqlite_store::{
    KnowledgeRow, MetricsRow, SpecRow, SqliteEventStore,
};
use mustard_core::model::event::HarnessEvent;
use mustard_core::telemetry::{SummaryRow, TelemetryReader, TelemetryStore};

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

/// Resolve the project root whose `.claude/.harness/mustard.db` the server
/// reads. [`SqliteEventStore::for_project`] applies the `MUSTARD_DB_PATH`
/// override on top of this, so the current directory is the right default.
fn resolve_project_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

// ---------------------------------------------------------------------------
// Tool input schemas — 1:1 with the TypeScript `zod` schemas
// ---------------------------------------------------------------------------

/// Input for `search_knowledge` (mirrors the TS `zod` schema).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchKnowledgeArgs {
    /// Free-text FTS5 query (non-empty).
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

/// One knowledge row in `search_knowledge` output. Mirrors the TS object
/// (`id`, `type`, `name`, `description`, `confidence`).
#[derive(Debug, Serialize)]
struct KnowledgeOut {
    id: String,
    r#type: Option<String>,
    name: Option<String>,
    description: Option<String>,
    confidence: Option<f64>,
}

impl From<KnowledgeRow> for KnowledgeOut {
    fn from(row: KnowledgeRow) -> Self {
        Self {
            id: row.id,
            r#type: row.kind,
            name: row.name,
            description: row.description,
            confidence: row.confidence,
        }
    }
}

/// One event row in `query_events` output. Mirrors the TS `EventRecord`:
/// `ts`, `event`, `payload`, plus the optional `sessionId` / `wave` / `spec`
/// / `actor` fields when present.
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

impl From<HarnessEvent> for EventOut {
    fn from(ev: HarnessEvent) -> Self {
        // The TS `rowToEvent` omits `sessionId`/`wave` when the column was
        // NULL. Rust decodes those to `""`/`0`; treat the empty/zero defaults
        // as "absent" so the JSON shape matches the original.
        let session_id = if ev.session_id.is_empty() {
            None
        } else {
            Some(ev.session_id)
        };
        let wave = if ev.wave == 0 { None } else { Some(ev.wave) };
        let actor = serde_json::to_value(&ev.actor).ok();
        Self {
            ts: ev.ts,
            event: ev.event,
            payload: ev.payload,
            session_id,
            wave,
            spec: ev.spec,
            actor,
        }
    }
}

/// One spec row in `find_similar_specs` output. Mirrors the TS `SpecRecord`.
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

impl From<&SpecRow> for SpecOut {
    fn from(row: &SpecRow) -> Self {
        // `affected_files` is stored as a JSON-array string; decode it back to
        // a list (TS `safeJsonParse`) — a malformed value degrades to absent.
        let affected_files = row
            .affected_files
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok());
        Self {
            name: row.name.clone(),
            status: row.status.clone(),
            phase: row.phase.clone(),
            started_at: row.started_at.clone(),
            completed_at: row.completed_at.clone(),
            affected_files,
        }
    }
}

/// A scored spec match in `find_similar_specs` output (`{ spec, score }`).
#[derive(Debug, Serialize)]
struct SpecMatch {
    spec: SpecOut,
    score: u32,
}

/// The `metrics_projection` row in `get_spec_metrics` output. Mirrors the TS
/// `MetricsRecord`.
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

impl From<MetricsRow> for MetricsOut {
    fn from(row: MetricsRow) -> Self {
        // The legacy schema stores `pass1` as 0/1 and the breakdown columns as
        // JSON-object strings; `rowToMetrics` in the TS store decodes both.
        let pass1 = row.pass1.unwrap_or(0) != 0;
        let tool_breakdown = decode_json_object(row.tool_breakdown.as_deref());
        let dispatch_failures_by_phase =
            decode_json_object(row.dispatch_failures_by_phase.as_deref());
        Self {
            spec: row.spec,
            api_calls: row.api_calls.unwrap_or(0),
            retries: row.retries.unwrap_or(0),
            pass1,
            tool_breakdown,
            dispatch_failures_by_phase,
            agent_count: row.agent_count.unwrap_or(0),
            updated_at: row.updated_at.unwrap_or_default(),
        }
    }
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

/// Aggregated `get_run_summary` output. Mirrors the TS object exactly.
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

/// Decode a JSON-object column string back into a [`Value`].
///
/// A `NULL` column or a malformed value degrades to an empty object — the
/// fail-open equivalent of the TS `safeJsonParse(text, {})`.
fn decode_json_object(raw: Option<&str>) -> Value {
    raw.and_then(|text| serde_json::from_str::<Value>(text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

// ---------------------------------------------------------------------------
// The MCP server
// ---------------------------------------------------------------------------

/// The `mustard-memory` MCP server — a read-only view over the harness store.
///
/// Holds only the project directory; each tool opens the [`SqliteEventStore`]
/// fresh and closes it when the call returns. A [`rusqlite::Connection`] is
/// `!Sync`, and re-opening is sub-millisecond, so a per-call open keeps the
/// type `Send + Sync` (required by `rmcp`) without an `Arc<Mutex<…>>`.
#[derive(Clone)]
struct MustardMemory {
    /// Project root; resolved to `.claude/.harness/mustard.db` on each open.
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

    /// Open the harness store for this project, fail-open.
    fn open_store(&self) -> Option<SqliteEventStore> {
        SqliteEventStore::for_project(&self.project_dir).ok()
    }

    /// Open the dedicated telemetry store (`.harness/telemetry.db`) for this
    /// project, fail-open. Backs `get_run_summary` after the telemetry split.
    fn open_telemetry(&self) -> Option<TelemetryStore> {
        TelemetryStore::for_project(&self.project_dir).ok()
    }

    /// Tool 1 — full-text search past learnings / decisions / patterns.
    ///
    /// Backs onto [`SqliteEventStore::search`] (FTS5 `bm25`). The `type`
    /// filter is applied in-process after the MATCH, exactly as the TS
    /// original did — `search` has no SQL-level kind filter, so the rows are
    /// over-fetched and trimmed here.
    #[tool(
        description = "Full-text search past learnings/decisions/patterns from the EventStore knowledge table"
    )]
    fn search_knowledge(
        &self,
        Parameters(args): Parameters<SearchKnowledgeArgs>,
    ) -> CallToolResult {
        // Clamp `limit` to the TS schema bounds (1..=50, default 10).
        let limit = args.limit.unwrap_or(10).clamp(1, 50) as usize;
        let Some(store) = self.open_store() else {
            return json_result(&Vec::<KnowledgeOut>::new());
        };
        // A malformed FTS MATCH expression fails-open to no results.
        let candidates = store.search(&args.query).unwrap_or_default();
        let type_filter = args.r#type.as_deref();
        let rows: Vec<KnowledgeOut> = candidates
            .into_iter()
            .filter(|row: &KnowledgeRow| match type_filter {
                Some(t) => row.kind.as_deref() == Some(t),
                None => true,
            })
            .take(limit)
            .map(KnowledgeOut::from)
            .collect();
        json_result(&rows)
    }

    /// Tool 2 — filter events by spec / event / since.
    ///
    /// W5: events live in two stores — `pipeline_events` (SQLite lifecycle
    /// index) and per-spec NDJSON files (`.claude/spec/<spec>/events/`). This
    /// folds both sources together so MCP consumers see a single timeline.
    /// When `spec` is given, only that spec's NDJSON dir is read; otherwise
    /// every spec dir under `.claude/spec/` contributes.
    #[tool(
        description = "Filter events by spec/event/since (ISO ts). Returns up to `limit` rows."
    )]
    fn query_events(
        &self,
        Parameters(args): Parameters<QueryEventsArgs>,
    ) -> CallToolResult {
        let limit = args.limit.unwrap_or(100).clamp(1, 500) as usize;

        // 1) Lifecycle slice from SQLite.
        let mut events = match self.open_store() {
            Some(store) => store.replay().unwrap_or_default(),
            None => Vec::new(),
        };

        // 2) NDJSON slice — per-spec dirs.
        let specs_root = self.project_dir.join(".claude").join("spec");
        if let Some(spec) = args.spec.as_deref() {
            let dir = specs_root.join(spec).join("events");
            events.extend(
                mustard_core::projection::read_harness_events_from_ndjson_dir(&dir),
            );
        } else if let Ok(entries) = std::fs::read_dir(&specs_root) {
            for entry in entries.flatten() {
                if !entry.path().is_dir() {
                    continue;
                }
                let dir = entry.path().join("events");
                events.extend(
                    mustard_core::projection::read_harness_events_from_ndjson_dir(&dir),
                );
            }
        }

        // Chronological sort so the lexical `since` comparison stays correct.
        events.sort_by(|a, b| a.ts.cmp(&b.ts));

        let rows: Vec<EventOut> = events
            .into_iter()
            // Internal meta-telemetry emitted by the NDJSON writer for the
            // `/economia` dashboard (see `event_writer_ndjson::write_event`'s
            // T5.8 inline emission). Surfaces every other row in `query_events`
            // and is never what a consumer wants — filter it out at the boundary.
            .filter(|ev| ev.event != "pipeline.economy.event.written")
            .filter(|ev| match &args.spec {
                Some(s) => ev.spec.as_deref() == Some(s.as_str()),
                None => true,
            })
            .filter(|ev| match &args.event {
                Some(e) => ev.event == *e,
                None => true,
            })
            .filter(|ev| match &args.since {
                // ISO-8601 timestamps compare lexically per RFC-3339; mirror
                // the legacy SQL `WHERE ts >= ?` semantics.
                Some(since) => ev.ts.as_str() >= since.as_str(),
                None => true,
            })
            .take(limit)
            .map(EventOut::from)
            .collect();
        json_result(&rows)
    }

    /// Tool 3 — rank specs by token overlap against a free-text description.
    ///
    /// Scoring is identical to the TS original: lowercase the description,
    /// split on whitespace, and count how many distinct tokens appear in the
    /// `name + phase + affectedFiles` haystack of each spec.
    #[tool(
        description = "Rank specs by token overlap against a free-text description (name + phase + affectedFiles)"
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
        let Some(store) = self.open_store() else {
            return json_result(&Vec::<SpecMatch>::new());
        };
        let specs = store.specs().unwrap_or_default();
        let mut matches: Vec<SpecMatch> = specs
            .iter()
            .map(|row| {
                let haystack = spec_haystack(row);
                let score = tokens
                    .iter()
                    .filter(|tok| haystack.contains(tok.as_str()))
                    .count() as u32;
                SpecMatch {
                    spec: SpecOut::from(row),
                    score,
                }
            })
            .filter(|m| m.score > 0)
            .collect();
        // Highest score first; the sort is stable so equal scores keep the
        // `specs()` order (alphabetical by name) — deterministic output.
        // `Reverse` gives the descending key without a hand-written closure.
        matches.sort_by_key(|m| std::cmp::Reverse(m.score));
        matches.truncate(limit);
        json_result(&matches)
    }

    /// Tool 4 — the `metrics_projection` row for a spec, or `{ error }`.
    #[tool(description = "Return the metrics_projection row for a spec, or { error } if missing")]
    fn get_spec_metrics(
        &self,
        Parameters(args): Parameters<GetSpecMetricsArgs>,
    ) -> CallToolResult {
        let Some(store) = self.open_store() else {
            return json_result(&missing_metrics(&args.spec));
        };
        match store.metrics(&args.spec).ok().flatten() {
            Some(row) => json_result(&MetricsOut::from(row)),
            None => json_result(&missing_metrics(&args.spec)),
        }
    }

    /// Tool 5 — aggregated token / duration summary from `run_usage`.
    ///
    /// Totals plus a per-model breakdown, matching the TS object exactly. The
    /// data lives in the dedicated telemetry database (`.harness/telemetry.db`,
    /// table `run_usage`), so the spec/phase filter and `limit` cap are pushed
    /// into [`TelemetryReader::runs_for_summary`]; the output shape is unchanged.
    #[tool(description = "Aggregated token/duration summary from run_usage; groups by model")]
    fn get_run_summary(
        &self,
        Parameters(args): Parameters<GetRunSummaryArgs>,
    ) -> CallToolResult {
        let limit = args.limit.unwrap_or(1000).clamp(1, 5000) as usize;
        let Some(store) = self.open_telemetry() else {
            return json_result(&empty_run_summary());
        };
        let rows = store
            .runs_for_summary(args.spec.as_deref(), args.phase.as_deref(), limit)
            .unwrap_or_default();
        json_result(&summarize_runs(&rows))
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
        // `Implementation::from_build_env()` fills in icons / title / website
        // from the crate metadata; override only the protocol-visible name
        // and version so the server identifies as `mustard-memory`.
        let mut server_info = Implementation::from_build_env();
        server_info.name = "mustard-memory".to_string();
        server_info.version = "2.0.0".to_string();

        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = server_info;
        info.instructions = Some(
            "Read-only query access to the Mustard harness store \
             (events, knowledge, specs, metrics, run_usage)."
                .to_string(),
        );
        info
    }
}

// ---------------------------------------------------------------------------
// Free helpers — kept out of the `#[tool_router]` impl so they are plain fns
// ---------------------------------------------------------------------------

/// Build the lowercase haystack a spec is scored against in `find_similar_specs`.
///
/// Mirrors the TS expression `${name} ${phase ?? ''} ${affectedFiles.join(' ')}`.
fn spec_haystack(row: &SpecRow) -> String {
    let mut haystack = row.name.to_lowercase();
    if let Some(phase) = &row.phase {
        haystack.push(' ');
        haystack.push_str(&phase.to_lowercase());
    }
    if let Some(raw) = &row.affected_files {
        if let Ok(files) = serde_json::from_str::<Vec<String>>(raw) {
            for file in files {
                haystack.push(' ');
                haystack.push_str(&file.to_lowercase());
            }
        }
    }
    haystack
}

/// The `{ error, spec }` object `get_spec_metrics` returns when no row exists.
fn missing_metrics(spec: &str) -> Value {
    json!({ "error": "no metrics for spec", "spec": spec })
}

/// Aggregate `run_usage` summary rows into the `get_run_summary` output shape.
///
/// The per-model breakdown and the four totals are computed from the
/// [`SummaryRow`]s the telemetry reader returns for `run_usage`.
fn summarize_runs(runs: &[SummaryRow]) -> RunSummary {
    let mut by_model: Map<String, Value> = Map::new();
    let mut buckets: std::collections::BTreeMap<String, ModelBucket> =
        std::collections::BTreeMap::new();
    let mut total_input = 0_i64;
    let mut total_output = 0_i64;
    let mut total_duration = 0_i64;

    for run in runs {
        let input = run.input_tokens.unwrap_or(0);
        let output = run.output_tokens.unwrap_or(0);
        let duration = run.duration_ms.unwrap_or(0);
        total_input += input;
        total_output += output;
        total_duration += duration;

        let model = run.model.clone().unwrap_or_else(|| "unknown".to_string());
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

#[cfg(test)]
mod tests;
