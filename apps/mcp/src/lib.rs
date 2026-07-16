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
//! exposes seven tools (five from the TypeScript original, plus `find_anchors` / `rank_files`), with the
//! same input schemas and output shapes (except `search_knowledge`, re-pointed
//! at the event log when the markdown knowledge store was retired):
//!
//! - `search_knowledge`   — substring search over the `decision` / `lesson`
//!   events in the per-spec NDJSON log; rows are `{ts, kind, title, body?,
//!   spec?}` — the same shape the dashboard knowledge surface renders.
//! - `query_events`       — filter the per-spec NDJSON event log by spec /
//!   event / since.
//! - `find_similar_specs` — rank specs by token overlap on a description.
//! - `get_spec_metrics`   — projected metrics for a spec from NDJSON events.
//! - `get_run_summary`    — aggregated token/duration totals from
//!   `pipeline.telemetry.run` events.
//! - `find_anchors`       — the scan census DIGEST query: tokenizes a free-text
//!   intent and returns the ranked anchor files (plus per-file score / carrying
//!   terms) and the matched-term report. Fail-empty on a missing model / scan
//!   error. Promotes the retrieval that was locked in `mustard-rt run feature`.
//! - `rank_files`         — the scan census file ranker (personalized
//!   PageRank): returns the files ranked for a raw query (plus per-file order /
//!   terms). Degrades to an empty ranking WITH a `note` when the
//!   `grain.dictionary.json` sidecar is absent (the rank pool needs it).
//!
//! ## Persistence (post-W5B)
//!
//! No SQLite. Every read is filesystem-backed:
//!
//! - knowledge → `decision` / `lesson` events in the per-spec NDJSON log
//!   (emitted at CLOSE via `run emit-event`; durable prose knowledge lives in
//!   Claude Code native auto-memory, outside this server).
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

use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use mustard_core::{Event, EventReader};
use mustard_core::domain::scan::{DigestQuery, RankFile, Scan};

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run the `mcp` face: serve the `mustard-memory` MCP server over stdio.
///
/// Builds a local `current_thread` `tokio` runtime, registers the seven tools,
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
    /// Optional kind filter: `decision` or `lesson`.
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

/// Input for `find_anchors` — the scan census DIGEST query wrapper (F6).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FindAnchorsArgs {
    /// Free-text intent. Tokenized (lowercased alphanumeric runs of >= 3 chars,
    /// deduped, capped at 32) into the digest query terms.
    intent: String,
    /// Maximum anchor files to return (`1..=50`, default `10`).
    #[serde(default)]
    limit: Option<usize>,
}

/// Input for `rank_files` — the scan census personalized-PageRank wrapper (F6).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RankFilesArgs {
    /// Free-text (any language) query the ranker matches against the
    /// dictionary-seeded model.
    query: String,
    /// Pool depth — how many ranked files to return (`1..=100`, default `10`).
    #[serde(default)]
    top: Option<usize>,
}


// ---------------------------------------------------------------------------
// Output shapes — serialized to JSON text exactly like the TS `jsonResult`
// ---------------------------------------------------------------------------

/// One knowledge row in `search_knowledge` output — a `decision` / `lesson`
/// event projected to its salient fields.
#[derive(Debug, Serialize)]
struct KnowledgeOut {
    /// Event timestamp (ISO-8601).
    ts: String,
    /// `decision` or `lesson`.
    kind: String,
    /// `payload.title` (decision) / `payload.takeaway` (lesson).
    title: String,
    /// `payload.rationale` (decision) / `payload.trigger` (lesson). Named
    /// `body` so the MCP row and the dashboard knowledge surface stop
    /// drifting on the same concept: `{ts, kind, title, body?, spec?}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
    /// Owning spec, when the event was spec-attributed.
    #[serde(skip_serializing_if = "Option::is_none")]
    spec: Option<String>,
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

    /// Tool 1 — substring search over the recorded decisions / lessons.
    ///
    /// Reads the `decision` / `lesson` events from every per-spec NDJSON log
    /// (the same channel `query_events` reads). The optional `type` filter
    /// narrows to one kind; the substring match is case-insensitive over
    /// `title + body`. Rows rank by match count, newest first on ties.
    #[tool(
        description = "Substring search recorded decisions/lessons from the per-spec NDJSON event log"
    )]
    fn search_knowledge(
        &self,
        Parameters(args): Parameters<SearchKnowledgeArgs>,
    ) -> CallToolResult {
        let limit = args.limit.unwrap_or(10).clamp(1, 50) as usize;
        let Some(paths) = self.claude_paths() else {
            return json_result(&Vec::<KnowledgeOut>::new());
        };
        let specs_root = paths.spec_dir();
        let mut events: Vec<Event> = Vec::new();
        if let Ok(entries) = fs::read_dir(&specs_root) {
            for entry in entries {
                if !entry.path.is_dir() {
                    continue;
                }
                collect_ndjson_under(&entry.path.join(".events"), &mut events);
            }
        }
        let rows = knowledge_rows(events, &args.query, args.r#type.as_deref(), limit);
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

    /// Tool 6 — promote the scan census DIGEST query into the MCP: tokenize a
    /// free-text `intent`, load `.claude/grain.model.json`, and return the
    /// ranked anchor files (plus each anchor's fixed-point score and carrying
    /// terms) with the matched-term report. READ-ONLY / fail-empty: a missing
    /// model, an empty tokenization, or any scan spawn/parse error degrades to
    /// an empty `{ files: [], ... }` result carrying an explanatory `note` —
    /// never a panic, never a tool error.
    #[tool(
        description = "Query the scan census DIGEST for an intent: returns the ranked anchor files (with score/terms) plus the matched-term report. Fail-empty when the scan model is missing."
    )]
    fn find_anchors(
        &self,
        Parameters(args): Parameters<FindAnchorsArgs>,
    ) -> CallToolResult {
        json_result(&self.find_anchors_value(&args.intent, args.limit))
    }

    /// Tool 7 — promote the scan census file ranker (personalized PageRank)
    /// into the MCP: load `.claude/grain.model.json` plus the
    /// `grain.dictionary.json` sidecar and return the files ranked for a raw
    /// `query`, each with its 1-based order and carrying terms. READ-ONLY /
    /// fail-empty: a missing model or scan error degrades to an empty ranking;
    /// a missing dictionary sidecar (the rank pool needs it to seed the walk)
    /// degrades to an empty ranking WITH a clear `note`, never a tool error.
    #[tool(
        description = "Rank the scan census files for a query via personalized PageRank: returns the ranked files (with order/terms). Empty ranking plus a note when the grain.dictionary.json sidecar is absent."
    )]
    fn rank_files(
        &self,
        Parameters(args): Parameters<RankFilesArgs>,
    ) -> CallToolResult {
        json_result(&self.rank_files_value(&args.query, args.top))
    }
}

/// The scan-census retrieval bodies (F6), split from the `#[tool]` methods so
/// the IO / spawn is separate from the pure shaping and both are testable
/// without the MCP transport — mirroring `search_knowledge` over
/// `knowledge_rows`. Every step is fail-open: a degraded read returns an empty
/// structured result with a `note`, never a panic or a tool error.
impl MustardMemory {
    /// The `find_anchors` body as a plain `Value`.
    fn find_anchors_value(&self, intent: &str, limit: Option<usize>) -> Value {
        let limit = limit.unwrap_or(10).clamp(1, 50);
        let terms = intent_terms(intent);
        let Some(paths) = self.claude_paths() else {
            return anchors_empty(intent, &terms, NOTE_NO_PATHS);
        };
        let model = paths.claude_dir().join("grain.model.json");
        if !model.is_file() {
            return anchors_empty(intent, &terms, NOTE_NO_MODEL);
        }
        if terms.is_empty() {
            return anchors_empty(intent, &terms, NOTE_NO_TERMS);
        }
        // The scan digest is deterministic for a given model + terms, so the
        // shaped output is byte-stable. Any spawn / parse failure degrades to an
        // empty result (read-only, fail-open) rather than being surfaced.
        match Scan::locate().digest_query(&model, &terms) {
            Ok(q) => anchors_payload(intent, &terms, &q, limit),
            Err(_) => anchors_empty(intent, &terms, NOTE_SCAN_ERR),
        }
    }

    /// The `rank_files` body as a plain `Value`. The dictionary-absent branch is
    /// a first-class, tested outcome — the caller learns WHY the ranking is
    /// empty (sidecar not built) instead of mistaking it for "nothing matched".
    fn rank_files_value(&self, query: &str, top: Option<usize>) -> Value {
        let top = top.unwrap_or(RANK_TOP_DEFAULT).clamp(1, 100);
        let Some(paths) = self.claude_paths() else {
            return rank_empty(query, top, NOTE_NO_PATHS);
        };
        let claude = paths.claude_dir();
        let model = claude.join("grain.model.json");
        let dict = claude.join("grain.dictionary.json");
        if !model.is_file() {
            return rank_empty(query, top, NOTE_NO_MODEL);
        }
        if !dict.is_file() {
            // scan seeds the personalized PageRank from the dictionary sidecar;
            // without it the rank pool is empty. Report WHY, do not error.
            return rank_empty(query, top, NOTE_DICT_ABSENT);
        }
        match Scan::locate().rank_detail(&model, &dict, query, top, RANK_DIRECT_BASE) {
            Ok(rows) => rank_payload(query, &rows, top),
            Err(_) => rank_empty(query, top, NOTE_SCAN_ERR),
        }
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
             (events, knowledge, specs, metrics, runs), plus the deterministic scan census (find_anchors / rank_files), backed by .claude/."
                .to_string(),
        );
        info
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Project a `decision` / `lesson` event to a `KnowledgeOut` row. `None` for
/// any other event kind or a row with no usable title.
fn event_to_knowledge_out(ev: &Event) -> Option<KnowledgeOut> {
    let kind = event_name(ev);
    if kind != "decision" && kind != "lesson" {
        return None;
    }
    let (title_key, body_key) = if kind == "decision" {
        ("title", "rationale")
    } else {
        ("takeaway", "trigger")
    };
    let title = ev
        .payload
        .get(title_key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let body = ev
        .payload
        .get(body_key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Some(KnowledgeOut {
        ts: event_ts(ev),
        kind: kind.to_string(),
        title,
        body,
        spec: ev.raw.get("spec").and_then(Value::as_str).map(str::to_string),
    })
}

/// Filter + rank the `decision` / `lesson` rows for `search_knowledge`.
///
/// Case-insensitive substring match of `query` over `title + body`;
/// `type_filter` narrows to one kind. Rows rank by match count, newest first
/// on ties, capped at `limit`. Pure — unit-tested without a server.
fn knowledge_rows(
    events: Vec<Event>,
    query: &str,
    type_filter: Option<&str>,
    limit: usize,
) -> Vec<KnowledgeOut> {
    let needle = query.to_lowercase();
    let mut hits: Vec<(usize, KnowledgeOut)> = events
        .iter()
        .filter_map(event_to_knowledge_out)
        .filter(|row| type_filter.is_none_or(|t| row.kind == t))
        .filter_map(|row| {
            let hay = format!("{} {}", row.title, row.body.as_deref().unwrap_or(""))
                .to_lowercase();
            if needle.is_empty() || !hay.contains(&needle) {
                return None;
            }
            let score = hay.matches(&needle).count();
            Some((score, row))
        })
        .collect();
    hits.sort_by(|(sa, ra), (sb, rb)| sb.cmp(sa).then_with(|| rb.ts.cmp(&ra.ts)));
    hits.into_iter().take(limit).map(|(_, r)| r).collect()
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
// find_anchors / rank_files — the promoted scan census retrieval (F6)
//
// `find_anchors` wraps `mustard_core::Scan::digest_query`; `rank_files` wraps
// `Scan::rank_detail`. Both keep the crate's read-only, fail-open contract.
// ---------------------------------------------------------------------------

/// `scan rank`'s direct identifier-match floor — the calibrated product
/// contract, pinned to the same value `mustard-rt run feature` uses so the
/// in-process ranking is byte-identical to the subprocess it promotes.
const RANK_DIRECT_BASE: u64 = 100_000;

/// Default `rank_files` pool depth when the caller omits `top`.
const RANK_TOP_DEFAULT: usize = 10;

/// The `note` on a degraded result names WHY the census answer is empty, so the
/// caller can act (build a scan, build the dictionary) instead of guessing.
const NOTE_NO_PATHS: &str =
    "project .claude/ layout could not be resolved — returning an empty census result";
const NOTE_NO_MODEL: &str =
    "scan model is not built (.claude/grain.model.json is absent) — run a scan first; no census until then";
const NOTE_NO_TERMS: &str =
    "the intent tokenized to no query terms (need a word of >= 3 characters) — nothing to look up";
const NOTE_SCAN_ERR: &str =
    "the scan tool was unavailable or returned no parseable output — returning an empty census result (read-only, fail-open)";
const NOTE_DICT_ABSENT: &str =
    "the ranking dictionary sidecar (.claude/grain.dictionary.json) is not built — personalized PageRank needs it, so the ranking is empty until a scan enrich builds it";

/// Tokenize a free-text intent into digest query terms, mirroring the scan
/// digest path's own extraction (`feature::domain_terms`): lowercased
/// alphanumeric runs of >= 3 chars carrying at least one letter, deduped in
/// first-occurrence order (a `BTreeSet` keeps it deterministic), capped at 32.
/// The digest ORs terms and drops natural-language glue query-side, so
/// over-querying is harmless. Pure — unit-tested without the scan binary.
fn intent_terms(intent: &str) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out: Vec<String> = Vec::new();
    for raw in intent.split(|c: char| !c.is_alphanumeric()) {
        let t = raw.to_lowercase();
        if t.len() >= 3 && t.chars().any(char::is_alphabetic) && seen.insert(t.clone()) {
            out.push(t);
        }
        if out.len() >= 32 {
            break;
        }
    }
    out
}

/// Build the `find_anchors` success payload from a scan `DigestQuery`. Pure (no
/// spawn / IO) so the shape is unit-testable from a synthetic digest. `files` is
/// the ranked anchor list (capped at `limit`); `filesDetail` carries each
/// anchor's fixed-point `scoreX1024` plus the terms that carry it (scan's
/// `files_detail`); `matchedTerms` names the request terms that actually matched
/// (tier != none), falling back to the raw `matched_terms` list on an older
/// payload with no per-term report. Deterministic: scan's order is preserved.
fn anchors_payload(intent: &str, terms: &[String], q: &DigestQuery, limit: usize) -> Value {
    let files: Vec<&String> = q.files.iter().take(limit).collect();
    let files_detail: Vec<Value> = q
        .files_detail
        .iter()
        .take(limit)
        .map(|d| json!({ "file": d.file, "scoreX1024": d.score_x1024, "terms": d.terms }))
        .collect();
    let matched: Vec<String> = if q.report.terms.is_empty() {
        q.matched_terms.iter().map(|t| t.term.clone()).collect()
    } else {
        q.report
            .terms
            .iter()
            .filter(|t| !t.tier.is_empty() && t.tier != "none")
            .map(|t| t.term.clone())
            .collect()
    };
    json!({
        "intent": intent,
        "terms": terms,
        "files": files,
        "filesDetail": files_detail,
        "matchedTerms": matched,
        "report": {
            "matched": q.report.matched,
            "total": q.report.total,
            "reason": q.report.reason,
        },
        "miss": q.miss,
        "note": anchors_note(q),
    })
}

/// The empty `find_anchors` result (the fail-empty contract): `files` is `[]`
/// and a `note` explains why. Same key set as [`anchors_payload`] so a caller
/// reads `files` identically on hit or miss.
fn anchors_empty(intent: &str, terms: &[String], note: &str) -> Value {
    json!({
        "intent": intent,
        "terms": terms,
        "files": [],
        "filesDetail": [],
        "matchedTerms": [],
        "report": { "matched": 0, "total": 0, "reason": "" },
        "miss": true,
        "note": note,
    })
}

/// A concise, deterministic guidance note keyed on the digest's report reason,
/// so ANY Claude session consuming the tool learns how to read the anchors
/// (ranked evidence vs. re-query vs. net-new) without opening the scan JSON.
fn anchors_note(q: &DigestQuery) -> &'static str {
    match q.report.reason.as_str() {
        "strong" => "repo precedent found — `files` is ranked by relevance (BM25F); read the top anchors that fit, then the hubs. `filesDetail` carries each anchor's score and carrying terms.",
        "weak" => "weak precedent — under half the terms matched, or only derived hits; re-query in the code's own vocabulary or Explore before planning on top of this.",
        "generated_only" => "matches live only in machine-written modules — regenerate or extend the generator input; never edit the matched files directly.",
        "none" => "no repo precedent matched — treat as net-new; the term index has false negatives and no synonyms, so confirm by reading before concluding 'absent'.",
        _ if q.miss => "no repo precedent matched — treat as net-new; confirm by reading, do not conclude 'absent' blindly.",
        _ => "repo precedent found — `files` is ranked by relevance; read the top anchors that fit the request.",
    }
}

/// Build the `rank_files` success payload from scan's ranked rows. Pure (no
/// spawn / IO). Each row is `{ file, rank, terms }` where `rank` is the 1-based
/// position: the retrieval signal is the ORDER (scan keeps the fixed-point score
/// inside the tool — the fusion downstream is rank-based, never score-based), so
/// the ordinal IS the exposed score. `terms` is the per-file matched-term
/// evidence (empty on an older scan binary). Order preserved — byte-stable for
/// a deterministic scan.
fn rank_payload(query: &str, rows: &[RankFile], top: usize) -> Value {
    let files: Vec<Value> = rows
        .iter()
        .take(top)
        .enumerate()
        .map(|(i, r)| json!({ "file": r.file, "rank": i + 1, "terms": r.terms }))
        .collect();
    json!({
        "query": query,
        "files": files,
        "top": top,
        "note": "ranked by personalized PageRank over the scan dictionary; `rank` is the 1-based order (the fixed-point score stays inside the scan tool). Read the top files that fit the request.",
    })
}

/// The empty `rank_files` result (the fail-empty contract): `files` is `[]` and
/// a `note` explains why — most importantly the dictionary-absent case, so the
/// caller knows the ranking is unavailable (not that nothing matched).
fn rank_empty(query: &str, top: usize, note: &str) -> Value {
    json!({ "query": query, "files": [], "top": top, "note": note })
}


// ---------------------------------------------------------------------------
// Tests — search_knowledge (event-backed) + `get_run_summary` consolidation
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

    /// Deserialize one NDJSON-shaped JSON value into an [`Event`].
    fn event_from(value: serde_json::Value) -> Event {
        serde_json::from_value(value).expect("valid event")
    }

    /// `search_knowledge` backend: decision/lesson events filter, rank by
    /// match count (newest first on ties), and honor kind filter + limit.
    #[test]
    fn knowledge_rows_filters_and_ranks_decision_lesson_events() {
        let events = vec![
            event_from(json!({
                "kind": "knowledge", "event": "decision", "ts": "2026-07-01T00:00:00.000Z",
                "spec": "alpha",
                "payload": { "title": "Use atomic writes for stores", "rationale": "torn writes corrupt state" },
            })),
            event_from(json!({
                "kind": "knowledge", "event": "lesson", "ts": "2026-07-02T00:00:00.000Z",
                "spec": "alpha",
                "payload": { "trigger": "atomic rename failed on NFS", "takeaway": "atomic atomic writes need same-volume tempfiles" },
            })),
            event_from(json!({
                "kind": "tool", "event": "tool.use", "ts": "2026-07-03T00:00:00.000Z",
                "payload": { "tool": "Bash" },
            })),
        ];

        // Substring match over title+body, both kinds; the lesson carries
        // "atomic" three times (2x takeaway + 1x trigger) and outranks the
        // decision (1x title + 1x rationale = 2).
        let rows = knowledge_rows(events.clone(), "atomic", None, 10);
        assert_eq!(rows.len(), 2, "tool.use is never a knowledge row");
        assert_eq!(rows[0].kind, "lesson");
        assert_eq!(rows[1].kind, "decision");
        assert_eq!(rows[1].title, "Use atomic writes for stores");
        assert_eq!(rows[1].body.as_deref(), Some("torn writes corrupt state"));
        assert_eq!(rows[1].spec.as_deref(), Some("alpha"));

        // Kind filter narrows to one event kind.
        let decisions = knowledge_rows(events.clone(), "atomic", Some("decision"), 10);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].kind, "decision");

        // Limit caps the rows; an unmatched needle yields nothing.
        assert_eq!(knowledge_rows(events.clone(), "atomic", None, 1).len(), 1);
        assert!(knowledge_rows(events, "no-such-term", None, 10).is_empty());
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

    // -- F6: find_anchors / rank_files (promoted scan census retrieval) ----

    #[test]
    fn intent_terms_lowercases_dedups_drops_short_and_caps() {
        let t = intent_terms("Draft the SPEC — spec acceptance, to QA!");
        assert!(t.contains(&"draft".to_string()));
        assert!(t.contains(&"spec".to_string()));
        assert!(t.contains(&"acceptance".to_string()));
        assert!(t.contains(&"the".to_string()), ">=3 chars kept; digest filters relevance");
        assert!(!t.contains(&"to".to_string()), "<3 chars dropped");
        assert!(!t.contains(&"qa".to_string()), "2 chars dropped");
        // Dedup: SPEC/spec collapse to one lowercased term.
        assert_eq!(t.iter().filter(|x| *x == "spec").count(), 1);
        // Cap at 32.
        let many = (0..50).map(|i| format!("term{i}")).collect::<Vec<_>>().join(" ");
        assert!(intent_terms(&many).len() <= 32);
        // Punctuation-only intent yields no terms (drives the NO_TERMS note).
        assert!(intent_terms("!! -- ??").is_empty());
    }

    #[test]
    fn anchors_payload_projects_files_detail_matched_terms_and_report() {
        let q: DigestQuery = serde_json::from_str(
            r#"{"query":["spec","draft","acceptance"],
                "matched_terms":[{"term":"spec","count":40,"samples":["a.rs"]}],
                "files":["a.rs","b.rs","c.rs"],
                "files_detail":[
                    {"file":"a.rs","score_x1024":2048,"terms":["spec"]},
                    {"file":"b.rs","score_x1024":512,"terms":["draft"]},
                    {"file":"c.rs","score_x1024":0,"terms":[]}],
                "miss":false,
                "report":{"matched":2,"total":3,"reason":"strong","terms":[
                    {"term":"spec","tier":"exact","lang":"","files":["a.rs"]},
                    {"term":"draft","tier":"fold","lang":"","files":["b.rs"]},
                    {"term":"acceptance","tier":"none","lang":"","files":[]}]}}"#,
        )
        .expect("digest json");
        let terms = intent_terms("spec draft acceptance");
        let v = anchors_payload("spec draft acceptance", &terms, &q, 2);
        assert_eq!(v["files"], json!(["a.rs", "b.rs"]));
        assert_eq!(v["filesDetail"].as_array().unwrap().len(), 2);
        assert_eq!(v["filesDetail"][0]["scoreX1024"], 2048);
        assert_eq!(v["filesDetail"][0]["terms"], json!(["spec"]));
        assert_eq!(v["matchedTerms"], json!(["spec", "draft"]));
        assert_eq!(v["report"]["matched"], 2);
        assert_eq!(v["report"]["total"], 3);
        assert_eq!(v["report"]["reason"], "strong");
        assert_eq!(v["miss"], json!(false));
        let a = serde_json::to_string(&anchors_payload("spec draft acceptance", &terms, &q, 2)).unwrap();
        let b = serde_json::to_string(&anchors_payload("spec draft acceptance", &terms, &q, 2)).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn anchors_payload_falls_back_to_matched_terms_on_old_payload() {
        let q: DigestQuery = serde_json::from_str(
            r#"{"query":["spec"],"matched_terms":[{"term":"spec","count":3,"samples":["a.rs"]}],"files":["a.rs"],"miss":false}"#,
        )
        .expect("old digest");
        let v = anchors_payload("spec", &["spec".to_string()], &q, 10);
        assert_eq!(v["matchedTerms"], json!(["spec"]));
        assert_eq!(v["report"]["reason"], "");
    }

    #[test]
    fn anchors_empty_keeps_files_array_and_note() {
        let v = anchors_empty("anything", &["anything".to_string()], NOTE_NO_MODEL);
        assert_eq!(v["files"], json!([]));
        assert_eq!(v["filesDetail"], json!([]));
        assert_eq!(v["matchedTerms"], json!([]));
        assert_eq!(v["miss"], json!(true));
        assert!(v["note"].as_str().unwrap().contains("grain.model.json"));
    }

    #[test]
    fn rank_payload_numbers_rows_carries_terms_and_truncates() {
        let rows = vec![
            RankFile { file: "src/a.rs".to_string(), terms: vec!["spec".to_string()] },
            RankFile { file: "src/b.rs".to_string(), terms: vec![] },
        ];
        let v = rank_payload("spec pipeline", &rows, 10);
        let files = v["files"].as_array().unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0]["file"], "src/a.rs");
        assert_eq!(files[0]["rank"], 1);
        assert_eq!(files[0]["terms"], json!(["spec"]));
        assert_eq!(files[1]["rank"], 2);
        assert_eq!(files[1]["terms"], json!([]));
        assert_eq!(v["top"], 10);
        let v2 = rank_payload("spec", &rows, 1);
        assert_eq!(v2["files"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn rank_empty_reports_dictionary_absent() {
        let v = rank_empty("spec", 10, NOTE_DICT_ABSENT);
        assert_eq!(v["files"], json!([]));
        assert_eq!(v["top"], 10);
        assert!(v["note"].as_str().unwrap().contains("grain.dictionary.json"));
    }

    #[test]
    fn find_anchors_value_fails_empty_without_model() {
        let dir = tempfile::tempdir().unwrap();
        let server = MustardMemory::new(dir.path().to_path_buf());
        let v = server.find_anchors_value("spec draft acceptance", None);
        assert_eq!(v["files"], json!([]));
        assert_eq!(v["miss"], json!(true));
        assert!(v["note"].as_str().unwrap().contains("grain.model.json"));
        assert!(v["terms"].as_array().unwrap().contains(&json!("spec")));
    }

    #[test]
    fn rank_files_value_fails_empty_without_model() {
        let dir = tempfile::tempdir().unwrap();
        let server = MustardMemory::new(dir.path().to_path_buf());
        let v = server.rank_files_value("spec pipeline", None);
        assert_eq!(v["files"], json!([]));
        assert!(v["note"].as_str().unwrap().contains("grain.model.json"));
    }

    #[test]
    fn rank_files_value_reports_dict_absent_when_model_present() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join(".claude");
        fs::create_dir_all(&claude).unwrap();
        fs::write(claude.join("grain.model.json"), b"{}").unwrap();
        let server = MustardMemory::new(dir.path().to_path_buf());
        let v = server.rank_files_value("spec pipeline", None);
        assert_eq!(v["files"], json!([]));
        assert!(v["note"].as_str().unwrap().contains("grain.dictionary.json"));
    }

    /// LIVE proof (ignored by default; run with `--ignored --nocapture`): drive
    /// `find_anchors` against this repository real `.claude/grain.model.json` via
    /// the `scan` binary. Self-skips when the model is absent so a hermetic
    /// environment never fails on it.
    #[test]
    #[ignore = "live: needs the repo grain.model.json plus the scan binary"]
    fn live_find_anchors_against_repo_model() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        if !root.join(".claude").join("grain.model.json").is_file() {
            eprintln!("skip: grain.model.json absent under {}", root.display());
            return;
        }
        let server = MustardMemory::new(root);
        let v = server.find_anchors_value("spec draft acceptance", Some(8));
        eprintln!("find_anchors result:\n{}", serde_json::to_string_pretty(&v).unwrap());
        // The read-only contract holds even live: `files` is always an array.
        assert!(v["files"].is_array());
    }

    /// LIVE proof (ignored by default): `rank_files` against this repository
    /// model. This repository has no `grain.dictionary.json`, so it exercises the
    /// graceful dictionary-absent degradation end-to-end.
    #[test]
    #[ignore = "live: needs the repo grain.model.json"]
    fn live_rank_files_degrades_without_dictionary() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        if !root.join(".claude").join("grain.model.json").is_file() {
            eprintln!("skip: grain.model.json absent under {}", root.display());
            return;
        }
        let server = MustardMemory::new(root);
        let v = server.rank_files_value("spec pipeline reconciliation", Some(10));
        eprintln!("rank_files result:\n{}", serde_json::to_string_pretty(&v).unwrap());
        assert!(v["files"].is_array());
    }
}

