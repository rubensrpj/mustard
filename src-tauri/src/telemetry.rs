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
/// events.jsonl. Tokens come from spans table (not yet wired); duration
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

pub fn hook_fire_counts(repo_path: &Path) -> Vec<HookFireCount> {
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
        let mut most_recent_ts: Option<String> = None;
        for line in content.lines() {
            let v: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            fires += 1;
            tokens_saved += v["tokens_saved"].as_u64().unwrap_or(0);
            if let Some(ts) = v["ts"].as_str() {
                most_recent_ts = Some(ts.to_string());
            }
        }
        results.push(HookFireCount { hook: stem, fires, tokens_saved, most_recent_ts });
    }

    results.sort_by(|a, b| b.tokens_saved.cmp(&a.tokens_saved).then(b.fires.cmp(&a.fires)));
    results
}

// ── Routing breakdown ────────────────────────────────────────────────────────

pub fn routing_breakdown(repo_path: &Path) -> RoutingBlock {
    let path = repo_path
        .join(".claude")
        .join(".metrics")
        .join("model-routing-gate.jsonl");
    let empty = || RoutingBlock {
        blocks: 0,
        allows: 0,
        by_intent: vec![],
        by_note: vec![],
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
        if is_block {
            total_blocks += 1;
            entry.0 += 1;
        } else {
            total_allows += 1;
            entry.1 += 1;
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
    // Since Wave 4 the canonical source is events.jsonl — hooks emit there
    // (metrics-tracker etc.). The SQLite mirror is a migration artefact and
    // stops receiving new writes. We try JSONL first and fall back to SQLite
    // only when JSONL is missing/empty.
    let jsonl = workflow_by_phase_from_jsonl(repo_path);
    let jsonl_total: u64 = jsonl.by_phase.iter().map(|p| p.count).sum();
    if jsonl_total > 0 {
        return jsonl;
    }
    if let Some(r) = crate::db::with_db(repo_path, crate::db::workflow_by_phase_from_db) {
        match r {
            Ok(block) => return block,
            Err(e) => eprintln!("workflow_by_phase: db fallback error: {}", e),
        }
    }
    jsonl // empty when JSONL also missing
}

fn workflow_by_phase_from_jsonl(repo_path: &Path) -> WorkflowBlock {
    let path = repo_path.join(".claude").join(".harness").join("events.jsonl");
    if !path.exists() {
        return WorkflowBlock { by_phase: vec![] };
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return WorkflowBlock { by_phase: vec![] },
    };
    let mut counts: HashMap<String, u64> = HashMap::new();
    for line in content.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let event_type = v["event"].as_str().or_else(|| v["type"].as_str());
        // Count both explicit pipeline.phase transitions and tool.use events
        // (which carry payload.phase). tool.use is the actual signal of where
        // the project spends time.
        let is_phase_event = event_type == Some("pipeline.phase") || event_type == Some("tool.use");
        if !is_phase_event {
            continue;
        }
        let phase = v
            .get("payload")
            .and_then(|p| {
                if let Some(obj) = p.as_object() {
                    obj.get("phase").and_then(|x| x.as_str()).map(|s| s.to_string())
                } else if let Some(s) = p.as_str() {
                    serde_json::from_str::<serde_json::Value>(s)
                        .ok()
                        .and_then(|pv| pv.get("phase").and_then(|x| x.as_str()).map(|s| s.to_string()))
                } else {
                    None
                }
            });
        if let Some(phase) = phase {
            *counts.entry(phase.to_ascii_uppercase()).or_insert(0) += 1;
        }
    }
    let mut by_phase: Vec<PhaseCount> = counts
        .into_iter()
        .map(|(phase, count)| PhaseCount { phase, count })
        .collect();
    by_phase.sort_by(|a, b| b.count.cmp(&a.count));
    WorkflowBlock { by_phase }
}

// ── Tool breakdown ────────────────────────────────────────────────────────────

pub fn tool_breakdown(repo_path: &Path) -> Vec<ToolCount> {
    // Same rationale as workflow_by_phase: JSONL is canonical in Wave 4+,
    // SQLite mirror is stale. Prefer JSONL, fall back to SQLite only when empty.
    let jsonl = tool_breakdown_from_jsonl(repo_path, 15);
    if !jsonl.is_empty() {
        return jsonl;
    }
    if let Some(r) = crate::db::with_db(repo_path, |c| crate::db::tool_breakdown_from_db(c, 15)) {
        match r {
            Ok(list) => return list,
            Err(e) => eprintln!("tool_breakdown: db fallback error: {}", e),
        }
    }
    vec![]
}

fn tool_breakdown_from_jsonl(repo_path: &Path, limit: usize) -> Vec<ToolCount> {
    let path = repo_path.join(".claude").join(".harness").join("events.jsonl");
    if !path.exists() {
        return vec![];
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let mut counts: HashMap<String, u64> = HashMap::new();
    for line in content.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v["event"].as_str().or_else(|| v["type"].as_str()) != Some("tool.use") {
            continue;
        }
        let tool = v.get("payload").and_then(|p| {
            if let Some(obj) = p.as_object() {
                obj.get("tool")
                    .and_then(|x| x.as_str())
                    .or_else(|| obj.get("tool_name").and_then(|x| x.as_str()))
                    .map(|s| s.to_string())
            } else if let Some(s) = p.as_str() {
                serde_json::from_str::<serde_json::Value>(s).ok().and_then(|pv| {
                    pv.get("tool")
                        .and_then(|x| x.as_str())
                        .or_else(|| pv.get("tool_name").and_then(|x| x.as_str()))
                        .map(|s| s.to_string())
                })
            } else {
                None
            }
        });
        if let Some(tool) = tool {
            *counts.entry(tool).or_insert(0) += 1;
        }
    }
    let mut list: Vec<ToolCount> = counts
        .into_iter()
        .map(|(tool_name, count)| ToolCount { tool_name, count })
        .collect();
    list.sort_by(|a, b| b.count.cmp(&a.count));
    list.truncate(limit);
    list
}

// ── Agent activity (T2-1: span-lite consumer) ────────────────────────────────
//
// Reads agent.start / agent.stop pairs from events.jsonl and aggregates per
// agent_type. Tokens are deliberately omitted here — they live in the spans
// SQLite table (Phase 2) which the hooks don't currently write to. Duration
// is best-effort: derived from start.ts → stop.ts pairing on actor.id +
// sessionId. When the pair is missing (agent still running, or stop dropped),
// the start counts but duration stays at 0.

pub fn agent_activity_from_jsonl(repo_path: &Path) -> AgentActivityBlock {
    let path = repo_path
        .join(".claude")
        .join(".harness")
        .join("events.jsonl");
    if !path.exists() {
        return AgentActivityBlock { total_dispatches: 0, total_errors: 0, agents: vec![] };
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return AgentActivityBlock { total_dispatches: 0, total_errors: 0, agents: vec![] },
    };

    #[derive(Default)]
    struct Acc {
        starts: u64,
        stops: u64,
        errors: u64,
        durations_ms: Vec<u64>,
        last_ts: Option<String>,
    }

    let mut acc: HashMap<String, Acc> = HashMap::new();
    // pair-up table: key = "{sessionId}|{actor.id}" → start ts
    let mut pending_starts: HashMap<String, String> = HashMap::new();

    for line in content.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let event_type = v["event"].as_str().unwrap_or("");
        if event_type != "agent.start" && event_type != "agent.stop" {
            continue;
        }
        let actor_id = v
            .get("actor")
            .and_then(|a| a.get("id"))
            .and_then(|x| x.as_str())
            .unwrap_or("unknown");
        let session_id = v.get("sessionId").and_then(|x| x.as_str()).unwrap_or("");
        let ts = v.get("ts").and_then(|x| x.as_str()).map(|s| s.to_string());
        let pair_key = format!("{}|{}", session_id, actor_id);

        let entry = acc.entry(actor_id.to_string()).or_default();
        // Always track last_ts (max wins lexicographically — ISO-8601 sorts correctly).
        if let Some(ref t) = ts {
            if entry.last_ts.as_ref().map_or(true, |cur| t > cur) {
                entry.last_ts = Some(t.clone());
            }
        }

        if event_type == "agent.start" {
            entry.starts += 1;
            if let Some(t) = ts {
                pending_starts.insert(pair_key, t);
            }
        } else {
            entry.stops += 1;
            let is_error = v
                .get("payload")
                .and_then(|p| p.get("isError"))
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            if is_error {
                entry.errors += 1;
            }
            // Compute duration when we have a matching start.
            if let (Some(start_ts), Some(stop_ts)) = (pending_starts.remove(&pair_key), ts) {
                if let (Some(t0), Some(t1)) = (parse_iso_ms(&start_ts), parse_iso_ms(&stop_ts)) {
                    if t1 >= t0 {
                        entry.durations_ms.push(t1 - t0);
                    }
                }
            }
        }
    }

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
    // Most active first; ties broken by most-recently-seen.
    agents.sort_by(|a, b| {
        b.starts
            .cmp(&a.starts)
            .then_with(|| b.last_ts.cmp(&a.last_ts))
    });
    agents.truncate(10);

    AgentActivityBlock {
        total_dispatches,
        total_errors,
        agents,
    }
}

/// Parse ISO-8601 timestamp to milliseconds since epoch. Best-effort — returns
/// None when the timestamp is malformed. Accepts both with and without
/// fractional seconds and the trailing 'Z'.
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
// Reads `<repo>/.claude/.harness/events.jsonl` directly — the canonical "tail"
// of what is happening right now. Unlike the SQLite mirror (which a batch
// migration populates), this file is updated by metrics-tracker on every
// PreToolUse, so it always reflects the current session.

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

const CANONICAL_PHASES: &[&str] = &["ANALYZE", "PLAN", "EXECUTE", "QA", "CLOSE"];

/// Lower bound on event file size we'll try to parse fully. Anything larger
/// gets a tail read. events.jsonl is append-only and rarely exceeds a few MB
/// in practice, but we cap to keep the dashboard snappy on large projects.
const EVENTS_TAIL_BYTES: u64 = 4 * 1024 * 1024;

pub fn live_activity(repo_path: &Path) -> LiveActivity {
    let path = repo_path.join(".claude").join(".harness").join("events.jsonl");
    if !path.exists() {
        return LiveActivity::default();
    }

    // Read tail (or whole file if small). Tail boundary may slice the first
    // line — we discard partial first lines after seek.
    let content = match read_tail(&path, EVENTS_TAIL_BYTES) {
        Ok(s) => s,
        Err(_) => return LiveActivity::default(),
    };

    let now = chrono::Utc::now();
    let today_start = now.date_naive().and_hms_opt(0, 0, 0).unwrap();
    let hour_ago = now - chrono::Duration::hours(1);
    let five_min_ago = now - chrono::Duration::minutes(5);

    let mut events_today: u64 = 0;
    let mut events_last_hour: u64 = 0;
    let mut events_last_5min: u64 = 0;
    let mut tools_today_map: HashMap<String, u64> = HashMap::new();
    let mut minute_buckets: Vec<u64> = vec![0; 60];
    let mut last_event_ts: Option<String> = None;
    let mut last_event_dt: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut current_spec: Option<String> = None;
    let mut current_phase: Option<String> = None;
    let mut current_wave: Option<u32> = None;

    /// Agregado interno por fase canônica. Convertido em PhaseActivity no final.
    #[derive(Default)]
    struct PhaseAgg {
        events_today: u64,
        events_last_hour: u64,
        events_last_5min: u64,
        minute_buckets: Vec<u64>,
        last_event_ts: Option<String>,
        last_event_dt: Option<chrono::DateTime<chrono::Utc>>,
        tools: HashMap<String, u64>,
        last_spec: Option<String>,
    }
    let mut phase_aggs: HashMap<String, PhaseAgg> = HashMap::new();
    for p in CANONICAL_PHASES {
        let mut agg = PhaseAgg::default();
        agg.minute_buckets = vec![0; 60];
        phase_aggs.insert((*p).to_string(), agg);
    }

    let mut first_line = true;
    for line in content.lines() {
        // Discard a possibly-truncated first line when we tailed.
        if first_line {
            first_line = false;
            if content.len() == EVENTS_TAIL_BYTES as usize {
                continue;
            }
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let ts_str = match v["ts"].as_str().or_else(|| v["timestamp"].as_str()) {
            Some(s) => s,
            None => continue,
        };
        let dt = match chrono::DateTime::parse_from_rfc3339(ts_str) {
            Ok(d) => d.with_timezone(&chrono::Utc),
            Err(_) => continue,
        };

        // Always remember the latest event seen (events.jsonl is append-only;
        // last line = latest, but we don't rely on order). spec/phase/wave are
        // sticky: an event without the field doesn't clear the last-known value.
        if last_event_dt.map_or(true, |prev| dt > prev) {
            last_event_dt = Some(dt);
            last_event_ts = Some(ts_str.to_string());
        }
        if let Some(s) = v["spec"].as_str() {
            current_spec = Some(s.to_string());
        }
        if let Some(n) = v["wave"].as_u64() {
            if let Ok(w) = u32::try_from(n) {
                current_wave = Some(w);
            }
        }
        if let Some(s) = v
            .get("payload")
            .and_then(|p| p.get("phase"))
            .and_then(|x| x.as_str())
        {
            current_phase = Some(s.to_string());
        }

        let dt_naive = dt.naive_utc();
        if dt_naive < today_start {
            continue;
        }
        events_today += 1;

        if dt >= hour_ago {
            events_last_hour += 1;
            // Minute bucket: distance from now in minutes (0 = 59 minutes ago).
            let minutes_ago = (now - dt).num_minutes();
            if (0..60).contains(&minutes_ago) {
                let idx = 59 - minutes_ago as usize;
                if let Some(b) = minute_buckets.get_mut(idx) {
                    *b += 1;
                }
            }
        }
        if dt >= five_min_ago {
            events_last_5min += 1;
        }

        let is_tool_use = v["event"].as_str().or_else(|| v["type"].as_str()) == Some("tool.use");
        let event_tool = if is_tool_use {
            v.get("payload")
                .and_then(|p| p.get("tool"))
                .or_else(|| v.get("payload").and_then(|p| p.get("tool_name")))
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        } else {
            None
        };
        if let Some(ref t) = event_tool {
            *tools_today_map.entry(t.clone()).or_insert(0) += 1;
        }

        // Per-phase aggregation: only events with a canonical phase land here.
        let event_phase = v
            .get("payload")
            .and_then(|p| p.get("phase"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_ascii_uppercase());
        if let Some(p) = event_phase {
            if CANONICAL_PHASES.contains(&p.as_str()) {
                if let Some(agg) = phase_aggs.get_mut(&p) {
                    agg.events_today += 1;
                    if dt >= hour_ago {
                        agg.events_last_hour += 1;
                        let minutes_ago = (now - dt).num_minutes();
                        if (0..60).contains(&minutes_ago) {
                            let idx = 59 - minutes_ago as usize;
                            if let Some(b) = agg.minute_buckets.get_mut(idx) {
                                *b += 1;
                            }
                        }
                    }
                    if dt >= five_min_ago {
                        agg.events_last_5min += 1;
                    }
                    if agg.last_event_dt.map_or(true, |prev| dt > prev) {
                        agg.last_event_dt = Some(dt);
                        agg.last_event_ts = Some(ts_str.to_string());
                    }
                    if let Some(ref t) = event_tool {
                        *agg.tools.entry(t.clone()).or_insert(0) += 1;
                    }
                    if let Some(s) = v["spec"].as_str() {
                        agg.last_spec = Some(s.to_string());
                    }
                }
            }
        }
    }

    let mut tools_today: Vec<ToolCount> = tools_today_map
        .into_iter()
        .map(|(tool_name, count)| ToolCount { tool_name, count })
        .collect();
    tools_today.sort_by(|a, b| b.count.cmp(&a.count));
    tools_today.truncate(10);

    let is_fresh = last_event_dt
        .map(|d| (now - d).num_seconds() < 120)
        .unwrap_or(false);

    // Build by_phase in canonical order. Each phase always present (events=0 is
    // information — "EXECUTE is idle" is exactly what we want to show).
    let by_phase: Vec<PhaseActivity> = CANONICAL_PHASES
        .iter()
        .map(|p| {
            let key = (*p).to_string();
            let agg = phase_aggs.remove(&key).unwrap_or_else(|| {
                let mut a = PhaseAgg::default();
                a.minute_buckets = vec![0; 60];
                a
            });
            let mut top_tools: Vec<ToolCount> = agg
                .tools
                .into_iter()
                .map(|(tool_name, count)| ToolCount { tool_name, count })
                .collect();
            top_tools.sort_by(|a, b| b.count.cmp(&a.count));
            top_tools.truncate(3);
            PhaseActivity {
                phase: key,
                events_today: agg.events_today,
                events_last_hour: agg.events_last_hour,
                events_last_5min: agg.events_last_5min,
                minute_buckets: agg.minute_buckets,
                last_event_ts: agg.last_event_ts,
                top_tools,
                last_spec: agg.last_spec,
            }
        })
        .collect();

    LiveActivity {
        last_event_ts,
        events_today,
        events_last_hour,
        events_last_5min,
        tools_today,
        minute_buckets,
        current_spec,
        current_phase,
        current_wave,
        is_fresh,
        by_phase,
    }
}

/// Read the last `max_bytes` of a file as UTF-8 (lossy). Returns the whole file
/// when smaller.
fn read_tail(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let start = len.saturating_sub(max_bytes);
    if start > 0 {
        f.seek(SeekFrom::Start(start))?;
    }
    let mut buf = Vec::with_capacity((len - start) as usize);
    f.read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
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

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct SubtractionsBlock {
    pub wave_slice_bytes: u64,
    pub wave_slice_count: u64,
    pub diff_vs_full_bytes: u64,
    pub diff_vs_full_count: u64,
    pub review_diff_first_bytes: u64,
    pub review_diff_first_count: u64,
    pub analyze_diff_skip_bytes: u64,
    pub analyze_diff_skip_count: u64,
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
/// `event='mustard.subtraction.applied'`. Routes the three known types into
/// the dedicated fields. Unknown types are silently ignored (no surprise UI).
fn subtractions_block(conn: &Connection) -> SubtractionsBlock {
    let mut out = SubtractionsBlock::default();

    let sql = "SELECT json_extract(payload, '$.type') AS t, \
                      COALESCE(SUM(CAST(json_extract(payload, '$.bytes_omitted') AS INTEGER)), 0) AS bytes, \
                      COUNT(*) AS cnt \
               FROM events WHERE event = 'mustard.subtraction.applied' GROUP BY t";
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return out,
    };
    let rows = match stmt.query_map([], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?.unwrap_or_default(),
            row.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0) as u64,
            row.get::<_, Option<i64>>(2)?.unwrap_or(0).max(0) as u64,
        ))
    }) {
        Ok(r) => r,
        Err(_) => return out,
    };
    for r in rows.flatten() {
        let (t, bytes, count) = r;
        match t.as_str() {
            "wave-slice" => {
                out.wave_slice_bytes = bytes;
                out.wave_slice_count = count;
            }
            "diff-vs-full" => {
                out.diff_vs_full_bytes = bytes;
                out.diff_vs_full_count = count;
            }
            "review-diff-first" => {
                out.review_diff_first_bytes = bytes;
                out.review_diff_first_count = count;
            }
            "analyze-diff-skip" => {
                out.analyze_diff_skip_bytes = bytes;
                out.analyze_diff_skip_count = count;
            }
            _ => {}
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
    Ok(DashboardPromptEconomy {
        cost: cost_block(&conn),
        subtractions: subtractions_block(&conn),
        claude_events: claude_events_block(&conn),
        freshness: freshness_block(&conn, &base),
    })
}
