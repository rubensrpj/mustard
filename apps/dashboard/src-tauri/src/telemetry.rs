use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Serialize, Clone)]
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
    /// Daily series (oldest first). Empty when RTK is unavailable.
    pub daily: Vec<RtkDaily>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MeasuredBlock {
    pub tokens_total: u64,
    pub tokens_today: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HookFireCount {
    pub hook: String,
    pub fires: u64,
    pub tokens_saved: u64,
    pub most_recent_ts: Option<String>,
    /// Lifetime totals are append-only and never reset. These `session_*`
    /// fields count only the lines whose `ts` falls inside the current
    /// session window (see `session_start_ts`), so the UI can honestly show
    /// "323.4K total · +N nesta sessão".
    pub session_fires: u64,
    pub session_tokens_saved: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RoutingByIntent {
    pub intent: String,
    pub blocks: u64,
    pub allows: u64,
}

/// Breakdown by note (prevention category): "violation", "no-model-denied",
/// "no-model-denied-sonnet" (Mustard 2.5 rule), "no-model-advisory" (warn-mode),
/// "passed". Surfaces which protection mechanism is firing.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RoutingByNote {
    pub note: String,
    pub count: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RoutingBlock {
    pub blocks: u64,
    pub allows: u64,
    pub by_intent: Vec<RoutingByIntent>,
    pub by_note: Vec<RoutingByNote>,
    /// Session-scoped subset of `blocks` / `allows` — only dispatches inside
    /// the current session window. Lifetime numbers above never reset; these
    /// answer "what happened in this run".
    pub session_blocks: u64,
    pub session_allows: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PhaseCount {
    pub phase: String,
    pub count: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowBlock {
    pub by_phase: Vec<PhaseCount>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolCount {
    pub tool_name: String,
    pub count: u64,
}

/// Per-agent-type aggregation of agent.start / agent.stop pairs from
/// the events table. Tokens come from spans table (not yet wired); duration
/// is derived from start→stop timestamps when both halves exist.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentActivity {
    pub agent_type: String,
    /// Number of agent.start events seen for this agent_type.
    pub starts: u64,
    /// Number of matched agent.stop events (sessionId + actor.id pair).
    pub stops: u64,
    /// Number of stops flagged is_error=true.
    pub errors: u64,
    /// Average duration in milliseconds across matched pairs. 0 when no pairs.
    pub avg_duration_ms: u64,
    /// Most recent ts seen (start or stop).
    pub last_ts: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentActivityBlock {
    pub total_dispatches: u64,
    pub total_errors: u64,
    pub agents: Vec<AgentActivity>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TelemetrySummary {
    pub rtk: RtkBlock,
    pub measured: MeasuredBlock,
    pub prevention: Vec<HookFireCount>,
    pub routing: RoutingBlock,
    pub workflow: WorkflowBlock,
    pub tool_breakdown: Vec<ToolCount>,
    pub agent_activity: AgentActivityBlock,
    /// ISO-8601 timestamp marking the start of the current session, or null
    /// when no session boundary could be derived. Every `session_*` counter
    /// in this payload is "lines whose ts >= this value".
    pub session_start_ts: Option<String>,
}

// ── Session window ───────────────────────────────────────────────────────────
//
// Hook `.metrics/*.jsonl` files and `model-routing-gate.jsonl` are append-only
// for the lifetime of the install — they never reset. To honestly show "+N
// this session" we need a cut-off timestamp. We derive it from mustard.db:
// the `ts` of the FIRST event sharing the LAST event's `session_id`. That is
// the moment the current Claude Code session began emitting.

/// Returns the ISO timestamp at which the current session started, or `None`
/// when mustard.db is missing/empty or carries no `session_id`.
pub fn session_start_ts(repo_path: &Path) -> Option<String> {
    crate::db::with_db(repo_path, |conn| {
        crate::db::session_start_ts_from_db(conn).ok_or_else(|| "no session".to_string())
    })
    .and_then(|r| r.ok())
}

/// True when `ts` is lexically >= the session cut-off. ISO-8601 UTC strings
/// sort chronologically, so a plain string comparison is correct and avoids a
/// date-parsing dependency.
fn in_session(ts: Option<&str>, since: Option<&str>) -> bool {
    match (ts, since) {
        (Some(t), Some(s)) => t >= s,
        _ => false,
    }
}

// ── RTK ─────────────────────────────────────────────────────────────────────
//
// RTK reads from its own global store (~/.rtk/). We invoke `rtk gain` with
// `--format json` for stable parsing and `--daily` for the time series.
// Per-project filtering uses `--project` (RTK filters by the cwd of the call,
// so we set `current_dir` to the repo path); the global block omits `-p`.

fn rtk_unavailable() -> RtkBlock {
    RtkBlock {
        available: false,
        total_commands: None,
        input_tokens: None,
        output_tokens: None,
        tokens_saved: None,
        savings_pct: None,
        total_exec_time_ms: None,
        daily: vec![],
    }
}

/// Run `rtk gain -f json --daily`, optionally with `-p` and a chdir, and decode
/// the result. Returns `rtk_unavailable()` on any failure (binary missing,
/// non-zero exit, malformed JSON).
fn run_rtk_gain(repo_path: Option<&Path>) -> RtkBlock {
    let mut cmd = std::process::Command::new("rtk");
    cmd.arg("gain").arg("-f").arg("json").arg("--daily");
    if let Some(p) = repo_path {
        cmd.arg("-p").current_dir(p);
    }
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
    let v: serde_json::Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(_) => return rtk_unavailable(),
    };

    let summary = v.get("summary");
    let total_commands = summary.and_then(|s| s.get("total_commands")).and_then(|x| x.as_u64());
    let input_tokens = summary.and_then(|s| s.get("total_input")).and_then(|x| x.as_u64());
    let output_tokens = summary.and_then(|s| s.get("total_output")).and_then(|x| x.as_u64());
    let tokens_saved = summary.and_then(|s| s.get("total_saved")).and_then(|x| x.as_u64());
    let savings_pct = summary.and_then(|s| s.get("avg_savings_pct")).and_then(|x| x.as_f64());
    let total_exec_time_ms = summary.and_then(|s| s.get("total_time_ms")).and_then(|x| x.as_u64());

    let daily: Vec<RtkDaily> = v
        .get("daily")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    let date = entry.get("date").and_then(|x| x.as_str())?.to_string();
                    Some(RtkDaily {
                        date,
                        commands: entry.get("commands").and_then(|x| x.as_u64()).unwrap_or(0),
                        input_tokens: entry.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
                        output_tokens: entry.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
                        saved_tokens: entry.get("saved_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
                        savings_pct: entry.get("savings_pct").and_then(|x| x.as_f64()).unwrap_or(0.0),
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

/// RTK stats filtered to the given repo (uses `rtk gain -p` with cwd set).
pub fn rtk_summary(repo_path: &Path) -> RtkBlock {
    run_rtk_gain(Some(repo_path))
}

/// RTK stats across all projects (no `-p`). Used by the global overview.
pub fn rtk_summary_global() -> RtkBlock {
    run_rtk_gain(None)
}

// ── Hook fire counts ─────────────────────────────────────────────────────────

const EXCLUDED_HOOKS: &[&str] = &["rtk-gain", "rtk-rewrite", "budget-observations"];

pub fn hook_fire_counts(repo_path: &Path, session_since: Option<&str>) -> Vec<HookFireCount> {
    let metrics_dir = repo_path.join(".claude").join(".metrics");
    let rd = match std::fs::read_dir(&metrics_dir) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    let mut results: Vec<HookFireCount> = Vec::new();

    for entry in rd {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
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
            Err(e) => {
                eprintln!("hook_fire_counts: failed to read {:?}: {}", path, e);
                continue;
            }
        };
        let mut fires: u64 = 0;
        let mut tokens_saved: u64 = 0;
        let mut session_fires: u64 = 0;
        let mut session_tokens_saved: u64 = 0;
        let mut most_recent_ts: Option<String> = None;
        for line in content.lines() {
            let v: serde_json::Value = match serde_json::from_str(line) {
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

pub fn routing_breakdown(repo_path: &Path, session_since: Option<&str>) -> RoutingBlock {
    let path = repo_path
        .join(".claude")
        .join(".metrics")
        .join("model-routing-gate.jsonl");
    let empty = || RoutingBlock {
        blocks: 0,
        allows: 0,
        by_intent: vec![],
        by_note: vec![],
        session_blocks: 0,
        session_allows: 0,
    };
    if !path.exists() {
        return empty();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return empty(),
    };

    let mut total_blocks: u64 = 0;
    let mut total_allows: u64 = 0;
    let mut session_blocks: u64 = 0;
    let mut session_allows: u64 = 0;
    // grouping key (subagent_type | pipeline_type) -> (blocks, allows)
    let mut grouped: HashMap<String, (u64, u64)> = HashMap::new();
    // per-note tally for the prevention-category breakdown
    let mut by_note_map: HashMap<String, u64> = HashMap::new();

    for line in content.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        // `note` lives at the top level (legacy) or under payload (newer hook).
        let note = v["note"]
            .as_str()
            .or_else(|| v.get("payload").and_then(|p| p.get("note")).and_then(|n| n.as_str()))
            .unwrap_or("");
        // Real block notes from model-routing-gate.js:
        // - "violation": explicit upgrade attempt blocked
        // - "no-model-denied": Explorer dispatch without explicit model
        // - "no-model-denied-sonnet": Rule C — inherited model when sonnet expected
        // "blocked" kept for legacy.
        let is_block = matches!(
            note,
            "violation" | "no-model-denied" | "no-model-denied-sonnet" | "blocked"
        );
        // Allow notes: explicit pass + advisory (warned but allowed).
        let is_allow =
            note == "passed" || note == "no-model-advisory" || note.starts_with("allow");
        if !is_block && !is_allow {
            continue;
        }

        // Bump per-note tally regardless of block/allow.
        *by_note_map.entry(note.to_string()).or_insert(0) += 1;

        // Group by subagent_type (most actionable). Falls back to pipeline_type,
        // then legacy intent, then "unknown".
        let key = extract_routing_key(&v);

        let entry = grouped.entry(key).or_insert((0, 0));
        let session = in_session(v.get("ts").and_then(|x| x.as_str()), session_since);
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
    // Top 6 by blocks first (problemas mais importantes), depois total
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
    by_note.sort_by(|a, b| b.count.cmp(&a.count));

    RoutingBlock {
        blocks: total_blocks,
        allows: total_allows,
        by_intent: intent_vec,
        by_note,
        session_blocks,
        session_allows,
    }
}

/// Pull the most useful grouping key out of a model-routing-gate event:
/// subagent_type → pipeline_type → legacy intent → "unknown". Looks in both
/// top-level and payload.extras since the hook moved fields around.
fn extract_routing_key(v: &serde_json::Value) -> String {
    let extras = v.get("payload").and_then(|p| p.get("extras"));
    let lookup = |k: &str| -> Option<String> {
        v.get(k)
            .and_then(|x| x.as_str())
            .or_else(|| extras.and_then(|e| e.get(k)).and_then(|x| x.as_str()))
            .filter(|s| !s.is_empty() && *s != "unknown" && *s != "none")
            .map(|s| s.to_string())
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
        .and_then(|i| i.as_str())
    {
        if !s.is_empty() {
            return s.to_string();
        }
    }
    "outros".to_string()
}

// ── Workflow by phase ────────────────────────────────────────────────────────

pub fn workflow_by_phase(repo_path: &Path) -> WorkflowBlock {
    if let Some(r) = crate::db::with_db(repo_path, crate::db::workflow_by_phase_from_db) {
        match r {
            Ok(block) => return block,
            Err(e) => eprintln!("workflow_by_phase: db error: {}", e),
        }
    }
    WorkflowBlock { by_phase: vec![] }
}

// ── Tool breakdown ────────────────────────────────────────────────────────────

pub fn tool_breakdown(repo_path: &Path) -> Vec<ToolCount> {
    if let Some(r) = crate::db::with_db(repo_path, |c| crate::db::tool_breakdown_from_db(c, 15)) {
        match r {
            Ok(list) => return list,
            Err(e) => eprintln!("tool_breakdown: db error: {}", e),
        }
    }
    vec![]
}

// ── Agent activity (span-lite consumer) ──────────────────────────────────────
//
// Aggregates agent.start / agent.stop pairs from the events table, grouped by
// actor_id (agent_type). Tokens are deliberately omitted — they live in the
// spans table, not in events. Duration is best-effort: start→stop pairing on
// session_id + actor_id. When a pair is missing (agent still running, or stop
// dropped), the start counts but duration stays at 0.

pub fn agent_activity(repo_path: &Path) -> AgentActivityBlock {
    match crate::db::with_db(repo_path, crate::db::agent_activity_from_db) {
        Some(Ok(block)) => block,
        Some(Err(e)) => {
            eprintln!("agent_activity: db error: {}", e);
            AgentActivityBlock { total_dispatches: 0, total_errors: 0, agents: vec![] }
        }
        None => AgentActivityBlock { total_dispatches: 0, total_errors: 0, agents: vec![] },
    }
}

/// Parse ISO-8601 timestamp to milliseconds since epoch. Best-effort — returns
/// None when the timestamp is malformed. Accepts both with and without
/// fractional seconds and the trailing 'Z'.
///
/// Public so `db::agent_activity_from_db` can use it for duration computation.
pub fn parse_iso_ms_pub(s: &str) -> Option<u64> {
    parse_iso_ms(s)
}

fn parse_iso_ms(s: &str) -> Option<u64> {
    // chrono dependency would be ideal but the crate already vendors it elsewhere;
    // here we do a minimal manual parse: YYYY-MM-DDTHH:MM:SS[.fff]Z
    // Returns ms from a fixed epoch — relative deltas only matter for duration.
    let bytes = s.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let year: i64 = std::str::from_utf8(&bytes[0..4]).ok()?.parse().ok()?;
    let month: i64 = std::str::from_utf8(&bytes[5..7]).ok()?.parse().ok()?;
    let day: i64 = std::str::from_utf8(&bytes[8..10]).ok()?.parse().ok()?;
    let hour: i64 = std::str::from_utf8(&bytes[11..13]).ok()?.parse().ok()?;
    let minute: i64 = std::str::from_utf8(&bytes[14..16]).ok()?.parse().ok()?;
    let second: i64 = std::str::from_utf8(&bytes[17..19]).ok()?.parse().ok()?;
    // Optional .fff
    let ms_frac: i64 = if bytes.len() >= 23 && bytes[19] == b'.' {
        std::str::from_utf8(&bytes[20..23]).ok()?.parse().ok().unwrap_or(0)
    } else {
        0
    };
    // Approximate epoch math — good enough for duration deltas within the
    // same year. days_in_year is a constant; the year offset cancels out for
    // start→stop diffs on the same date.
    let days = year * 365 + month * 31 + day; // approximate; fine for diffs
    let total_seconds = days * 86_400 + hour * 3600 + minute * 60 + second;
    Some((total_seconds * 1000 + ms_frac) as u64)
}

// ── Measured ─────────────────────────────────────────────────────────────────

pub fn measured(repo_path: &Path) -> MeasuredBlock {
    if let Some(r) = crate::db::with_db(repo_path, crate::db::metrics_from_db) {
        if let Ok(m) = r {
            return MeasuredBlock {
                tokens_total: m.tokens_total,
                tokens_today: m.tokens_today,
            };
        }
    }
    MeasuredBlock { tokens_total: 0, tokens_today: 0 }
}

// ── Live activity ────────────────────────────────────────────────────────────
//
// Derives live activity from mustard.db (the canonical harness store since the
// eliminate-bun spec). Events are written by mustard-rt on every hook dispatch,
// so the DB is always current. The SQLite `datetime('now', ...)` predicates
// replacing the former NDJSON-based readers.

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct PhaseActivity {
    /// Fase canônica: "ANALYZE" | "PLAN" | "EXECUTE" | "QA" | "CLOSE".
    pub phase: String,
    pub events_today: u64,
    pub events_last_hour: u64,
    pub events_last_5min: u64,
    /// 60 buckets, um por minuto, oldest first.
    pub minute_buckets: Vec<u64>,
    /// Timestamp do evento mais recente NESTA fase (RFC3339).
    pub last_event_ts: Option<String>,
    /// Top tools usadas nesta fase hoje (até 3).
    pub top_tools: Vec<ToolCount>,
    /// Última spec etiquetada em um evento desta fase (para contexto).
    pub last_spec: Option<String>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct LiveActivity {
    /// Timestamp do evento mais recente em qualquer fase.
    pub last_event_ts: Option<String>,
    /// Total de eventos desde 00:00 UTC hoje (todas as fases).
    pub events_today: u64,
    /// Eventos na última hora (todas as fases).
    pub events_last_hour: u64,
    /// Eventos nos últimos 5 minutos (todas as fases).
    pub events_last_5min: u64,
    /// Top tools hoje (todas as fases).
    pub tools_today: Vec<ToolCount>,
    /// Sparkline agregado de 60 minutos (todas as fases).
    pub minute_buckets: Vec<u64>,
    /// Fase canônica do evento mais recente.
    pub current_phase: Option<String>,
    /// Spec do evento mais recente (legacy — pode ser de spec abandonada).
    pub current_spec: Option<String>,
    /// Wave do evento mais recente.
    pub current_wave: Option<u32>,
    /// `true` se o evento mais recente é mais novo que 2 minutos.
    pub is_fresh: bool,
    /// Agregados por fase canônica (sempre 5 entradas, ordem fixa).
    pub by_phase: Vec<PhaseActivity>,
}

pub const CANONICAL_PHASES: &[&str] = &["ANALYZE", "PLAN", "EXECUTE", "QA", "CLOSE"];

pub fn live_activity(repo_path: &Path) -> LiveActivity {
    match crate::db::with_db(repo_path, crate::db::live_activity_from_db) {
        Some(Ok(activity)) => activity,
        Some(Err(e)) => {
            eprintln!("live_activity: db error: {}", e);
            LiveActivity::default()
        }
        None => LiveActivity::default(),
    }
}

// ── Friction telemetry (Wave 4) ─────────────────────────────────────────────
//
// `.claude/.metrics/friction.json` holds measured atrito (high hook-retry,
// heavy API usage) — telemetry, NOT knowledge. The session-knowledge hooks
// write it as `{ version, entries: [...] }`. The dashboard reads it so the
// Knowledge page can show friction in a section separate from real patterns.

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct FrictionEntry {
    pub name: String,
    pub description: String,
    pub source: Option<String>,
    pub tags: Vec<String>,
    /// Measured hook-level retries (only on high-hook-retry entries).
    pub retry_count: Option<u64>,
    /// Measured API call count (only on heavy-pipeline entries).
    pub api_calls: Option<u64>,
    /// Optional prescriptive hint derived by the extractor.
    pub prescription: Option<String>,
    pub updated_at: Option<String>,
}

/// Read friction entries for a repo. Returns an empty vec when the file is
/// missing or malformed — friction is genuinely rare, so empty is the norm.
///
/// Wave 5 fix (2026-05-20): `friction.json` accumulates one entry per write,
/// so a hot-loop hook (e.g. `high-hook-retry-*.metrics`) used to surface as
/// ~11 visually identical rows. We deduplicate by `name`, keeping the row
/// with the most recent `updated_at`. The on-disk file is unchanged — the
/// dedup happens in the read path so old data is not lost.
pub fn friction_entries(repo_path: &Path) -> Vec<FrictionEntry> {
    let path = repo_path
        .join(".claude")
        .join(".metrics")
        .join("friction.json");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let v: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let entries = match v.get("entries").and_then(|e| e.as_array()) {
        Some(a) => a,
        None => return vec![],
    };

    // Pass 1 — decode every row, keeping the *latest* per `name`.
    let mut by_name: std::collections::HashMap<String, FrictionEntry> =
        std::collections::HashMap::new();
    for e in entries {
        let Some(name) = e.get("name").and_then(|x| x.as_str()).map(String::from) else {
            continue;
        };
        let entry = FrictionEntry {
            name: name.clone(),
            description: e
                .get("description")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            source: e.get("source").and_then(|x| x.as_str()).map(String::from),
            tags: e
                .get("tags")
                .and_then(|x| x.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|t| t.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            retry_count: e.get("retryCount").and_then(|x| x.as_u64()),
            api_calls: e.get("apiCalls").and_then(|x| x.as_u64()),
            prescription: e
                .get("prescription")
                .and_then(|x| x.as_str())
                .map(String::from),
            updated_at: e.get("updatedAt").and_then(|x| x.as_str()).map(String::from),
        };
        match by_name.get(&name) {
            // First time we've seen this name → keep.
            None => {
                by_name.insert(name, entry);
            }
            // We've seen it before → keep whichever has the more recent
            // updated_at (lexicographic compare works for ISO-8601). Entries
            // without `updated_at` lose to ones that have one.
            Some(existing) => {
                let new_ts = entry.updated_at.as_deref().unwrap_or("");
                let old_ts = existing.updated_at.as_deref().unwrap_or("");
                if new_ts > old_ts {
                    by_name.insert(name, entry);
                }
            }
        }
    }

    // Pass 2 — drain into a Vec sorted by name so the UI shows a stable list.
    let mut out: Vec<FrictionEntry> = by_name.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

// ── Honest Prompt Economy (Wave 5) ──────────────────────────────────────────
//
// Three honest blocks the dashboard surfaces side by side:
//   A · Cost from Claude Code native OTEL (claude_code_otel table)
//   B · Bytes the orchestrator chose NOT to send (events table, `mustard.subtraction.applied`)
//   C · Operational counters from Claude Code (session count + active time)
//
// Plus a freshness block driving the green/amber/red badge and an optional
// canary log tail when OTEL is unhealthy.

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct ModelCost {
    pub model: String,
    pub usd: f64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct SessionCost {
    pub session_id: String,
    pub usd: f64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct CostBlock {
    pub usd_total: f64,
    pub by_model: Vec<ModelCost>,
    pub by_session: Vec<SessionCost>,
}

/// Per-wave breakdown of context sent vs. avoided. One row per `wave` value
/// found in `mustard.subtraction.applied` payloads.
#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct WaveSubtraction {
    pub wave: u64,
    /// Σ `prompt_bytes` — context actually dispatched to sub-agents this wave.
    pub sent_bytes: u64,
    /// Σ `bytes_omitted` — rest of the spec the sub-agents never had to see.
    pub avoided_bytes: u64,
    pub count: u64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct SubtractionsBlock {
    /// Σ `prompt_bytes` across all wave slices — context Mustard actually sent.
    pub context_sent_bytes: u64,
    /// Σ `bytes_omitted` across all wave slices — context Mustard avoided.
    pub context_avoided_bytes: u64,
    /// Total `mustard.subtraction.applied` events counted.
    pub event_count: u64,
    /// Breakdown grouped by wave, ascending.
    pub by_wave: Vec<WaveSubtraction>,
    /// Lifetime totals above are an append-only accumulator and never reset.
    /// These `session_*` fields count only `mustard.subtraction.applied` events
    /// whose `ts` falls inside the current session window — same treatment as
    /// `HookFireCount.session_fires` / `RoutingBlock.session_blocks`, so the UI
    /// can honestly show "1.7 MB total · +N nesta sessão". When the session
    /// window cannot be derived they stay 0 and the UI labels the card as a
    /// lifetime accumulator with no noisy "+0".
    pub session_sent_bytes: u64,
    pub session_avoided_bytes: u64,
    pub session_count: u64,
    /// True when a session window was available to compute the deltas above.
    /// `false` means "show total only, labelled lifetime".
    pub session_known: bool,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct ClaudeEventsBlock {
    pub session_count: u64,
    pub active_time_seconds: f64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct FreshnessBlock {
    pub last_metric_ts: Option<String>,
    pub last_subtraction_ts: Option<String>,
    pub otel_healthy: bool,
    pub canary_tail: Option<Vec<String>>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct DashboardPromptEconomy {
    pub cost: CostBlock,
    pub subtractions: SubtractionsBlock,
    pub claude_events: ClaudeEventsBlock,
    pub freshness: FreshnessBlock,
}

/// Format an `ms` epoch (rounded to minute) as RFC 3339 / ISO-8601 UTC.
/// We rely on chrono (already in Cargo.toml) for correctness; the existing
/// hand-rolled converters in lib.rs are tuned for seconds, not ms.
fn ms_to_iso(ms: i64) -> Option<String> {
    let secs = ms / 1000;
    let nsec = ((ms % 1000) * 1_000_000) as u32;
    chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nsec).map(|dt| dt.to_rfc3339())
}

/// Cost block — three queries against `claude_code_otel`. Each is fail-soft
/// (returns the default block) when the table is missing or empty.
fn cost_block(conn: &Connection) -> CostBlock {
    let mut out = CostBlock::default();

    out.usd_total = conn
        .query_row(
            "SELECT COALESCE(SUM(sum), 0) FROM claude_code_otel \
             WHERE metric = 'claude_code.cost.usage'",
            [],
            |row| row.get::<_, Option<f64>>(0),
        )
        .ok()
        .flatten()
        .unwrap_or(0.0);

    if let Ok(mut stmt) = conn.prepare(
        "SELECT model, COALESCE(SUM(sum), 0) AS usd FROM claude_code_otel \
         WHERE metric = 'claude_code.cost.usage' AND model IS NOT NULL \
         GROUP BY model ORDER BY usd DESC",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok(ModelCost {
                model: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                usd: row.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
            })
        }) {
            out.by_model = rows.flatten().collect();
        }
    }

    if let Ok(mut stmt) = conn.prepare(
        "SELECT session_id, COALESCE(SUM(sum), 0) AS usd FROM claude_code_otel \
         WHERE metric = 'claude_code.cost.usage' AND session_id IS NOT NULL \
         GROUP BY session_id ORDER BY usd DESC LIMIT 10",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok(SessionCost {
                session_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                usd: row.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
            })
        }) {
            out.by_session = rows.flatten().collect();
        }
    }

    out
}

/// Subtractions block — one GROUP BY query against `events` where
/// `event='mustard.subtraction.applied'`. Every such event is a `wave-slice`
/// now, so we group by `wave` and sum `prompt_bytes` (context sent) and
/// `bytes_omitted` (context avoided) per wave plus an overall total.
///
/// The lifetime totals are an append-only accumulator. `session_since` (the
/// ISO `ts` the current session started at) lets us additionally count the
/// subset of events inside the session window — the `events` table carries a
/// per-row `ts`, so the delta is honest, not estimated. When `session_since`
/// is `None` the `session_*` fields stay 0 and `session_known` is `false`.
fn subtractions_block(conn: &Connection, session_since: Option<&str>) -> SubtractionsBlock {
    let mut out = SubtractionsBlock::default();

    let sql = "SELECT CAST(json_extract(payload, '$.wave') AS INTEGER) AS w, \
                      COALESCE(SUM(CAST(json_extract(payload, '$.prompt_bytes') AS INTEGER)), 0) AS sent, \
                      COALESCE(SUM(CAST(json_extract(payload, '$.bytes_omitted') AS INTEGER)), 0) AS avoided, \
                      COUNT(*) AS cnt \
               FROM events WHERE event = 'mustard.subtraction.applied' \
               GROUP BY w ORDER BY w";
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return out,
    };
    let rows = match stmt.query_map([], |row| {
        Ok(WaveSubtraction {
            wave: row.get::<_, Option<i64>>(0)?.unwrap_or(0).max(0) as u64,
            sent_bytes: row.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0) as u64,
            avoided_bytes: row.get::<_, Option<i64>>(2)?.unwrap_or(0).max(0) as u64,
            count: row.get::<_, Option<i64>>(3)?.unwrap_or(0).max(0) as u64,
        })
    }) {
        Ok(r) => r,
        Err(_) => return out,
    };
    for w in rows.flatten() {
        out.context_sent_bytes += w.sent_bytes;
        out.context_avoided_bytes += w.avoided_bytes;
        out.event_count += w.count;
        out.by_wave.push(w);
    }

    // Session delta — only when a session window is known. ISO-8601 UTC
    // strings sort chronologically, so `ts >= ?` is a correct cut-off.
    if let Some(since) = session_since {
        let session_sql = "SELECT \
                COALESCE(SUM(CAST(json_extract(payload, '$.prompt_bytes') AS INTEGER)), 0), \
                COALESCE(SUM(CAST(json_extract(payload, '$.bytes_omitted') AS INTEGER)), 0), \
                COUNT(*) \
             FROM events \
             WHERE event = 'mustard.subtraction.applied' AND ts >= ?1";
        if let Ok((sent, avoided, count)) = conn.query_row(session_sql, [since], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?.unwrap_or(0).max(0) as u64,
                row.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0) as u64,
                row.get::<_, Option<i64>>(2)?.unwrap_or(0).max(0) as u64,
            ))
        }) {
            out.session_sent_bytes = sent;
            out.session_avoided_bytes = avoided;
            out.session_count = count;
            out.session_known = true;
        }
    }

    out
}

/// Claude Code operational counters — session.count and active_time.total.
fn claude_events_block(conn: &Connection) -> ClaudeEventsBlock {
    let session_count = conn
        .query_row(
            "SELECT COALESCE(SUM(sum), 0) FROM claude_code_otel \
             WHERE metric = 'claude_code.session.count'",
            [],
            |row| row.get::<_, Option<f64>>(0),
        )
        .ok()
        .flatten()
        .unwrap_or(0.0);
    let active_time_seconds = conn
        .query_row(
            "SELECT COALESCE(SUM(sum), 0) FROM claude_code_otel \
             WHERE metric = 'claude_code.active_time.total'",
            [],
            |row| row.get::<_, Option<f64>>(0),
        )
        .ok()
        .flatten()
        .unwrap_or(0.0);
    ClaudeEventsBlock {
        session_count: session_count.max(0.0).round() as u64,
        active_time_seconds,
    }
}

/// Freshness — when did we last see a metric? a subtraction event? is the
/// collector healthy? Healthy means: most recent OTEL bucket within 5 minutes
/// OR a PID file exists at `<repo>/.claude/.harness/.otel-collector.pid`.
/// (We don't probe the process — `sysinfo` isn't a dep; spec allows trusting
/// PID-file presence as a fallback.)
fn freshness_block(conn: &Connection, repo_path: &Path) -> FreshnessBlock {
    let last_metric_ms: Option<i64> = conn
        .query_row(
            "SELECT MAX(ts_bucket) FROM claude_code_otel",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )
        .ok()
        .flatten();
    let last_metric_ts = last_metric_ms.and_then(ms_to_iso);

    let last_subtraction_ts: Option<String> = conn
        .query_row(
            "SELECT MAX(ts) FROM events WHERE event = 'mustard.subtraction.applied'",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten();

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let fresh_metric = match last_metric_ms {
        Some(ts) => (now_ms - ts).abs() < 5 * 60_000,
        None => false,
    };

    let pid_path = repo_path
        .join(".claude")
        .join(".harness")
        .join(".otel-collector.pid");
    // Trust the PID file only when it has been refreshed within the last
    // 5 minutes — a wedged collector may leave a stale PID behind, and
    // mere `is_file()` would keep the badge green/amber in that case.
    let pid_recent = std::fs::metadata(&pid_path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map(|d| d.as_secs() < 300)
        .unwrap_or(false);

    let otel_healthy = fresh_metric || pid_recent;

    let canary_tail = if otel_healthy {
        None
    } else {
        let canary_path = repo_path
            .join(".claude")
            .join(".harness")
            .join(".canary.log");
        match std::fs::read_to_string(&canary_path) {
            Ok(s) => {
                let lines: Vec<String> = s
                    .lines()
                    .rev()
                    .take(20)
                    .map(|l| l.to_string())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                if lines.is_empty() {
                    None
                } else {
                    Some(lines)
                }
            }
            Err(_) => None,
        }
    };

    FreshnessBlock {
        last_metric_ts,
        last_subtraction_ts,
        otel_healthy,
        canary_tail,
    }
}

/// Single source of truth for the OTEL collector badge shown on every page.
/// Three states, derived from the exact same rule the Prompt Economy page used
/// to compute locally — now every screen consumes this instead of inventing
/// its own signal:
///  - `Live`  : collector healthy AND last OTEL metric within 5 minutes.
///  - `Stale` : a metric was seen at some point, but it's old / collector down.
///  - `Off`   : no metric was ever received → OTEL genuinely not configured.
///
/// `Off` is reserved for "never saw data" — a project with months of history
/// but a crashed collector is `Stale`, not `Off`.
#[derive(Serialize, Default, PartialEq, Eq, Clone, Copy, Debug)]
#[serde(rename_all = "lowercase")]
pub enum CollectorHealth {
    Live,
    Stale,
    #[default]
    Off,
}

/// Compute the collector badge state from a freshness block. Kept separate from
/// the Tauri command so it stays unit-testable and reusable.
pub fn collector_health_from_freshness(f: &FreshnessBlock) -> CollectorHealth {
    // Accurate epoch-ms parse via chrono (already a dependency). Avoids the
    // approximate date arithmetic in `parse_iso_ms`, which is fine for relative
    // durations but not for an absolute "within 5 minutes" check.
    let last_metric_ms = f.last_metric_ts.as_deref().and_then(|s| {
        chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|dt| dt.timestamp_millis())
    });
    let Some(ts) = last_metric_ms else {
        // Never received a metric → genuinely not configured.
        return CollectorHealth::Off;
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let fresh = (now_ms - ts).abs() < 5 * 60_000;
    if f.otel_healthy && fresh {
        CollectorHealth::Live
    } else {
        // Collector is parado (stopped/stale): log the last known metric timestamp
        // so diagnostics can surface exactly when data stopped flowing.
        eprintln!(
            "collector parado — otel_healthy={}, last_metric_ts={:?}",
            f.otel_healthy, f.last_metric_ts
        );
        CollectorHealth::Stale
    }
}

/// Tauri command: the unified collector badge. Every page (Telemetry, Prompt
/// Economy, and any future Economy section) consumes this instead of deriving
/// its own badge — guarantees the same state shows everywhere at once.
/// A missing `mustard.db` degrades to `Off` (never configured) rather than an
/// error, since "no harness DB" is exactly the not-configured case.
#[tauri::command]
pub fn collector_health(repo_path: String) -> Result<CollectorHealth, String> {
    let base = std::path::PathBuf::from(&repo_path);
    match open_repo_db(&base) {
        Ok(conn) => {
            let freshness = freshness_block(&conn, &base);
            Ok(collector_health_from_freshness(&freshness))
        }
        // No harness DB yet → OTEL was never wired up for this project.
        Err(_) => Ok(CollectorHealth::Off),
    }
}

/// Open the mustard.db file for a given repo. Returns a descriptive error when
/// the file is missing — the dashboard turns this into a red badge upstream.
fn open_repo_db(repo_path: &Path) -> Result<Connection, String> {
    let db_path = repo_path.join(".claude").join(".harness").join("mustard.db");
    if !db_path.exists() {
        return Err(format!(
            "mustard.db not found at {} — run any pipeline once to initialise the harness",
            db_path.display()
        ));
    }
    Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("failed to open {}: {}", db_path.display(), e))
}

/// Tauri command for the Wave 5 dashboard. Composes the four blocks above into
/// a single payload. Schema gaps (no `claude_code_otel` yet, no subtraction
/// events) degrade to zeros — the frontend interprets that as "empty state".
#[tauri::command]
pub fn dashboard_prompt_economy(
    repo_path: String,
) -> Result<DashboardPromptEconomy, String> {
    let base = std::path::PathBuf::from(&repo_path);
    let conn = open_repo_db(&base)?;
    let session_since = session_start_ts(&base);
    Ok(DashboardPromptEconomy {
        cost: cost_block(&conn),
        subtractions: subtractions_block(&conn, session_since.as_deref()),
        claude_events: claude_events_block(&conn),
        freshness: freshness_block(&conn, &base),
    })
}

// ── Honest Prompt Economy — full breakdown (Wave 7) ──────────────────────────
//
// W7 page consumes `mustard_core::economy::reader::economy_summary` directly so
// the dashboard surfaces the same numbers downstream agents and hooks already
// see. The Tauri command is a thin wrapper around the core reader plus a
// JSON-friendly scope DTO (the core `EconomyScope` enum carries newtype +
// `PathBuf` payloads that don't round-trip through JS cleanly when the
// transparent newtypes hit a tuple variant).

/// JS-friendly mirror of `mustard_core::economy::EconomyScope`. Internally
/// tagged on `kind` so the TS side can shape it as a clean discriminated union
/// (`{ kind: "project", project } | { kind: "spec", project, spec } | …`).
///
/// Strings (not `ProjectPath` / `SpecId` newtypes) so a flat JSON payload from
/// JS deserializes without the transparent-newtype gymnastics — the
/// `into_core` step rebuilds the typed core scope before calling the reader.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EconomyScopeDto {
    Project { project: String },
    Spec { project: String, spec: String },
    Wave { project: String, spec: String, wave: String },
    AllProjects { projects: Vec<String> },
}

impl EconomyScopeDto {
    /// Project path used to open the harness DB. `AllProjects` uses the first
    /// entry for connection bootstrap since the core reader's `fan_out` step
    /// re-opens each project itself in read-only mode.
    fn primary_project(&self) -> Option<&str> {
        match self {
            Self::Project { project } | Self::Spec { project, .. } | Self::Wave { project, .. } => {
                Some(project.as_str())
            }
            Self::AllProjects { projects } => projects.first().map(String::as_str),
        }
    }

    fn into_core(self) -> mustard_core::economy::EconomyScope {
        use mustard_core::economy::{EconomyScope, ProjectPath, SpecId, WaveId};
        match self {
            Self::Project { project } => EconomyScope::Project(ProjectPath::new(project)),
            Self::Spec { project, spec } => EconomyScope::Spec {
                project: ProjectPath::new(project),
                spec: SpecId::new(spec),
            },
            Self::Wave { project, spec, wave } => EconomyScope::Wave {
                project: ProjectPath::new(project),
                spec: SpecId::new(spec),
                wave: WaveId::new(wave),
            },
            Self::AllProjects { projects } => EconomyScope::AllProjects(
                projects.into_iter().map(ProjectPath::new).collect(),
            ),
        }
    }
}

/// Top-level economy summary for a given scope. Returns
/// `mustard_core::economy::EconomySummary` verbatim — its `Serialize` impl is
/// already snake_case. Failures (missing DB, unreadable rows) bubble up as
/// `Err(String)` so the React Query layer can surface a hard error instead of
/// silently emptying the page.
#[tauri::command]
pub fn dashboard_economy_summary(
    scope: EconomyScopeDto,
) -> Result<mustard_core::economy::EconomySummary, String> {
    let (project_path, core_scope) = open_scope(scope)?;
    let conn = mustard_core::economy::store::open_for(&project_path)
        .map_err(|e| format!("open economy db for {project_path}: {e}"))?;
    mustard_core::economy::reader::economy_summary(&conn, core_scope)
        .map_err(|e| format!("economy_summary: {e}"))
}

/// Per-`SavingsSource` breakdown — backs the W7 `<SavingsBreakdownCard>`.
/// Delegates to `mustard_core::economy::reader::savings_breakdown`.
#[tauri::command]
pub fn dashboard_economy_savings_breakdown(
    scope: EconomyScopeDto,
) -> Result<mustard_core::economy::SavingsBreakdown, String> {
    let (project_path, core_scope) = open_scope(scope)?;
    let conn = mustard_core::economy::store::open_for(&project_path)
        .map_err(|e| format!("open economy db for {project_path}: {e}"))?;
    mustard_core::economy::reader::savings_breakdown(&conn, core_scope)
        .map_err(|e| format!("savings_breakdown: {e}"))
}

/// Context-routing quality (cache hit ratio, prefix-stable ratio, retry
/// overhead) for the W7 cache-hit KPI card. Ratios are permille (0..1000) on
/// the wire — the UI divides by 1000.0 when rendering.
#[tauri::command]
pub fn dashboard_economy_context_routing(
    scope: EconomyScopeDto,
) -> Result<mustard_core::economy::ContextRoutingMetrics, String> {
    let (project_path, core_scope) = open_scope(scope)?;
    let conn = mustard_core::economy::store::open_for(&project_path)
        .map_err(|e| format!("open economy db for {project_path}: {e}"))?;
    mustard_core::economy::reader::context_routing_quality(&conn, core_scope)
        .map_err(|e| format!("context_routing_quality: {e}"))
}

/// Resolve the primary project path + typed core scope from the JS DTO.
/// `AllProjects` uses the first entry for connection bootstrap since the core
/// reader's fan_out re-opens each project itself in read-only mode.
fn open_scope(
    scope: EconomyScopeDto,
) -> Result<(String, mustard_core::economy::EconomyScope), String> {
    let project_path = scope
        .primary_project()
        .ok_or_else(|| "economy scope has no project path".to_string())?
        .to_string();
    Ok((project_path, scope.into_core()))
}

// ── Wave 6 — Trace viewer ───────────────────────────────────────────────────
//
// `dashboard_spec_trace` pivots the events table into a 4-level tree:
//
//   spec  →  wave  →  agent  →  tool
//
// Token totals roll up bottom-up from `per_agent_costs` (Wave 4 reader, which
// does the real attribution via the `attribution_cte`). Tool events come
// straight off `events.event = 'tool.use'` filtered by `spec`; we keep the
// payload JSON intact so the frontend can render diffs / code blocks lazily.
//
// Empty branches degrade silently — a spec with no events resolves to a
// 1-node root with empty children, which the UI renders as an empty state.

/// Per-agent token roll-up (input/output/cache split). Counters mirror the
/// `claude_code_otel` columns; cost is optional because `per_agent_costs`
/// returns 0 when the OTEL collector hasn't observed a span yet.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct TokenBreakdown {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
    pub cost_usd_micros: Option<i64>,
}

/// Recursive trace node — same shape at every depth so the frontend can
/// render with a single `<TraceNodeRow>` component. `kind` discriminates the
/// level (`"spec"|"wave"|"agent"|"tool"`); `payload` is `Some` only for tool
/// events and carries the original event payload verbatim (`tool_name`,
/// `tool_input`, `tool_response`, etc.).
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct TraceNode {
    pub kind: String,
    pub label: String,
    pub tokens: Option<TokenBreakdown>,
    pub duration_ms: Option<i64>,
    pub ts: Option<String>,
    pub payload: Option<serde_json::Value>,
    pub children: Vec<TraceNode>,
}

/// One row of the `agent.start` / `agent.stop` walk used to seed the tree.
struct AgentEvent {
    ts: String,
    wave_id: Option<String>,
    actor_id: Option<String>,
    payload: serde_json::Value,
    kind: AgentEventKind,
}

enum AgentEventKind {
    Start,
    Stop,
}

/// One row of the `tool.use` walk. Kept separate so we don't pay the
/// JSON-decode cost per agent — the tool list is usually 10× larger.
struct ToolEvent {
    ts: String,
    wave_id: Option<String>,
    actor_id: Option<String>,
    tool_name: Option<String>,
    /// `tool_use_id` lifted from the payload when present. Used to pair a
    /// `tool.use` with its matching `tool.result` exactly (see
    /// `pair_tool_results`). Falls back to chronological ordering per actor
    /// when missing on either side.
    tool_use_id: Option<String>,
    payload: serde_json::Value,
}

/// One row of the `tool.result` walk — emitted by the post-tool hook for
/// captured stdout/stderr/diff content. Carries the verbatim payload that
/// the frontend renders inline once paired with its `tool.use` parent.
struct ToolResult {
    ts: String,
    actor_id: Option<String>,
    tool_use_id: Option<String>,
    payload: serde_json::Value,
}

/// Build the trace tree for `spec_name` in `project_path`.
///
/// 1. Open the shared harness DB via `mustard_core::economy::store::open_for`
///    (same path resolution every other reader uses — `MUSTARD_DB_PATH` env
///    override honored).
/// 2. Pull `agent.start`/`agent.stop` events filtered by `spec` and pair them
///    by `actor_id` to derive a per-agent duration.
/// 3. Pull `tool.use` events filtered by `spec`.
/// 4. Group tools under their agent (by `actor_id`), agents under their wave
///    (by `wave_id`), and wrap in a `spec` root.
/// 5. Roll up tokens from `per_agent_costs(EconomyScope::Spec)` onto matching
///    agent nodes; wave + spec nodes get the sum of their children.
#[tauri::command]
pub fn dashboard_spec_trace(
    project_path: String,
    spec_name: String,
) -> Result<TraceNode, String> {
    use mustard_core::economy::{
        per_agent_costs, store::open_for, EconomyScope, ProjectPath, SpecId,
    };

    let conn = open_for(&project_path)
        .map_err(|e| format!("open harness db for {project_path}: {e}"))?;

    let agent_events = load_agent_events(&conn, &spec_name)
        .map_err(|e| format!("load agent events: {e}"))?;
    let tool_events = load_tool_events(&conn, &spec_name)
        .map_err(|e| format!("load tool events: {e}"))?;
    // Post-tool hook emits `tool.result` for captured stdout / file diffs.
    // load_tool_results runs the same SELECT shape so a missing table or
    // empty result is silently treated as "no results captured yet".
    let tool_results = load_tool_results(&conn, &spec_name)
        .map_err(|e| format!("load tool results: {e}"))?;

    // Pivot per-agent token totals via the Wave 4 reader. Failure is non-fatal
    // — a fresh project may not have any spans yet, in which case every
    // agent simply gets `tokens=None`.
    let agent_costs = per_agent_costs(
        &conn,
        EconomyScope::Spec {
            project: ProjectPath::new(&project_path),
            spec: SpecId::new(spec_name.clone()),
        },
    )
    .unwrap_or_default();
    let mut tokens_by_agent: HashMap<String, TokenBreakdown> = HashMap::new();
    for cost in agent_costs {
        // The reader's `tokens` is input+output combined; split is unavailable
        // from this path so we surface the total as `input` (the dominant
        // share) and let the UI label it "tokens". `cache_*` stays 0 until a
        // dedicated per-token-type rollup lands.
        tokens_by_agent.insert(
            cost.agent_id.0,
            TokenBreakdown {
                input: cost.tokens,
                output: 0,
                cache_read: 0,
                cache_creation: 0,
                cost_usd_micros: Some(cost.cost_usd_micros),
            },
        );
    }

    Ok(build_trace_tree(
        &spec_name,
        agent_events,
        tool_events,
        tool_results,
        tokens_by_agent,
    ))
}

fn load_agent_events(conn: &Connection, spec_name: &str) -> rusqlite::Result<Vec<AgentEvent>> {
    let mut stmt = conn.prepare(
        "SELECT ts, event, actor_id, payload \
         FROM events \
         WHERE spec = ?1 AND event IN ('agent.start', 'agent.stop') \
         ORDER BY id",
    )?;
    let rows = stmt
        .query_map([spec_name], |row| {
            let ts: String = row.get(0)?;
            let event: String = row.get(1)?;
            let actor_id: Option<String> = row.get(2)?;
            let payload_raw: Option<String> = row.get(3)?;
            let payload: serde_json::Value = payload_raw
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(serde_json::Value::Null);
            let wave_id = payload
                .get("wave_id")
                .and_then(|v| v.as_str())
                .map(String::from);
            let kind = if event == "agent.start" {
                AgentEventKind::Start
            } else {
                AgentEventKind::Stop
            };
            Ok(AgentEvent {
                ts,
                wave_id,
                actor_id,
                payload,
                kind,
            })
        })?
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(rows)
}

fn load_tool_events(conn: &Connection, spec_name: &str) -> rusqlite::Result<Vec<ToolEvent>> {
    let mut stmt = conn.prepare(
        "SELECT ts, actor_id, payload \
         FROM events \
         WHERE spec = ?1 AND event = 'tool.use' \
         ORDER BY id",
    )?;
    let rows = stmt
        .query_map([spec_name], |row| {
            let ts: String = row.get(0)?;
            let actor_id: Option<String> = row.get(1)?;
            let payload_raw: Option<String> = row.get(2)?;
            let payload: serde_json::Value = payload_raw
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(serde_json::Value::Null);
            let wave_id = payload
                .get("wave_id")
                .and_then(|v| v.as_str())
                .map(String::from);
            // The real `tool.use` payload uses `tool` (the shape the hook
            // actually writes). `tool_name`/`name` kept as legacy fallbacks
            // so older rows still render their label.
            let tool_name = payload
                .get("tool")
                .or_else(|| payload.get("tool_name"))
                .or_else(|| payload.get("name"))
                .and_then(|v| v.as_str())
                .map(String::from);
            let tool_use_id = payload
                .get("tool_use_id")
                .and_then(|v| v.as_str())
                .map(String::from);
            Ok(ToolEvent {
                ts,
                wave_id,
                actor_id,
                tool_name,
                tool_use_id,
                payload,
            })
        })?
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(rows)
}

/// Load `tool.result` events for `spec_name` ordered by id. Each row carries
/// captured side-effects of a prior `tool.use` (stdout/stderr for Bash, the
/// before/after file content for Edit/Write/MultiEdit, the file content for
/// Read). Pairing happens in `pair_tool_results` — preferred match is by
/// `tool_use_id` in the payload, with a chronological-per-actor fallback for
/// events emitted before the id was wired in.
fn load_tool_results(conn: &Connection, spec_name: &str) -> rusqlite::Result<Vec<ToolResult>> {
    let mut stmt = conn.prepare(
        "SELECT ts, actor_id, payload \
         FROM events \
         WHERE spec = ?1 AND event = 'tool.result' \
         ORDER BY id",
    )?;
    let rows = stmt
        .query_map([spec_name], |row| {
            let ts: String = row.get(0)?;
            let actor_id: Option<String> = row.get(1)?;
            let payload_raw: Option<String> = row.get(2)?;
            let payload: serde_json::Value = payload_raw
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(serde_json::Value::Null);
            let tool_use_id = payload
                .get("tool_use_id")
                .and_then(|v| v.as_str())
                .map(String::from);
            Ok(ToolResult {
                ts,
                actor_id,
                tool_use_id,
                payload,
            })
        })?
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(rows)
}

/// Pair each `tool.use` with its matching `tool.result` payload.
///
/// Strategy:
/// 1. **Exact match by `tool_use_id`** — when both sides carry the id (the
///    post-tool hook stamps it on every new event), we splice the result
///    payload directly under `payload.result` on the matching `tool.use`.
/// 2. **Chronological fallback per `actor_id`** — for legacy rows missing
///    the id we line up the unmatched results, in order, with unmatched
///    `tool.use`s from the same actor. This is best-effort: if a tool was
///    silently skipped by the hook the alignment can drift, but the result
///    only feeds opt-in render blocks (diffs, stdout) so the worst case is
///    a wrong stdout snippet, not a crash.
///
/// Mutates `tool_events` in place — every paired result is written into the
/// `tool.use` payload as `payload.result = <result_payload>`.
fn pair_tool_results(tool_events: &mut [ToolEvent], tool_results: Vec<ToolResult>) {
    if tool_results.is_empty() {
        return;
    }

    // Pass 1 — exact match by tool_use_id.
    let mut by_id: HashMap<String, serde_json::Value> = HashMap::new();
    let mut leftover: Vec<ToolResult> = Vec::new();
    for r in tool_results {
        match r.tool_use_id.as_deref() {
            Some(id) if !id.is_empty() => {
                by_id.insert(id.to_string(), r.payload);
            }
            _ => leftover.push(r),
        }
    }
    let mut matched_use_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for ev in tool_events.iter_mut() {
        if let Some(id) = ev.tool_use_id.as_deref() {
            if let Some(payload) = by_id.remove(id) {
                if let Some(obj) = ev.payload.as_object_mut() {
                    obj.insert("result".to_string(), payload);
                }
                matched_use_ids.insert(id.to_string());
            }
        }
    }
    // Any id-only results that didn't find a use → drop. Mismatched ids
    // usually mean the use was on a different spec or filtered out earlier;
    // we'd rather lose the orphan than guess.

    // Pass 2 — chronological fallback per actor_id for unmatched events.
    let mut results_by_actor: HashMap<String, Vec<ToolResult>> = HashMap::new();
    for r in leftover {
        let key = r.actor_id.clone().unwrap_or_default();
        results_by_actor.entry(key).or_default().push(r);
    }
    // Each actor's leftover results are already in id order (ORDER BY id
    // upstream); we walk the matching unpaired `tool.use`s in the same
    // order and zip them together by ts.
    let mut cursor_by_actor: HashMap<String, usize> = HashMap::new();
    for ev in tool_events.iter_mut() {
        // Skip events that already received a result via pass 1.
        let already_paired = ev
            .payload
            .as_object()
            .map(|o| o.contains_key("result"))
            .unwrap_or(false);
        if already_paired {
            continue;
        }
        let key = ev.actor_id.clone().unwrap_or_default();
        let Some(bucket) = results_by_actor.get(&key) else {
            continue;
        };
        let idx = cursor_by_actor.entry(key.clone()).or_insert(0);
        if *idx >= bucket.len() {
            continue;
        }
        // Cheap chronological guard: result.ts must be >= use.ts. When the
        // next result is *earlier* than the current use, it belonged to a
        // dropped/earlier use and is skipped permanently.
        while *idx < bucket.len() && bucket[*idx].ts < ev.ts {
            *idx += 1;
        }
        if *idx < bucket.len() {
            let payload = bucket[*idx].payload.clone();
            if let Some(obj) = ev.payload.as_object_mut() {
                obj.insert("result".to_string(), payload);
            }
            *idx += 1;
        }
    }
}

fn build_trace_tree(
    spec_name: &str,
    agent_events: Vec<AgentEvent>,
    mut tool_events: Vec<ToolEvent>,
    tool_results: Vec<ToolResult>,
    tokens_by_agent: HashMap<String, TokenBreakdown>,
) -> TraceNode {
    // Splice every `tool.result` payload into its matching `tool.use` BEFORE
    // we drop the events into the tree, so each tool node already carries
    // `payload.result` for the frontend to render diffs/stdout inline.
    pair_tool_results(&mut tool_events, tool_results);

    // Pair start/stop per actor_id → derive a duration_ms when both halves
    // exist. Order preserved by `ORDER BY id` upstream.
    let mut starts: HashMap<String, &AgentEvent> = HashMap::new();
    let mut stops: HashMap<String, &AgentEvent> = HashMap::new();
    for ev in &agent_events {
        let Some(id) = ev.actor_id.as_deref() else {
            continue;
        };
        match ev.kind {
            AgentEventKind::Start => {
                starts.entry(id.to_string()).or_insert(ev);
            }
            AgentEventKind::Stop => {
                stops.insert(id.to_string(), ev);
            }
        }
    }

    // wave_id → agent_id → tool nodes. BTreeMap keeps waves in stable order.
    let mut waves: std::collections::BTreeMap<String, HashMap<String, Vec<TraceNode>>> =
        std::collections::BTreeMap::new();

    // Seed every wave/agent we see in `agent.start` so an agent with zero
    // tool events still surfaces in the tree.
    for (actor_id, start) in &starts {
        let wave_key = start
            .wave_id
            .clone()
            .unwrap_or_else(|| "wave-unknown".to_string());
        waves
            .entry(wave_key)
            .or_default()
            .entry(actor_id.clone())
            .or_default();
    }

    // Drop tool events into their agent bucket.
    for tool in tool_events {
        let wave_key = tool
            .wave_id
            .clone()
            .unwrap_or_else(|| "wave-unknown".to_string());
        let actor_key = tool
            .actor_id
            .clone()
            .unwrap_or_else(|| "agent-unknown".to_string());
        let tool_name = tool.tool_name.clone().unwrap_or_else(|| "tool".to_string());
        // Real shape: `payload.target = { command?, file_path?, description? }`.
        // Older legacy rows stored a flat string under `target`/`file_path`/
        // `path` — we keep both branches so historic events still render.
        let target_obj = tool.payload.get("target");
        let target_string: String = if let Some(obj) = target_obj.and_then(|v| v.as_object()) {
            obj.get("file_path")
                .or_else(|| obj.get("file"))
                .or_else(|| obj.get("command"))
                .or_else(|| obj.get("description"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        } else {
            target_obj
                .and_then(|v| v.as_str())
                .or_else(|| tool.payload.get("file_path").and_then(|v| v.as_str()))
                .or_else(|| tool.payload.get("path").and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_string()
        };
        let label = if target_string.is_empty() {
            tool_name.clone()
        } else {
            format!("{tool_name} · {target_string}")
        };
        let node = TraceNode {
            kind: "tool".to_string(),
            label,
            tokens: None,
            duration_ms: None,
            ts: Some(tool.ts),
            payload: Some(tool.payload),
            children: vec![],
        };
        waves
            .entry(wave_key)
            .or_default()
            .entry(actor_key)
            .or_default()
            .push(node);
    }

    // Build wave nodes.
    let mut wave_nodes: Vec<TraceNode> = Vec::new();
    for (wave_id, agents) in waves {
        let mut agent_nodes: Vec<TraceNode> = Vec::new();
        for (actor_id, tool_children) in agents {
            let agent_tokens = tokens_by_agent.get(&actor_id).cloned();
            // Best-effort duration: stop.ts − start.ts (ms).
            let duration_ms = match (starts.get(&actor_id), stops.get(&actor_id)) {
                (Some(s), Some(e)) => {
                    let s_ms = parse_iso_ms_pub(&s.ts);
                    let e_ms = parse_iso_ms_pub(&e.ts);
                    match (s_ms, e_ms) {
                        (Some(a), Some(b)) if b >= a => Some((b - a) as i64),
                        _ => None,
                    }
                }
                _ => None,
            };
            let ts = starts.get(&actor_id).map(|s| s.ts.clone());
            let label = starts
                .get(&actor_id)
                .and_then(|s| s.payload.get("agent_type").and_then(|v| v.as_str()))
                .map(|t| format!("{t} · {actor_id}"))
                .unwrap_or_else(|| actor_id.clone());
            agent_nodes.push(TraceNode {
                kind: "agent".to_string(),
                label,
                tokens: agent_tokens,
                duration_ms,
                ts,
                payload: None,
                children: tool_children,
            });
        }
        // Stable order: agents with the most tool events first.
        agent_nodes.sort_by(|a, b| b.children.len().cmp(&a.children.len()));

        let wave_tokens = sum_tokens(&agent_nodes);
        wave_nodes.push(TraceNode {
            kind: "wave".to_string(),
            label: wave_id,
            tokens: wave_tokens,
            duration_ms: None,
            ts: None,
            payload: None,
            children: agent_nodes,
        });
    }

    let spec_tokens = sum_tokens(&wave_nodes);
    TraceNode {
        kind: "spec".to_string(),
        label: spec_name.to_string(),
        tokens: spec_tokens,
        duration_ms: None,
        ts: None,
        payload: None,
        children: wave_nodes,
    }
}

/// Sum `tokens` across a slice of trace nodes. Returns `None` when no child
/// reports tokens, so the UI can hide the pill entirely instead of showing a
/// misleading "0 tok".
fn sum_tokens(nodes: &[TraceNode]) -> Option<TokenBreakdown> {
    let mut acc = TokenBreakdown::default();
    let mut any = false;
    let mut any_cost = false;
    let mut cost: i64 = 0;
    for n in nodes {
        if let Some(t) = &n.tokens {
            any = true;
            acc.input += t.input;
            acc.output += t.output;
            acc.cache_read += t.cache_read;
            acc.cache_creation += t.cache_creation;
            if let Some(c) = t.cost_usd_micros {
                any_cost = true;
                cost += c;
            }
        }
    }
    if !any {
        return None;
    }
    if any_cost {
        acc.cost_usd_micros = Some(cost);
    }
    Some(acc)
}
