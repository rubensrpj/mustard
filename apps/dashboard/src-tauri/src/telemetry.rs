use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
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
