mod discovery;
pub mod db;
pub mod telemetry;
mod watcher;

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::path::PathBuf;

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PipelineSummary {
    spec_name: String,
    phase: String,
    scope: String,
    status: String,
    updated_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MetricsSummary {
    pub total_events: usize,
    pub sessions_recent: usize,
    pub agents_dispatched: usize,
    pub last_event_at: Option<String>,
    pub tokens_total: u64,
    pub tokens_today: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeSummary {
    pub patterns_count: usize,
    pub conventions_count: usize,
    pub high_confidence_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SpecRow {
    pub name: String,
    pub status: Option<String>,
    pub phase: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub affected_files: Vec<String>,
    pub bucket: Option<String>,
    /// When this row is a wave-N child of a wave plan, the parent spec name.
    /// The dashboard groups children under their parent visually. `None` for
    /// standalone specs.
    pub parent: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeRow {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub name: String,
    pub description: String,
    pub confidence: f64,
    pub source: Option<String>,
}

// ── Consumption / cost summary (Phase 2 spans) ──────────────────────────────

#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "snake_case")]
pub struct ModelUsage {
    pub model: String,
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_usd: f64,
    /// Share of `total_tokens` against the workspace total (0..1).
    pub pct_tokens: f64,
}

#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "snake_case")]
pub struct AgentUsage {
    pub agent_type: String,
    pub calls: u64,
    pub total_tokens: u64,
    pub cost_usd: f64,
    pub pct_tokens: f64,
}

#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "snake_case")]
pub struct SpecUsage {
    pub spec: String,
    pub calls: u64,
    pub total_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "snake_case")]
pub struct DailyPoint {
    pub date: String, // YYYY-MM-DD
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct ConsumptionSummary {
    pub tokens_total: u64,
    pub tokens_today: u64,
    pub cost_total_usd: f64,
    pub cost_today_usd: f64,
    pub by_model: Vec<ModelUsage>,
    pub by_agent_type: Vec<AgentUsage>,
    pub top_specs: Vec<SpecUsage>,
    pub daily_series: Vec<DailyPoint>,
}

#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "snake_case")]
pub struct ProjectUsage {
    pub id: String,
    pub name: String,
    pub path: String,
    pub tokens_total: u64,
    pub tokens_today: u64,
    pub cost_total_usd: f64,
    pub cost_today_usd: f64,
    pub last_activity_ms: Option<u64>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct GlobalConsumption {
    pub tokens_total: u64,
    pub tokens_today: u64,
    pub cost_total_usd: f64,
    pub cost_today_usd: f64,
    pub by_project: Vec<ProjectUsage>,
    pub by_model: Vec<ModelUsage>,
    pub daily_series: Vec<DailyPoint>,
    pub rtk: telemetry::RtkBlock,
}

#[tauri::command]
fn dashboard_pipelines(repo_path: String) -> Result<Vec<PipelineSummary>, String> {
    let base = PathBuf::from(&repo_path);
    let dir = base.join(".claude").join(".pipeline-states");
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut results = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let v: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };
        results.push(PipelineSummary {
            spec_name: v["specName"].as_str().unwrap_or("").to_string(),
            phase: v["phaseName"].as_str().unwrap_or("").to_string(),
            scope: v["scope"].as_str().unwrap_or("").to_string(),
            status: v["status"].as_str().unwrap_or("").to_string(),
            updated_at: v["checkpointedAt"].as_str().map(|s| s.to_string()),
        });
    }
    Ok(results)
}

#[tauri::command]
fn dashboard_metrics(repo_path: String) -> Result<MetricsSummary, String> {
    let base = PathBuf::from(&repo_path);
    if let Some(r) = db::with_db(&base, db::metrics_from_db) {
        return r;
    }
    let path = base.join(".claude").join(".harness").join("events.jsonl");
    if !path.exists() {
        return Ok(MetricsSummary { total_events: 0, sessions_recent: 0, agents_dispatched: 0, last_event_at: None, tokens_total: 0, tokens_today: 0 });
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = content.lines().collect();
    let total_events = lines.len();
    let window = if lines.len() > 2000 { &lines[lines.len() - 2000..] } else { &lines[..] };
    let mut sessions: HashSet<String> = HashSet::new();
    let mut agents_dispatched: usize = 0;
    let mut last_event_at: Option<String> = None;
    for line in window {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(sid) = v["sessionId"].as_str() {
            sessions.insert(sid.to_string());
        }
        if v["event"].as_str().or_else(|| v["type"].as_str()) == Some("agent.start") {
            agents_dispatched += 1;
        }
        let ts = v["ts"].as_str().or_else(|| v["timestamp"].as_str());
        if let Some(t) = ts {
            last_event_at = Some(t.to_string());
        }
    }
    Ok(MetricsSummary { total_events, sessions_recent: sessions.len(), agents_dispatched, last_event_at, tokens_total: 0, tokens_today: 0 })
}

#[tauri::command]
fn dashboard_knowledge(repo_path: String) -> Result<KnowledgeSummary, String> {
    let base = PathBuf::from(&repo_path);
    if let Some(r) = db::with_db(&base, db::knowledge_from_db) {
        return r;
    }
    let path = base.join(".claude").join("knowledge.json");
    if !path.exists() {
        return Ok(KnowledgeSummary { patterns_count: 0, conventions_count: 0, high_confidence_count: 0 });
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Ok(KnowledgeSummary { patterns_count: 0, conventions_count: 0, high_confidence_count: 0 }),
    };
    let v: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Ok(KnowledgeSummary { patterns_count: 0, conventions_count: 0, high_confidence_count: 0 }),
    };
    let mut patterns_count = 0usize;
    let mut conventions_count = 0usize;
    let mut high_confidence_count = 0usize;
    if let Some(obj) = v.as_object() {
        for entry in obj.values() {
            match entry["type"].as_str() {
                Some("pattern") => patterns_count += 1,
                Some("convention") => conventions_count += 1,
                _ => {}
            }
            if entry["confidence"].as_f64().unwrap_or(0.0) >= 0.8 {
                high_confidence_count += 1;
            }
        }
    }
    Ok(KnowledgeSummary { patterns_count, conventions_count, high_confidence_count })
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SubprojectInfo {
    name: String,
    role: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RecipeMeta {
    name: String,
    description: String,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SkillMeta {
    name: String,
    description: String,
    source: String,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RecentEvent {
    pub event_type: String,
    pub ts: Option<String>,
    pub summary: Option<String>,
    pub spec: Option<String>,
    pub wave: Option<i64>,
    pub actor_kind: Option<String>,
    pub actor_id: Option<String>,
    pub tool_name: Option<String>,
    pub target: Option<String>,
    /// Canonical pipeline phase from payload.phase, when present.
    pub phase: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ActivityGroup {
    pub spec: Option<String>,
    pub wave: Option<i64>,
    pub action_kind: Option<String>,
    pub count: i64,
    pub min_ts: Option<String>,
    pub max_ts: Option<String>,
    pub tokens_total: i64,
    pub files_touched: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RoleQuality {
    pub role: String,
    pub pass_at_1: f64,
    pub fix_loops: i64,
    pub samples: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SlowestWave {
    pub spec: Option<String>,
    pub wave: Option<i64>,
    pub duration_ms: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PhaseTokens {
    pub phase: String,
    pub input_avg: f64,
    pub output_avg: f64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct QualityMetrics {
    pub pass_at_1: f64,
    pub fix_loop_rate: f64,
    pub avg_phase_duration_ms: f64,
    pub by_role: Vec<RoleQuality>,
    pub slowest_waves: Vec<SlowestWave>,
    pub tokens_by_phase: Vec<PhaseTokens>,
}

#[tauri::command]
fn dashboard_subprojects(repo_path: String) -> Result<Vec<SubprojectInfo>, String> {
    let base = PathBuf::from(&repo_path);
    let output = std::process::Command::new("node")
        .arg(".claude/scripts/sync-detect.js")
        .current_dir(&base)
        .output()
        .map_err(|e| format!("sync-detect failed: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("sync-detect failed: {}", stderr));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).map_err(|e| e.to_string())?;
    let subprojects = match v["subprojects"].as_array() {
        Some(a) => a,
        None => return Ok(vec![]),
    };
    let agents: Vec<String> = v["detectedAgents"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let mut results = Vec::new();
    for sp in subprojects {
        let name = match sp.as_str() {
            Some(s) => s.to_string(),
            None => match sp["name"].as_str() {
                Some(s) => s.to_string(),
                None => continue,
            },
        };
        let role = agents.iter().find(|a| a.starts_with(&name)).and_then(|a| {
            a.split('-').next().map(|s| s.to_string())
        });
        results.push(SubprojectInfo { name, role });
    }
    Ok(results)
}

#[tauri::command]
fn dashboard_recipes(repo_path: String) -> Result<Vec<RecipeMeta>, String> {
    let base = PathBuf::from(&repo_path);
    let dir = base.join(".claude").join("recipes");
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut results = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => { eprintln!("recipes: failed to read {:?}", path); continue; }
        };
        let v: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => { eprintln!("recipes: malformed JSON {:?}", path); continue; }
        };
        let name = v["name"].as_str().unwrap_or(&filename).to_string();
        let description = v["description"].as_str().unwrap_or("").to_string();
        results.push(RecipeMeta { name, description });
    }
    Ok(results)
}

fn parse_skill_frontmatter(content: &str) -> (String, String) {
    let mut name = String::new();
    let mut description = String::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 1usize;
    while i < lines.len() && lines[i] != "---" {
        let line = lines[i];
        if let Some(pos) = line.find(':') {
            let key = line[..pos].trim();
            let val = line[pos + 1..].trim().trim_matches(|c| c == '\'' || c == '"');
            if key == "name" {
                name = val.to_string();
            } else if key == "description" {
                if val == "|" || val == ">" {
                    let mut j = i + 1;
                    while j < lines.len() && lines[j] != "---" {
                        let cont = lines[j].trim();
                        if !cont.is_empty() {
                            description = cont.to_string();
                            break;
                        }
                        j += 1;
                    }
                } else {
                    description = val.to_string();
                }
            }
        }
        i += 1;
    }
    (name, description)
}

#[tauri::command]
fn dashboard_skills(repo_path: String) -> Result<Vec<SkillMeta>, String> {
    let base = PathBuf::from(&repo_path);
    let roots = [
        (base.join(".claude").join("skills"), "foundation"),
        (base.join(".claude").join("commands").join("mustard"), "command"),
    ];
    let mut results = Vec::new();
    for (root, source) in &roots {
        if !root.exists() {
            continue;
        }
        let entries = match std::fs::read_dir(root) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries {
            let entry = match entry { Ok(e) => e, Err(_) => continue };
            let skill_path = entry.path().join("SKILL.md");
            if !skill_path.exists() { continue; }
            let content = match std::fs::read_to_string(&skill_path) { Ok(c) => c, Err(_) => continue };
            if !content.starts_with("---\n") { continue; }
            let (mut skill_name, description) = parse_skill_frontmatter(&content);
            if skill_name.is_empty() { skill_name = entry.file_name().to_string_lossy().to_string(); }
            results.push(SkillMeta { name: skill_name, description, source: source.to_string() });
        }
    }

    // Walk subproject skills via sync-detect
    let detect = std::process::Command::new("node")
        .arg(".claude/scripts/sync-detect.js")
        .current_dir(&base)
        .output();
    if let Ok(output) = detect {
        if output.status.success() {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                if let Some(arr) = v["subprojects"].as_array() {
                    for sp in arr {
                        let name = match sp.as_str() {
                            Some(s) => s.to_string(),
                            None => match sp["name"].as_str() {
                                Some(s) => s.to_string(),
                                None => continue,
                            },
                        };
                        let sub_root = base.join(&name).join(".claude").join("skills");
                        if !sub_root.exists() { continue; }
                        let entries = match std::fs::read_dir(&sub_root) { Ok(e) => e, Err(_) => continue };
                        for entry in entries {
                            let entry = match entry { Ok(e) => e, Err(_) => continue };
                            let skill_path = entry.path().join("SKILL.md");
                            if !skill_path.exists() { continue; }
                            let content = match std::fs::read_to_string(&skill_path) { Ok(c) => c, Err(_) => continue };
                            if !content.starts_with("---\n") { continue; }
                            let (mut skill_name, description) = parse_skill_frontmatter(&content);
                            if skill_name.is_empty() {
                                skill_name = entry.file_name().to_string_lossy().to_string();
                            }
                            results.push(SkillMeta { name: skill_name, description, source: format!("subproject:{}", name) });
                        }
                    }
                }
            }
        } else {
            eprintln!("dashboard_skills: sync-detect failed: {}", String::from_utf8_lossy(&output.stderr));
        }
    }

    Ok(results)
}

fn extract_from_value(v: &serde_json::Value, event_type: &str) -> (Option<String>, Option<String>) {
    let payload_obj: Option<&serde_json::Value> = if v.get("payload").and_then(|p| p.as_object()).is_some() {
        v.get("payload")
    } else {
        None
    };
    let payload_str_parsed: Option<serde_json::Value> = v.get("payload")
        .and_then(|p| p.as_str())
        .and_then(|s| serde_json::from_str(s).ok());
    let p: Option<&serde_json::Value> = payload_obj.or_else(|| payload_str_parsed.as_ref());

    let tool_name = p
        .and_then(|x| x.get("tool").and_then(|t| t.as_str()).or_else(|| x.get("tool_name").and_then(|t| t.as_str())))
        .map(|s| s.to_string());

    let target = if event_type == "agent.start" {
        p.and_then(|x| x.get("agent_type").and_then(|a| a.as_str()).or_else(|| x.get("agentType").and_then(|a| a.as_str())))
            .map(|s| s.to_string())
    } else if event_type == "pipeline.phase" {
        p.and_then(|x| x.get("phase")).and_then(|x| x.as_str()).map(|s| s.to_string())
    } else {
        let modern = p.and_then(|x| x.get("target")).and_then(|t| {
            t.get("file").and_then(|x| x.as_str())
                .or_else(|| t.get("command").and_then(|x| x.as_str()))
                .or_else(|| t.get("pattern").and_then(|x| x.as_str()))
                .or_else(|| t.get("url").and_then(|x| x.as_str()))
                .or_else(|| t.get("path").and_then(|x| x.as_str()))
        });
        let target_str = if modern.is_none() {
            p.and_then(|x| x.get("target")).and_then(|x| x.as_str())
        } else { modern };
        let legacy = p.and_then(|x| x.get("tool_input")).and_then(|ti| {
            ti.get("file_path").and_then(|x| x.as_str())
                .or_else(|| ti.get("command").and_then(|x| x.as_str()))
                .or_else(|| ti.get("pattern").and_then(|x| x.as_str()))
                .or_else(|| ti.get("url").and_then(|x| x.as_str()))
        });
        target_str.or(legacy).map(|s| s.to_string())
    };
    (tool_name, target)
}

#[tauri::command]
fn dashboard_recent_events(repo_path: String, limit: Option<usize>) -> Result<Vec<RecentEvent>, String> {
    let base = PathBuf::from(&repo_path);
    let cap = limit.unwrap_or(20);
    // Wave 4 made events.jsonl the canonical store; the SQLite mirror stopped
    // receiving writes around 2026-05-12 in the user's sialia project. Try
    // JSONL first so the dashboard surfaces today's events. Fall back to the
    // SQLite mirror only when the JSONL is missing/empty (fresh projects).
    let jsonl = recent_events_from_jsonl(&base, cap)?;
    if !jsonl.is_empty() {
        return Ok(jsonl);
    }
    if let Some(r) = db::with_db(&base, |c| db::recent_events_from_db(c, cap)) {
        return r;
    }
    Ok(vec![])
}

/// Extract a useful summary string from an events.jsonl entry. The hooks
/// emit structured payloads (no top-level `summary`), so this mirrors
/// `db::summary_from_payload` for the JSONL path. The two readers stayed
/// duplicated by accident — both must agree or the dashboard sees nulls.
///
/// Event-specific extraction:
///   - `qa.result`: payload.overall (pass|fail|skip), with failed-AC count
///     when present in payload.criteria.
///   - `agent.start`: payload.description (truncated).
///   - `agent.stop`: payload.summary (truncated).
///   - default: walks the generic summary/description/msg/text keys.
fn summary_from_jsonl_value(v: &serde_json::Value, event_type: &str) -> Option<String> {
    let payload = v.get("payload");
    if event_type == "qa.result" {
        if let Some(p) = payload {
            if let Some(overall) = p.get("overall").and_then(|x| x.as_str()) {
                let fail_count = p
                    .get("criteria")
                    .and_then(|c| c.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter(|c| c.get("result").and_then(|r| r.as_str()) == Some("fail"))
                            .count()
                    })
                    .unwrap_or(0);
                return Some(if fail_count > 0 {
                    format!("overall={} ({} failed)", overall, fail_count)
                } else {
                    format!("overall={}", overall)
                });
            }
        }
    }
    if event_type == "agent.start" {
        if let Some(d) = payload.and_then(|p| p.get("description")).and_then(|x| x.as_str()) {
            return Some(d.chars().take(120).collect());
        }
    }
    if event_type == "agent.stop" {
        if let Some(s) = payload.and_then(|p| p.get("summary")).and_then(|x| x.as_str()) {
            return Some(s.chars().take(120).collect());
        }
    }
    // Fallback: generic keys at the top level or inside payload.
    let from_top = v
        .get("summary")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("description").and_then(|x| x.as_str()))
        .map(|s| s.to_string());
    if from_top.is_some() {
        return from_top;
    }
    if let Some(p) = payload {
        for key in &["summary", "description", "msg", "text"] {
            if let Some(s) = p.get(*key).and_then(|x| x.as_str()) {
                return Some(s.chars().take(120).collect());
            }
        }
    }
    None
}

/// Read the last `cap` events from `<repo>/.claude/.harness/events.jsonl` and
/// return them with the freshest first.
fn recent_events_from_jsonl(base: &std::path::Path, cap: usize) -> Result<Vec<RecentEvent>, String> {
    let path = base.join(".claude").join(".harness").join("events.jsonl");
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let valid: Vec<serde_json::Value> = content
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .collect();
    let slice: &[serde_json::Value] = if valid.len() > cap {
        &valid[valid.len() - cap..]
    } else {
        &valid[..]
    };
    // Emit freshest-first (matches the SQLite-backed reader semantics).
    let mut results: Vec<RecentEvent> = slice
        .iter()
        .rev()
        .map(|v| {
            let event_type = v["event"]
                .as_str()
                .or_else(|| v["type"].as_str())
                .unwrap_or("unknown")
                .to_string();
            let (tool_name, target) = extract_from_value(v, &event_type);
            let actor_kind = v
                .get("actor")
                .and_then(|a| a.get("kind"))
                .and_then(|x| x.as_str())
                .or_else(|| v["actor_kind"].as_str())
                .map(String::from);
            let actor_id = v
                .get("actor")
                .and_then(|a| a.get("id"))
                .and_then(|x| x.as_str())
                .or_else(|| v["actor_id"].as_str())
                .map(String::from);
            let phase = v
                .get("payload")
                .and_then(|p| p.get("phase"))
                .and_then(|x| x.as_str())
                .map(|s| s.to_ascii_uppercase());
            RecentEvent {
                event_type: event_type.clone(),
                ts: v["ts"]
                    .as_str()
                    .or_else(|| v["timestamp"].as_str())
                    .map(|s| s.to_string()),
                summary: summary_from_jsonl_value(v, &event_type),
                spec: v["spec"].as_str().map(String::from),
                wave: v["wave"].as_i64(),
                actor_kind,
                actor_id,
                tool_name,
                target,
                phase,
            }
        })
        .collect();
    // Defensive: if some events lack `ts`, the file order is still our best
    // guess at recency, so we already reversed the slice. No-op when ts present.
    results.sort_by(|a, b| b.ts.cmp(&a.ts));
    Ok(results)
}

#[tauri::command]
fn dashboard_specs(repo_path: String) -> Result<Vec<SpecRow>, String> {
    let base = PathBuf::from(&repo_path);
    let spec_root = base.join(".claude").join("spec");

    // The filesystem is the source of truth — DB rows may be stale (specs
    // renamed/moved/migrated away). Walk FS first; this also walks wave-plan
    // children and emits them with parent set.
    let fs_rows = specs_from_fs(&base);

    // Build a presence map keyed by spec name (across all buckets), so we can
    // filter DB rows that no longer have a backing directory.
    let mut by_name: HashMap<String, SpecRow> = HashMap::new();
    for row in fs_rows {
        by_name.insert(row.name.clone(), row);
    }

    // Pull DB rows but ONLY merge them into specs that exist on disk. Specs
    // present in DB but missing from FS are historical (migrated, renamed) —
    // we deliberately drop them rather than show them as ghosts.
    if let Some(Ok(db_rows)) = db::with_db(&base, db::specs_from_db) {
        for mut row in db_rows {
            let bucket = detect_spec_existence(&spec_root, &row.name);
            if bucket.is_none() {
                // Historical: do not surface in the dashboard.
                continue;
            }
            row.bucket = bucket;
            // Enrich FS row with DB-only fields (timestamps, affected_files)
            // when both sides have the spec. Preserve parent from FS row.
            if let Some(existing) = by_name.get_mut(&row.name) {
                if existing.started_at.is_none() {
                    existing.started_at = row.started_at;
                }
                if existing.completed_at.is_none() {
                    existing.completed_at = row.completed_at;
                }
                if existing.affected_files.is_empty() {
                    existing.affected_files = row.affected_files;
                }
            } else {
                // DB row backed by FS but FS walk missed it — extremely rare.
                by_name.insert(row.name.clone(), row);
            }
        }
    }

    let mut rows: Vec<SpecRow> = by_name.into_values().collect();
    // Stable order: children right after their parent, then standalone.
    rows.sort_by(|a, b| {
        let ka = a.parent.as_deref().unwrap_or(&a.name);
        let kb = b.parent.as_deref().unwrap_or(&b.name);
        ka.cmp(kb)
            .then_with(|| a.parent.is_some().cmp(&b.parent.is_some()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(rows)
}

#[allow(dead_code)] // kept for legacy callers that may still rely on it
fn detect_bucket(spec_root: &PathBuf, spec_name: &str) -> Option<String> {
    for sub in ["active", "completed", "cancelled"] {
        if spec_root.join(sub).join(spec_name).is_dir() {
            return Some(sub.to_string());
        }
    }
    None
}

// Filesystem fallback for repos without the phase-1 DB schema.
// Walks .claude/.pipeline-states/*.json for phase/status, then iterates
// .claude/spec/{active,completed}/*/spec.md, parsing inline status/phase.
fn specs_from_fs(base: &PathBuf) -> Vec<SpecRow> {
    let claude = base.join(".claude");
    let mut state_map: HashMap<String, (Option<String>, Option<String>)> = HashMap::new();
    let states_dir = claude.join(".pipeline-states");
    if states_dir.exists() {
        if let Ok(rd) = std::fs::read_dir(&states_dir) {
            for entry in rd.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                let content = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let v: serde_json::Value = match serde_json::from_str(&content) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let name = match v["specName"].as_str() {
                    Some(s) if !s.is_empty() => s.to_string(),
                    _ => continue,
                };
                let phase = v["phaseName"].as_str().map(|s| s.to_string());
                let status = v["status"].as_str().map(|s| s.to_string());
                state_map.insert(name, (phase, status));
            }
        }
    }

    let spec_root = claude.join("spec");
    let mut completed: Vec<(SpecRow, Option<std::time::SystemTime>)> = Vec::new();
    let mut active: Vec<(SpecRow, Option<std::time::SystemTime>)> = Vec::new();
    let mut cancelled: Vec<(SpecRow, Option<std::time::SystemTime>)> = Vec::new();
    for sub in ["active", "completed", "cancelled"] {
        let dir = spec_root.join(sub);
        if !dir.exists() {
            continue;
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = match path.file_name().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let (phase, status) = match state_map.get(&name) {
                Some((p, s)) => (p.clone(), s.clone()),
                None => {
                    let from_spec = parse_spec_md(&path.join("spec.md"));
                    if from_spec.0.is_some() || from_spec.1.is_some() {
                        from_spec
                    } else {
                        // Wave-plan parents have no root spec.md — try wave-plan.md.
                        parse_spec_md(&path.join("wave-plan.md"))
                    }
                }
            };
            let mtime = entry.metadata().ok().and_then(|m| m.modified().ok());
            let parent_row = SpecRow {
                name: name.clone(),
                status,
                phase,
                started_at: None,
                completed_at: None,
                affected_files: vec![],
                bucket: Some(sub.to_string()),
                parent: None,
            };
            let bucket = sub.to_string();
            let target = match sub {
                "completed" => &mut completed,
                "cancelled" => &mut cancelled,
                _ => &mut active,
            };
            target.push((parent_row, mtime));

            // Wave plan: walk wave-N-*/spec.md children. Each child becomes a
            // SpecRow with parent set to the wave plan's name. The dashboard
            // groups them visually.
            let wave_plan = path.join("wave-plan.md");
            if wave_plan.exists() {
                if let Ok(child_rd) = std::fs::read_dir(&path) {
                    for child in child_rd.flatten() {
                        let cpath = child.path();
                        if !cpath.is_dir() {
                            continue;
                        }
                        let cname = match cpath.file_name().and_then(|s| s.to_str()) {
                            Some(s) => s.to_string(),
                            None => continue,
                        };
                        // Only walk dirs that look like wave-N-something
                        if !cname.starts_with("wave-") {
                            continue;
                        }
                        let cspec = cpath.join("spec.md");
                        if !cspec.exists() {
                            continue;
                        }
                        let (cphase, cstatus) = parse_spec_md(&cspec);
                        let cmtime = child.metadata().ok().and_then(|m| m.modified().ok());
                        let child_row = SpecRow {
                            name: cname,
                            status: cstatus,
                            phase: cphase,
                            started_at: None,
                            completed_at: None,
                            affected_files: vec![],
                            bucket: Some(bucket.clone()),
                            parent: Some(name.clone()),
                        };
                        target.push((child_row, cmtime));
                    }
                }
            }
        }
    }
    let sort_recent = |a: &(SpecRow, Option<std::time::SystemTime>), b: &(SpecRow, Option<std::time::SystemTime>)| match (a.1, b.1) {
        (Some(x), Some(y)) => y.cmp(&x),
        _ => b.0.name.cmp(&a.0.name),
    };
    active.sort_by(sort_recent);
    completed.sort_by(sort_recent);
    cancelled.sort_by(sort_recent);
    let mut out: Vec<SpecRow> = Vec::with_capacity(active.len() + completed.len() + cancelled.len());
    out.extend(active.into_iter().map(|(r, _)| r));
    out.extend(completed.into_iter().map(|(r, _)| r));
    out.extend(cancelled.into_iter().map(|(r, _)| r));
    out
}

/// Check whether a spec directory still exists on disk under
/// .claude/spec/{bucket}/{name}/ or as a wave child under any parent.
/// Returns Some(bucket) when found, None when historical/missing.
fn detect_spec_existence(spec_root: &PathBuf, name: &str) -> Option<String> {
    for sub in ["active", "completed", "cancelled"] {
        if spec_root.join(sub).join(name).is_dir() {
            return Some(sub.to_string());
        }
    }
    // Could also be a wave child — scan one level deeper for `wave-{n}-...` dirs.
    for sub in ["active", "completed", "cancelled"] {
        let dir = spec_root.join(sub);
        if !dir.exists() {
            continue;
        }
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.is_dir() && p.join(name).is_dir() {
                    return Some(sub.to_string());
                }
            }
        }
    }
    None
}

// Returns (phase, status) parsed from a spec.md or wave-plan.md.
// Supports three layouts:
//   YAML frontmatter `---\nphase: BACKLOG\nstatus: roadmap\n---`
//   `### Status: closed | Phase: CLOSE`
//   `- **Status**: closed` / `- **Phase**: CLOSE` (legacy bullet form)
fn parse_spec_md(path: &PathBuf) -> (Option<String>, Option<String>) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return (None, None),
    };
    let mut phase: Option<String> = None;
    let mut status: Option<String> = None;

    // YAML frontmatter — only when the file opens with `---` (allowing a BOM).
    let stripped = content.strip_prefix('\u{FEFF}').unwrap_or(&content);
    if let Some(after_open) = stripped.strip_prefix("---\n").or_else(|| stripped.strip_prefix("---\r\n")) {
        for line in after_open.lines() {
            let trimmed = line.trim();
            if trimmed == "---" {
                break;
            }
            if let Some(v) = strip_yaml_label(trimmed, "phase") {
                if phase.is_none() {
                    phase = Some(v);
                }
            } else if let Some(v) = strip_yaml_label(trimmed, "status") {
                if status.is_none() {
                    status = Some(v);
                }
            }
            if phase.is_some() && status.is_some() {
                break;
            }
        }
    }

    if phase.is_some() && status.is_some() {
        return (phase, status);
    }

    for raw in content.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("###") {
            let rest = rest.trim();
            // Pattern: "Status: X | Phase: Y" (either order, either present)
            for part in rest.split('|') {
                let part = part.trim();
                if let Some(v) = strip_label(part, "Status") {
                    if status.is_none() {
                        status = Some(v);
                    }
                } else if let Some(v) = strip_label(part, "Phase") {
                    if phase.is_none() {
                        phase = Some(v);
                    }
                }
            }
        } else if let Some(rest) = line.strip_prefix('-') {
            let rest = rest.trim();
            if let Some(v) = strip_bold_label(rest, "Status") {
                if status.is_none() {
                    status = Some(v);
                }
            } else if let Some(v) = strip_bold_label(rest, "Phase") {
                if phase.is_none() {
                    phase = Some(v);
                }
            }
        }
        if phase.is_some() && status.is_some() {
            break;
        }
    }
    (phase, status)
}

// Matches `key: value` (YAML scalar) → Some(value); strips quotes.
fn strip_yaml_label(s: &str, label: &str) -> Option<String> {
    let mut parts = s.splitn(2, ':');
    let key = parts.next()?.trim();
    if !key.eq_ignore_ascii_case(label) {
        return None;
    }
    let val = parts.next()?.trim();
    // Drop trailing inline comment ("phase: BACKLOG # note")
    let val = val.split('#').next()?.trim();
    let val = val.trim_matches(|c| c == '"' || c == '\'');
    if val.is_empty() {
        return None;
    }
    Some(val.split_whitespace().next()?.to_string())
}

// Matches "Label: value" → Some(value); else None.
fn strip_label(s: &str, label: &str) -> Option<String> {
    let mut parts = s.splitn(2, ':');
    let key = parts.next()?.trim();
    if !key.eq_ignore_ascii_case(label) {
        return None;
    }
    let val = parts.next()?.trim();
    if val.is_empty() {
        None
    } else {
        Some(val.split_whitespace().next()?.to_string())
    }
}

// Matches "**Label**: value" → Some(value); else None.
fn strip_bold_label(s: &str, label: &str) -> Option<String> {
    let bold = format!("**{}**", label);
    let rest = s.strip_prefix(&bold)?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(':')?.trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.split_whitespace().next()?.to_string())
    }
}

#[tauri::command]
fn dashboard_spec_markdown(repo_path: String, spec_name: String) -> Result<String, String> {
    let base = PathBuf::from(&repo_path).join(".claude").join("spec");
    // Reject traversal — spec_name is a single directory name, not a path.
    if spec_name.is_empty()
        || spec_name.contains('/')
        || spec_name.contains('\\')
        || spec_name.contains("..")
    {
        return Err(format!("invalid spec name: {}", spec_name));
    }
    // 1. Standalone spec: .claude/spec/{bucket}/{spec_name}/spec.md
    for sub in ["active", "completed", "cancelled"] {
        let path = base.join(sub).join(&spec_name).join("spec.md");
        if path.exists() {
            return std::fs::read_to_string(&path).map_err(|e| e.to_string());
        }
    }
    // 2. Wave-plan child: dashboard_specs emits children with a bare name
    //    (e.g. "wave-2-frontend") and parent set, but the file actually lives
    //    one level down at .claude/spec/{bucket}/{parent}/{spec_name}/spec.md.
    //    Without the parent, search every wave-plan dir for a matching child.
    for sub in ["active", "completed", "cancelled"] {
        let bucket = base.join(sub);
        let Ok(rd) = std::fs::read_dir(&bucket) else {
            continue;
        };
        for entry in rd.flatten() {
            let parent_dir = entry.path();
            if !parent_dir.is_dir() {
                continue;
            }
            let child = parent_dir.join(&spec_name).join("spec.md");
            if child.exists() {
                return std::fs::read_to_string(&child).map_err(|e| e.to_string());
            }
        }
    }
    Err(format!("spec markdown not found: {}", spec_name))
}

fn move_spec_dir(repo_path: &str, spec_name: &str, target: &str) -> Result<String, String> {
    if !matches!(target, "active" | "completed" | "cancelled") {
        return Err(format!("invalid target bucket: {}", target));
    }
    if spec_name.is_empty()
        || spec_name.contains('/')
        || spec_name.contains('\\')
        || spec_name.contains("..")
    {
        return Err(format!("invalid spec name: {}", spec_name));
    }
    let spec_root = PathBuf::from(repo_path).join(".claude").join("spec");
    let dest_dir = spec_root.join(target).join(spec_name);
    if dest_dir.is_dir() {
        return Ok(target.to_string());
    }
    let mut source: Option<PathBuf> = None;
    for sub in ["active", "completed", "cancelled"] {
        if sub == target {
            continue;
        }
        let candidate = spec_root.join(sub).join(spec_name);
        if candidate.is_dir() {
            source = Some(candidate);
            break;
        }
    }
    let from = source.ok_or_else(|| format!("spec not found: {}", spec_name))?;
    if let Some(parent) = dest_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&from, &dest_dir).map_err(|e| {
        format!("failed to move {} → {}: {}", from.display(), dest_dir.display(), e)
    })?;
    Ok(target.to_string())
}

#[tauri::command]
fn dashboard_spec_complete(repo_path: String, spec_name: String) -> Result<String, String> {
    move_spec_dir(&repo_path, &spec_name, "completed")
}

#[tauri::command]
fn dashboard_spec_cancel(repo_path: String, spec_name: String) -> Result<String, String> {
    move_spec_dir(&repo_path, &spec_name, "cancelled")
}

#[tauri::command]
fn dashboard_spec_reactivate(repo_path: String, spec_name: String) -> Result<String, String> {
    move_spec_dir(&repo_path, &spec_name, "active")
}

#[tauri::command]
fn dashboard_search_events(repo_path: String, query: String, limit: Option<usize>) -> Result<Vec<RecentEvent>, String> {
    let base = PathBuf::from(&repo_path);
    let cap = limit.unwrap_or(50);
    match db::with_db(&base, |c| db::search_events_from_db(c, &query, cap)) {
        Some(r) => r,
        None => Ok(vec![]),
    }
}

#[tauri::command]
fn dashboard_search_knowledge(repo_path: String, query: String, limit: Option<usize>) -> Result<Vec<KnowledgeRow>, String> {
    let base = PathBuf::from(&repo_path);
    let cap = limit.unwrap_or(50);
    match db::with_db(&base, |c| db::search_knowledge_from_db(c, &query, cap)) {
        Some(r) => r,
        None => Ok(vec![]),
    }
}

#[tauri::command]
fn dashboard_activity_aggregated(repo_path: String, limit: Option<usize>) -> Result<Vec<ActivityGroup>, String> {
    let lim = limit.unwrap_or(200);
    let base = PathBuf::from(&repo_path);
    match db::with_db(&base, |conn| db::aggregate_activity_from_db(conn, lim)) {
        Some(r) => r,
        None => Ok(vec![]),
    }
}

#[tauri::command]
fn dashboard_quality_metrics(repo_path: String) -> Result<QualityMetrics, String> {
    let base = PathBuf::from(&repo_path);
    match db::with_db(&base, |conn| db::quality_metrics_from_db(conn)) {
        Some(r) => r,
        None => Ok(QualityMetrics::default()),
    }
}

#[tauri::command]
fn dashboard_knowledge_browse(repo_path: String, limit: Option<usize>) -> Result<Vec<KnowledgeRow>, String> {
    let lim = limit.unwrap_or(500);
    let base = PathBuf::from(&repo_path);
    match db::with_db(&base, |conn| db::knowledge_browse_from_db(conn, lim)) {
        Some(r) => r,
        None => Ok(vec![]),
    }
}

#[tauri::command]
fn dashboard_telemetry(repo_path: String) -> Result<telemetry::TelemetrySummary, String> {
    let base = std::path::PathBuf::from(&repo_path);
    Ok(telemetry::TelemetrySummary {
        rtk: telemetry::rtk_summary(&base),
        measured: telemetry::measured(&base),
        prevention: telemetry::hook_fire_counts(&base),
        routing: telemetry::routing_breakdown(&base),
        workflow: telemetry::workflow_by_phase(&base),
        tool_breakdown: telemetry::tool_breakdown(&base),
        agent_activity: telemetry::agent_activity_from_jsonl(&base),
    })
}

/// Live activity derived from events.jsonl tail. Refreshed on every PreToolUse
/// by metrics-tracker, so this reflects the current session in real time.
#[tauri::command]
fn dashboard_live_activity(repo_path: String) -> Result<telemetry::LiveActivity, String> {
    let base = std::path::PathBuf::from(&repo_path);
    Ok(telemetry::live_activity(&base))
}

/// Per-workspace consumption + cost summary. Returns zeros when the spans
/// table is empty or the DB hasn't been initialised yet.
#[tauri::command]
fn dashboard_consumption(repo_path: String) -> Result<ConsumptionSummary, String> {
    let base = std::path::PathBuf::from(&repo_path);
    match db::with_db(&base, db::consumption_summary_from_db) {
        Some(r) => r,
        None => Ok(ConsumptionSummary::default()),
    }
}

/// Cross-project (global) consumption: walks every project discovered under
/// `projects_root`, sums tokens and cost per project + per model, builds a
/// merged 14-day daily series, and attaches the global RTK block.
#[tauri::command]
fn dashboard_consumption_global(projects_root: String) -> Result<GlobalConsumption, String> {
    let root = std::path::PathBuf::from(&projects_root);
    let projects = discovery::discover(&root)?;

    let mut out = GlobalConsumption::default();
    let mut model_acc: std::collections::HashMap<String, ModelUsage> = std::collections::HashMap::new();
    let mut daily_acc: std::collections::HashMap<String, DailyPoint> = std::collections::HashMap::new();

    for p in projects {
        let project_path = std::path::PathBuf::from(&p.path);
        let mut row = ProjectUsage {
            id: p.id.clone(),
            name: p.name.clone(),
            path: p.path.clone(),
            tokens_total: 0,
            tokens_today: 0,
            cost_total_usd: 0.0,
            cost_today_usd: 0.0,
            last_activity_ms: p.last_activity_ms,
        };

        if let Some(Ok(summary)) = db::with_db(&project_path, db::consumption_summary_from_db) {
            row.tokens_total = summary.tokens_total;
            row.tokens_today = summary.tokens_today;
            row.cost_total_usd = summary.cost_total_usd;
            row.cost_today_usd = summary.cost_today_usd;

            out.tokens_total += summary.tokens_total;
            out.tokens_today += summary.tokens_today;
            out.cost_total_usd += summary.cost_total_usd;
            out.cost_today_usd += summary.cost_today_usd;

            for m in summary.by_model {
                let entry = model_acc.entry(m.model.clone()).or_insert_with(|| ModelUsage {
                    model: m.model.clone(),
                    ..Default::default()
                });
                entry.calls += m.calls;
                entry.input_tokens += m.input_tokens;
                entry.output_tokens += m.output_tokens;
                entry.total_tokens += m.total_tokens;
                entry.cost_usd += m.cost_usd;
            }

            for d in summary.daily_series {
                let entry = daily_acc.entry(d.date.clone()).or_insert_with(|| DailyPoint {
                    date: d.date.clone(),
                    ..Default::default()
                });
                entry.calls += d.calls;
                entry.input_tokens += d.input_tokens;
                entry.output_tokens += d.output_tokens;
                entry.total_tokens += d.total_tokens;
                entry.cost_usd += d.cost_usd;
            }
        }

        out.by_project.push(row);
    }

    // Finalize aggregates.
    let grand_total: u64 = model_acc.values().map(|m| m.total_tokens).sum();
    let mut models: Vec<ModelUsage> = model_acc.into_values().collect();
    if grand_total > 0 {
        for m in &mut models {
            m.pct_tokens = m.total_tokens as f64 / grand_total as f64;
        }
    }
    models.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
    out.by_model = models;

    let mut series: Vec<DailyPoint> = daily_acc.into_values().collect();
    series.sort_by(|a, b| a.date.cmp(&b.date));
    out.daily_series = series;

    out.by_project.sort_by(|a, b| {
        b.cost_total_usd
            .partial_cmp(&a.cost_total_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    out.rtk = telemetry::rtk_summary_global();
    Ok(out)
}

#[tauri::command]
fn discover_projects(root: String) -> Result<Vec<discovery::Project>, String> {
    discovery::discover(std::path::Path::new(&root))
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ActivePipeline {
    spec_name: String,
    status: String,
    phase: String,
    current_wave: Option<u32>,
    total_waves: Option<u32>,
    model: Option<String>,
    has_dispatch_failure: bool,
    failure_age_ms: Option<u64>,
    tasks_pending: usize,
    tasks_in_progress: usize,
    tasks_completed: usize,
    updated_at: Option<String>,
}

/// Parse ISO 8601 / RFC 3339 timestamp string into seconds since UNIX_EPOCH.
/// Handles: `YYYY-MM-DDTHH:MM:SS(.fff)?Z` and `+00:00` offset.
fn parse_iso_to_unix_secs(s: &str) -> Option<u64> {
    // Trim trailing Z or +00:00 / -00:00
    let s = s.trim();
    let s = if s.ends_with('Z') { &s[..s.len() - 1] } else { s };
    let s = if let Some(pos) = s.rfind('+') {
        if pos > 10 { &s[..pos] } else { s }
    } else if let Some(pos) = s[10..].rfind('-') {
        &s[..10 + pos]
    } else {
        s
    };
    // Expected: YYYY-MM-DDTHH:MM:SS(.sss)?
    let (date_part, time_part) = s.split_once('T')?;
    let mut date_parts = date_part.splitn(3, '-');
    let year: u64 = date_parts.next()?.parse().ok()?;
    let month: u64 = date_parts.next()?.parse().ok()?;
    let day: u64 = date_parts.next()?.parse().ok()?;
    let time_no_frac = time_part.split('.').next()?;
    let mut time_parts = time_no_frac.splitn(3, ':');
    let hour: u64 = time_parts.next()?.parse().ok()?;
    let minute: u64 = time_parts.next()?.parse().ok()?;
    let second: u64 = time_parts.next()?.parse().ok()?;
    // Days since epoch using a simplified calculation (ignores leap seconds)
    let days = days_since_epoch(year, month, day)?;
    Some(days * 86400 + hour * 3600 + minute * 60 + second)
}

fn days_since_epoch(year: u64, month: u64, day: u64) -> Option<u64> {
    if year < 1970 { return None; }
    let mut total: u64 = 0;
    for y in 1970..year {
        total += if is_leap(y) { 366 } else { 365 };
    }
    let days_in_month = [31u64, if is_leap(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for m in 1..month {
        total += days_in_month.get((m - 1) as usize)?;
    }
    total += day - 1;
    Some(total)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn epoch_to_iso(secs: u64) -> String {
    // Minimal ISO formatter: seconds since epoch → YYYY-MM-DDTHH:MM:SSZ
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let mut days = secs / 86400;
    let mut year = 1970u64;
    loop {
        let dy = if is_leap(year) { 366 } else { 365 };
        if days < dy { break; }
        days -= dy;
        year += 1;
    }
    let days_in_month = [31u64, if is_leap(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1u64;
    for dm in &days_in_month {
        if days < *dm { break; }
        days -= dm;
        month += 1;
    }
    let day = days + 1;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", year, month, day, h, m, s)
}

fn mtime_to_iso(path: &std::path::Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    let secs = mtime.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(epoch_to_iso(secs))
}

#[tauri::command]
fn dashboard_watch_repos(
    repo_paths: Vec<String>,
    state: tauri::State<Arc<Mutex<watcher::WatcherState>>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    for path in repo_paths {
        if let Err(e) = watcher::ensure_watching(state.inner().clone(), path.clone(), app.clone()) {
            eprintln!("dashboard_watch_repos: failed for {}: {}", path, e);
        }
    }
    Ok(())
}

#[tauri::command]
fn dashboard_active_pipelines(repo_path: String) -> Result<Vec<ActivePipeline>, String> {
    let base = std::path::PathBuf::from(&repo_path);
    let dir = base.join(".claude").join(".pipeline-states");
    if !dir.exists() {
        return Ok(vec![]);
    }
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut results = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) => return Err(e.to_string()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Skip non-json and .metrics.json files
        let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if fname.ends_with(".metrics.json") { continue; }
        if path.extension().and_then(|s| s.to_str()) != Some("json") { continue; }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let v: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let status = v["status"].as_str().unwrap_or("unknown").to_string();
        // Filter completed/closed
        if status == "completed" || status == "closed" { continue; }

        let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        let spec_name = v["specName"].as_str()
            .or_else(|| v["name"].as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or(file_stem);

        let phase = v["phaseName"].as_str()
            .or_else(|| v["phase"].as_str())
            .unwrap_or("UNKNOWN")
            .to_string();

        let current_wave = v["currentWave"].as_u64().map(|n| n as u32);
        let total_waves = v["totalWaves"].as_u64().map(|n| n as u32);
        let model = v["model"].as_str().map(|s| s.to_string());

        // Dispatch failure
        let (has_dispatch_failure, failure_age_ms) = match v.get("lastDispatchFailure") {
            Some(serde_json::Value::Object(obj)) if !obj.is_empty() => {
                let age = obj.get("at")
                    .and_then(|a| a.as_str())
                    .and_then(|s| parse_iso_to_unix_secs(s))
                    .map(|at_secs| {
                        let elapsed = now_secs.saturating_sub(at_secs);
                        elapsed * 1000
                    });
                (true, age)
            }
            _ => (false, None),
        };

        // Tasks
        let mut tasks_pending = 0usize;
        let mut tasks_in_progress = 0usize;
        let mut tasks_completed = 0usize;
        if let Some(arr) = v["tasks"].as_array() {
            for task in arr {
                match task["status"].as_str() {
                    Some("pending") => tasks_pending += 1,
                    Some("in_progress") => tasks_in_progress += 1,
                    Some("completed") => tasks_completed += 1,
                    _ => {}
                }
            }
        }

        // updated_at: checkpointedAt → updatedAt → file mtime
        let updated_at = v["checkpointedAt"].as_str()
            .or_else(|| v["updatedAt"].as_str())
            .map(|s| s.to_string())
            .or_else(|| mtime_to_iso(&path));

        results.push(ActivePipeline {
            spec_name,
            status,
            phase,
            current_wave,
            total_waves,
            model,
            has_dispatch_failure,
            failure_age_ms,
            tasks_pending,
            tasks_in_progress,
            tasks_completed,
            updated_at,
        });
    }

    // Sort descending by updated_at
    results.sort_by(|a, b| {
        let ta = a.updated_at.as_deref().unwrap_or("");
        let tb = b.updated_at.as_deref().unwrap_or("");
        tb.cmp(ta)
    });

    Ok(results)
}

#[tauri::command]
fn dashboard_read_env(repo_path: String) -> Result<HashMap<String, String>, String> {
    let settings_path = PathBuf::from(&repo_path).join(".claude").join("settings.json");
    if !settings_path.exists() {
        return Ok(HashMap::new());
    }
    let content = std::fs::read_to_string(&settings_path).map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    let env_obj = match v.get("env").and_then(|e| e.as_object()) {
        Some(obj) => obj,
        None => return Ok(HashMap::new()),
    };
    let mut map = HashMap::new();
    for (k, val) in env_obj {
        map.insert(k.clone(), val.as_str().unwrap_or("").to_string());
    }
    Ok(map)
}

#[tauri::command]
fn dashboard_write_env(repo_path: String, env: HashMap<String, String>) -> Result<(), String> {
    let settings_path = PathBuf::from(&repo_path).join(".claude").join("settings.json");
    let mut value: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).map_err(|e| e.to_string())?
    } else {
        serde_json::json!({})
    };
    value.as_object_mut().ok_or_else(|| "settings.json is not a JSON object".to_string())?
        ["env"] = serde_json::to_value(env).map_err(|e| e.to_string())?;
    let serialized = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    let tmp_path = settings_path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp_path, &serialized) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e.to_string());
    }
    if let Err(e) = std::fs::rename(&tmp_path, &settings_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e.to_string());
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(Mutex::new(watcher::WatcherState::default())))
        .setup(|app| {
            #[cfg(desktop)]
            app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            dashboard_pipelines, dashboard_metrics, dashboard_knowledge,
            dashboard_subprojects, dashboard_recipes, dashboard_skills, dashboard_recent_events,
            dashboard_specs, dashboard_spec_markdown,
            dashboard_spec_complete, dashboard_spec_cancel, dashboard_spec_reactivate,
            dashboard_search_events, dashboard_search_knowledge,
            dashboard_telemetry, dashboard_live_activity,
            telemetry::dashboard_prompt_economy,
            telemetry::collector_health,
            dashboard_consumption, dashboard_consumption_global,
            dashboard_activity_aggregated, dashboard_quality_metrics, dashboard_knowledge_browse,
            dashboard_watch_repos, dashboard_active_pipelines,
            dashboard_read_env, dashboard_write_env,
            discover_projects
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
