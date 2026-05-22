mod artifact_update;
pub mod amend_queries;
mod discovery;
pub mod db;
mod prd_lapidator;
pub mod process_util;
mod projects;
pub mod spec_views;
pub mod telemetry;
pub mod telemetry_agg;
mod watcher;

use mustard_core::fs;
use serde::Serialize;
use tauri::Manager;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PipelineSummary {
    pub spec_name: String,
    pub phase: String,
    pub scope: String,
    pub status: String,
    pub updated_at: Option<String>,
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
    // DB wins: status, scope, phase, updated_at — all derived from the pipeline.*
    // event stream via `db::pipelines_from_db`. FS pipeline-states JSON walk is
    // removed; the event log is canonical (spec 2026-05-19-pipeline-state-from-sqlite).
    let base = PathBuf::from(&repo_path);
    if let Some(conn) = db::with_db(&base, |conn| Ok(db::pipelines_from_db(conn))) {
        return conn;
    }
    Ok(vec![])
}

#[tauri::command]
fn dashboard_metrics(repo_path: String) -> Result<MetricsSummary, String> {
    let base = PathBuf::from(&repo_path);
    if let Some(r) = db::with_db(&base, db::metrics_from_db) {
        return r;
    }
    Ok(MetricsSummary { total_events: 0, sessions_recent: 0, agents_dispatched: 0, last_event_at: None, tokens_total: 0, tokens_today: 0 })
}

#[tauri::command]
fn dashboard_knowledge(repo_path: String) -> Result<KnowledgeSummary, String> {
    // Wave 6c: DB is the only source — knowledge.json no longer written by rt.
    // db::knowledge_from_db queries knowledge_patterns (Wave 6a) with fallback
    // to the legacy `knowledge` table for pre-Wave-6a DBs.
    let base = PathBuf::from(&repo_path);
    match db::with_db(&base, db::knowledge_from_db) {
        Some(r) => r,
        None => Ok(KnowledgeSummary { patterns_count: 0, conventions_count: 0, high_confidence_count: 0 }),
    }
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
    let output = crate::process_util::no_window_command("node")
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
    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let path = &entry.path;
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        let content = match fs::read_to_string(path) {
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
        let entries = match fs::read_dir(root) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries {
            let skill_path = entry.path.join("SKILL.md");
            if !skill_path.exists() { continue; }
            let content = match fs::read_to_string(&skill_path) { Ok(c) => c, Err(_) => continue };
            if !content.starts_with("---\n") { continue; }
            let (mut skill_name, description) = parse_skill_frontmatter(&content);
            if skill_name.is_empty() { skill_name = entry.file_name.clone(); }
            results.push(SkillMeta { name: skill_name, description, source: source.to_string() });
        }
    }

    // Walk subproject skills via sync-detect
    let detect = crate::process_util::no_window_command("node")
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
                        let entries = match fs::read_dir(&sub_root) { Ok(e) => e, Err(_) => continue };
                        for entry in entries {
                            let skill_path = entry.path.join("SKILL.md");
                            if !skill_path.exists() { continue; }
                            let content = match fs::read_to_string(&skill_path) { Ok(c) => c, Err(_) => continue };
                            if !content.starts_with("---\n") { continue; }
                            let (mut skill_name, description) = parse_skill_frontmatter(&content);
                            if skill_name.is_empty() {
                                skill_name = entry.file_name.clone();
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

#[tauri::command]
fn dashboard_recent_events(repo_path: String, limit: Option<usize>) -> Result<Vec<RecentEvent>, String> {
    let base = PathBuf::from(&repo_path);
    let cap = limit.unwrap_or(20);
    match db::with_db(&base, |c| db::recent_events_from_db(c, cap)) {
        Some(r) => r,
        None => Ok(vec![]),
    }
}

#[tauri::command]
fn dashboard_specs(repo_path: String) -> Result<Vec<SpecRow>, String> {
    let base = PathBuf::from(&repo_path);
    let spec_root = base.join(".claude").join("spec");

    // The filesystem is the source of truth for *existence* — DB rows may be
    // stale (specs renamed/moved/migrated away). Walk FS first; this also
    // walks wave-plan children and emits them with parent set. The `phase`
    // field, however, comes from the SQLite event log (`pipeline.phase` events)
    // — see the merge below — because the FS pipeline-state JSON no longer
    // carries phase (spec 2026-05-19-dashboard-phase-from-sqlite).
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
            // Enrich FS row with DB-only fields (timestamps, affected_files,
            // and `phase`) when both sides have the spec. Preserve parent from
            // FS row. `phase` is the one field where DB *wins* over FS: the
            // event log is the canonical source for the current phase.
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
                // DB wins on phase. Only fall back to the FS-derived value
                // (parsed from spec.md/wave-plan.md frontmatter) when the DB
                // has no pipeline.phase event recorded yet.
                if row.phase.is_some() {
                    existing.phase = row.phase;
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

// Walk .claude/spec/*/spec.md (and wave-plan.md) for spec existence and
// frontmatter metadata (title, lang, scope). Flat layout: specs live directly
// under .claude/spec/{name}/ for their entire lifecycle — no bucket
// subdirectories (active/completed/cancelled).
//
// DB wins for: status, phase, tasks, wave counts — merged by `dashboard_specs`.
// FS wins for: spec existence, title, narrative (### Lang: / ### Scope:).
//
// The legacy state-file walk was removed in Wave 3b of spec
// 2026-05-19-pipeline-state-from-sqlite: the event log is canonical for all
// pipeline fields; FS JSON files are stale artifacts.
fn specs_from_fs(base: &PathBuf) -> Vec<SpecRow> {
    let spec_root = base.join(".claude").join("spec");
    let mut rows: Vec<(SpecRow, Option<std::time::SystemTime>)> = Vec::new();

    let rd = match fs::read_dir(&spec_root) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    for entry in rd {
        let path = &entry.path;
        if !entry.is_dir {
            continue;
        }
        let name = entry.file_name.clone();
        // Phase and status: parse spec.md / wave-plan.md frontmatter.
        // Status and phase from the DB (pipeline.* events) take precedence
        // in `dashboard_specs::merge` — these FS values are fallbacks only.
        let from_spec = {
            let parsed = parse_spec_md(&path.join("spec.md"));
            if parsed.0.is_some() || parsed.1.is_some() {
                parsed
            } else {
                // Wave-plan parents have no root spec.md — try wave-plan.md.
                parse_spec_md(&path.join("wave-plan.md"))
            }
        };
        // A directory must contain at least spec.md or wave-plan.md to be a spec.
        let has_spec = path.join("spec.md").exists() || path.join("wave-plan.md").exists();
        if !has_spec {
            continue;
        }
        let phase = from_spec.0;
        let status = from_spec.1;
        let mtime = fs::modified(path).ok();
        let parent_row = SpecRow {
            name: name.clone(),
            status,
            phase,
            started_at: None,
            completed_at: None,
            affected_files: vec![],
            bucket: None,
            parent: None,
        };
        rows.push((parent_row, mtime));

        // Wave plan: walk wave-N-*/spec.md children. Each child becomes a
        // SpecRow with parent set to the wave plan's name. The dashboard
        // groups them visually.
        let wave_plan = path.join("wave-plan.md");
        if wave_plan.exists() {
            if let Ok(child_rd) = fs::read_dir(path) {
                for child in child_rd {
                    let cpath = &child.path;
                    if !child.is_dir {
                        continue;
                    }
                    let cname = child.file_name.clone();
                    // Only walk dirs that look like wave-N-something
                    if !cname.starts_with("wave-") {
                        continue;
                    }
                    let cspec = cpath.join("spec.md");
                    if !cspec.exists() {
                        continue;
                    }
                    let (cphase, cstatus) = parse_spec_md(&cspec);
                    let cmtime = fs::modified(cpath).ok();
                    let child_row = SpecRow {
                        name: cname,
                        status: cstatus,
                        phase: cphase,
                        started_at: None,
                        completed_at: None,
                        affected_files: vec![],
                        bucket: None,
                        parent: Some(name.clone()),
                    };
                    rows.push((child_row, cmtime));
                }
            }
        }
    }

    rows.sort_by(|a, b| match (a.1, b.1) {
        (Some(x), Some(y)) => y.cmp(&x),
        _ => b.0.name.cmp(&a.0.name),
    });
    rows.into_iter().map(|(r, _)| r).collect()
}

/// Check whether a spec directory still exists on disk under the flat layout
/// .claude/spec/{name}/ or as a wave child under any parent spec dir.
/// Returns Some("flat") when found, None when historical/missing.
fn detect_spec_existence(spec_root: &PathBuf, name: &str) -> Option<String> {
    // Flat layout: spec lives directly at .claude/spec/{name}/
    if spec_root.join(name).is_dir() {
        return Some("flat".to_string());
    }
    // Could also be a wave child — scan one level for parent dirs that contain
    // a {name}/ subdir (wave-N-something under a wave-plan parent).
    if let Ok(rd) = fs::read_dir(spec_root) {
        for entry in rd {
            if entry.is_dir && entry.path.join(name).is_dir() {
                return Some("flat".to_string());
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
    let content = match fs::read_to_string(path) {
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
    // 1. Standalone spec — flat layout: .claude/spec/{spec_name}/spec.md
    let path = base.join(&spec_name).join("spec.md");
    if path.exists() {
        return fs::read_to_string(&path).map_err(|e| e.to_string());
    }
    // 2. Wave-plan child: dashboard_specs emits children with a bare name
    //    (e.g. "wave-2-frontend") and parent set, but the file actually lives
    //    one level down at .claude/spec/{parent}/{spec_name}/spec.md.
    //    Without the parent, search every spec dir for a matching child.
    let Ok(rd) = fs::read_dir(&base) else {
        return Err(format!("spec markdown not found: {}", spec_name));
    };
    for entry in rd {
        if !entry.is_dir {
            continue;
        }
        let child = entry.path.join(&spec_name).join("spec.md");
        if child.exists() {
            return fs::read_to_string(&child).map_err(|e| e.to_string());
        }
    }
    // 3. Wave-plan parent: roadmap specs carry only a `wave-plan.md` at their
    //    root plus `wave-N-*/spec.md` subdirs — no top-level `spec.md`. Fall
    //    back to the wave-plan file so the side panel renders the plan.
    let wplan = base.join(&spec_name).join("wave-plan.md");
    if wplan.exists() {
        return fs::read_to_string(&wplan).map_err(|e| e.to_string());
    }
    // 4. Symmetry with case 2: a wave-plan parent nested under another spec dir.
    let Ok(rd2) = fs::read_dir(&base) else {
        return Err(format!("spec markdown not found: {}", spec_name));
    };
    for entry in rd2 {
        if !entry.is_dir {
            continue;
        }
        let child = entry.path.join(&spec_name).join("wave-plan.md");
        if child.exists() {
            return fs::read_to_string(&child).map_err(|e| e.to_string());
        }
    }
    Err(format!("spec markdown not found: {}", spec_name))
}

// ── spec status helpers (emit-only, flat layout) ─────────────────────────────
//
// Flat spec layout (spec `2026-05-21-flatten-spec-layout-and-multi-collab`):
// specs live at `.claude/spec/{name}/` for their entire lifecycle; there are
// no bucket subdirectories (active/completed/cancelled). Status is canonical
// in the SQLite event store. These helpers mirror the private functions in
// `spec_views.rs` — duplicated rather than re-exported to avoid splitting the
// module's privacy boundary.

/// Emit `pipeline.status: <to>` via the SQLite event store. Fail-open.
fn lib_emit_pipeline_status(repo_path: &str, spec: &str, to: &str) {
    use mustard_core::model::event::{
        Actor, ActorKind, EVENT_PIPELINE_STATUS, HarnessEvent, PipelineStatusPayload,
        SCHEMA_VERSION,
    };
    use mustard_core::store::event_store::EventSink;
    use mustard_core::store::sqlite_store::SqliteEventStore;

    let payload = serde_json::to_value(PipelineStatusPayload {
        from: None,
        to: to.to_string(),
    }).unwrap_or(serde_json::Value::Null);
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        session_id: String::new(),
        wave: 0,
        actor: Actor { kind: ActorKind::Cli, id: Some("dashboard-spec-status".to_string()), actor_type: None },
        event: EVENT_PIPELINE_STATUS.to_string(),
        payload,
        spec: Some(spec.to_string()),
    };

    // Wave 3: append through the shared, managed store keyed by repo path
    // instead of opening a fresh `SqliteEventStore` per call. `with_store`
    // returns `None` only when the DB file does not yet exist; in that single
    // case fall back to `for_project`, which creates it on open.
    let base = std::path::Path::new(repo_path);
    let appended = db::with_store(base, |store| store.append(&event).map_err(|e| e.to_string()));
    match appended {
        Some(Ok(())) => {}
        Some(Err(e)) => eprintln!("lib_emit_pipeline_status: append: {e}"),
        None => match SqliteEventStore::for_project(repo_path) {
            Ok(store) => {
                if let Err(e) = store.append(&event) {
                    eprintln!("lib_emit_pipeline_status: append (fresh): {e}");
                }
            }
            Err(e) => eprintln!("lib_emit_pipeline_status: open store: {e}"),
        },
    }
}

/// Rewrite `### Status:` header in `.claude/spec/{spec}/spec.md`. Fail-open.
fn lib_sync_spec_status_header(repo_path: &str, spec: &str, to: &str) {
    let path = std::path::Path::new(repo_path)
        .join(".claude").join("spec").join(spec).join("spec.md");
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => { eprintln!("lib_sync_spec_status_header: read {}: {e}", path.display()); return; }
    };
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    let mut rewrote = false;
    for line in lines.iter_mut() {
        if line.trim_start().to_lowercase().starts_with("### status:") {
            *line = format!("### Status: {to}");
            rewrote = true;
            break;
        }
    }
    if !rewrote {
        eprintln!("lib_sync_spec_status_header: no `### Status:` in {}", path.display());
        return;
    }
    let mut out = lines.join("\n");
    if content.ends_with('\n') { out.push('\n'); }
    if let Err(e) = fs::write_atomic(&path, out.as_bytes()) {
        eprintln!("lib_sync_spec_status_header: write {}: {e}", path.display());
    }
}

#[tauri::command]
fn dashboard_spec_complete(repo_path: String, spec_name: String) -> Result<String, String> {
    lib_emit_pipeline_status(&repo_path, &spec_name, "completed");
    lib_sync_spec_status_header(&repo_path, &spec_name, "completed");
    Ok("completed".to_string())
}

#[tauri::command]
fn dashboard_spec_cancel(repo_path: String, spec_name: String) -> Result<String, String> {
    lib_emit_pipeline_status(&repo_path, &spec_name, "cancelled");
    lib_sync_spec_status_header(&repo_path, &spec_name, "cancelled");
    Ok("cancelled".to_string())
}

#[tauri::command]
fn dashboard_spec_reactivate(repo_path: String, spec_name: String) -> Result<String, String> {
    lib_emit_pipeline_status(&repo_path, &spec_name, "implementing");
    lib_sync_spec_status_header(&repo_path, &spec_name, "implementing");
    Ok("implementing".to_string())
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
    // Derive the current session cut-off once and feed it into the
    // accumulator readers so they can report "+N this session" alongside the
    // lifetime totals.
    let session_start = telemetry::session_start_ts(&base);
    let since = session_start.as_deref();
    Ok(telemetry::TelemetrySummary {
        rtk: telemetry::rtk_summary(&base),
        measured: telemetry::measured(&base),
        prevention: telemetry::hook_fire_counts(&base, since),
        routing: telemetry::routing_breakdown(&base, since),
        workflow: telemetry::workflow_by_phase(&base),
        tool_breakdown: telemetry::tool_breakdown(&base),
        agent_activity: telemetry::agent_activity(&base),
        session_start_ts: session_start.clone(),
    })
}

/// Friction telemetry from `.claude/.metrics/friction.json` — measured atrito
/// (hook-retry counts, heavy pipelines). Distinct from knowledge patterns;
/// the Knowledge page renders this in its own "Atrito" section. Empty vec
/// when the file is absent (the common case — friction is rare).
#[tauri::command]
fn dashboard_friction(repo_path: String) -> Result<Vec<telemetry::FrictionEntry>, String> {
    let base = std::path::PathBuf::from(&repo_path);
    Ok(telemetry::friction_entries(&base))
}

/// Live activity derived from mustard.db. Events are written by mustard-rt
/// on every hook dispatch, so the DB always reflects the current session.
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
    pub spec_name: String,
    pub status: String,
    pub phase: String,
    pub current_wave: Option<u32>,
    pub total_waves: Option<u32>,
    pub model: Option<String>,
    pub has_dispatch_failure: bool,
    pub failure_age_ms: Option<u64>,
    pub tasks_pending: usize,
    pub tasks_in_progress: usize,
    pub tasks_completed: usize,
    pub updated_at: Option<String>,
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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
    // DB wins: all pipeline fields — status, phase, wave counts, tasks, dispatch
    // failure — derived from the pipeline.* event stream. FS walk removed (Wave 3b
    // of spec 2026-05-19-pipeline-state-from-sqlite).
    let base = std::path::PathBuf::from(&repo_path);
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut results = match db::with_db(&base, |conn| {
        Ok(db::active_pipelines_from_db(conn, now_secs))
    }) {
        Some(Ok(r)) => r,
        _ => return Ok(vec![]),
    };
    // Filter out completed/closed (same semantics as the old FS-based reader).
    results.retain(|p| p.status != "completed" && p.status != "closed");
    // Sort descending by updated_at.
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
    let content = fs::read_to_string(&settings_path).map_err(|e| e.to_string())?;
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
        let content = fs::read_to_string(&settings_path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).map_err(|e| e.to_string())?
    } else {
        serde_json::json!({})
    };
    value.as_object_mut().ok_or_else(|| "settings.json is not a JSON object".to_string())?
        ["env"] = serde_json::to_value(env).map_err(|e| e.to_string())?;
    let serialized = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    fs::write_atomic(&settings_path, serialized.as_bytes()).map_err(|e| e.to_string())?;
    Ok(())
}

/// Install Mustard's `.claude/` scaffold into `path` (B5 Wave 3).
///
/// Calls `mustard_cli::init` natively — no sidecar process. The CLI runs in
/// its non-interactive mode automatically: with no terminal attached it falls
/// back to a safe merge when `.claude/` already exists, and `yes: true` keeps
/// the git-flow wizard from blocking on a prompt that can never be answered.
///
/// `anyhow::Error` is not `Serialize`, so the error is flattened to a string
/// for the frontend (the Tauri-2 idiom for `Result`-returning commands).
/// Reads `.claude/entity-registry.json` from the given repo root and returns
/// the discovered entity names. Used by the PRD lapidator's EntityPicker
/// (Wave 3 of spec 2026-05-20-dashboard-prd-ai-lapidator).
///
/// Returns an empty list when the registry file is missing or unreadable so
/// the UI never crashes on a project that hasn't been scanned yet. When the
/// registry has an explicit `entities` key (top-level array or object) it is
/// preferred; otherwise we fall back to top-level keys minus reserved `_*`
/// prefixes (today's shape — `_patterns`, `_enums`, `_meta`).
#[tauri::command]
fn read_entity_registry(repo_path: String) -> Result<Vec<String>, String> {
    let registry_path = PathBuf::from(&repo_path)
        .join(".claude")
        .join("entity-registry.json");
    if !registry_path.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(&registry_path).map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    // Preferred: explicit `entities` key.
    if let Some(entities) = v.get("entities") {
        if let Some(arr) = entities.as_array() {
            let names: Vec<String> = arr
                .iter()
                .filter_map(|e| e.as_str().map(|s| s.to_string()))
                .collect();
            return Ok(names);
        }
        if let Some(obj) = entities.as_object() {
            return Ok(obj.keys().cloned().collect());
        }
    }
    // Fallback: top-level keys minus reserved `_*` prefixes.
    let obj = match v.as_object() {
        Some(o) => o,
        None => return Ok(vec![]),
    };
    let mut names: Vec<String> = obj
        .keys()
        .filter(|k| !k.starts_with('_'))
        .cloned()
        .collect();
    names.sort();
    Ok(names)
}

#[tauri::command]
fn mustard_install(path: String) -> Result<(), String> {
    let options = mustard_cli::InitOptions {
        yes: true,
        ..Default::default()
    };
    mustard_cli::init(std::path::Path::new(&path), &options).map_err(|e| format!("{e:#}"))
}

/// Refresh an existing Mustard install at `path` (B5 Wave 3).
///
/// Calls `mustard_cli::update` natively. `force: true` skips the confirmation
/// prompt (there is no terminal in the GUI); the timestamped backup the CLI
/// takes is never skipped.
#[tauri::command]
fn mustard_update(path: String) -> Result<(), String> {
    let options = mustard_cli::UpdateOptions { force: true };
    mustard_cli::update(std::path::Path::new(&path), &options).map_err(|e| format!("{e:#}"))
}

// ── Wave-2 per-spec rollup commands ──────────────────────────────────────────

/// Wave 4 (2026-05-20) — these spec commands now delegate to
/// `mustard-core` via the `*_v2` adapters in `spec_views.rs`. The legacy
/// fallback that hard-coded `"unknown"`/`0` for missing data is gone: a spec
/// with no events resolves to the typed `SpecStatus::NoEvents`, which the
/// adapter surfaces as the `"no-events"` string, and the UI can render an
/// honest empty state.
#[tauri::command]
fn dashboard_spec_card(repo_path: String, spec: String) -> Result<spec_views::SpecCard, String> {
    match spec_views::spec_card_v2(&repo_path, &spec)? {
        Some(card) => Ok(card),
        None => Ok(spec_views::SpecCard {
            spec,
            status: "no-events".to_string(),
            phase: String::new(),
            scope: None,
            started_at: None,
            last_event_at: None,
            duration_ms: None,
            current_wave: None,
            total_waves: None,
            ac_passed: 0,
            ac_total: 0,
            files_touched: 0,
            tools_used: 0,
            model: None,
            children_count: 0,
        }),
    }
}

#[tauri::command]
fn dashboard_spec_waves(repo_path: String, spec: String) -> Result<Vec<spec_views::SpecWave>, String> {
    spec_views::spec_waves_v2(&repo_path, &spec)
}

#[tauri::command]
fn dashboard_spec_quality(repo_path: String, spec: String) -> Result<Vec<spec_views::SpecQualityItem>, String> {
    spec_views::spec_quality_v2(&repo_path, &spec)
}

#[tauri::command]
fn dashboard_spec_timeline(repo_path: String, spec: String) -> Result<Vec<spec_views::SpecTimelineNode>, String> {
    spec_views::spec_timeline_v2(&repo_path, &spec)
}

#[tauri::command]
fn dashboard_spec_events(
    repo_path: String,
    spec: String,
    filter: Option<spec_views::EventFilter>,
) -> Result<Vec<spec_views::TimelineEvent>, String> {
    let base = std::path::PathBuf::from(&repo_path);
    match db::with_db(&base, |conn| spec_views::spec_events(conn, &spec, filter)) {
        Some(r) => r,
        None => Ok(vec![]),
    }
}

#[tauri::command]
fn dashboard_spec_action(repo_path: String, spec: String, action: String) -> Result<spec_views::SpecAction, String> {
    let base = std::path::PathBuf::from(&repo_path);
    let action_kind = match action.to_lowercase().as_str() {
        "reopen" => spec_views::SpecActionKind::Reopen,
        "close"  => spec_views::SpecActionKind::Close,
        "remove" => spec_views::SpecActionKind::Remove,
        other    => return Err(format!("unknown action: {}", other)),
    };
    match db::with_db(&base, |conn| spec_views::spec_action(conn, &repo_path, &spec, action_kind)) {
        Some(r) => r,
        None => Ok(spec_views::SpecAction {
            action,
            spec,
            result: "error".to_string(),
            message: Some("banco de dados indisponível".to_string()),
        }),
    }
}

/// Wave-3 (2026-05-20, spec `2026-05-20-tactical-fix-via-sub-spec`) — list
/// sub-specs linked to `parent` via `spec.link` events. Delegates to
/// `spec_views::spec_children_v2` which in turn calls
/// `mustard_core::SpecReader::children_of`.
#[tauri::command]
async fn dashboard_spec_children(
    repo_path: String,
    parent: String,
) -> Result<Vec<spec_views::SpecChild>, String> {
    spec_views::spec_children_v2(&repo_path, &parent)
}

/// Wave 3 (spec-lifecycle-unification) — shell out to `mustard-rt run
/// spec-children-tree --spec NAME` and return the parsed `ChildrenTree`
/// (waves + acceptance criteria + sub-specs) for the dense `/specs` drill-down.
#[tauri::command]
async fn spec_children_tree(
    spec: String,
    project_path: String,
) -> Result<spec_views::ChildrenTree, String> {
    spec_views::spec_children_tree_run(&project_path, &spec)
}

/// Wave 4 (2026-05-20, spec `mustard-wave-network-standard`) — shell out to
/// `mustard-rt run metrics wave-status --spec <name>` and return the typed
/// `MetricsWaveStatus`. Audit-2 in this wave's `metrics-audit.md` documents
/// why this exists (the page was never wired to the wave-status output).
#[tauri::command]
fn dashboard_metrics_wave_status(
    repo_path: String,
    spec_name: String,
) -> Result<spec_views::MetricsWaveStatus, String> {
    spec_views::dashboard_metrics_wave_status_run(&repo_path, &spec_name)
}

/// Wave 3 (2026-05-20, spec `mustard-wave-network-standard`) — shell out to
/// `mustard-rt run wikilink-extract --spec-dir <dir>` for the spec resolved
/// from `spec_name` and return the parsed `{wikilinks, orphans}` payload. The
/// `SpecNetworkTab` consumes this to render the dependency graph.
#[tauri::command]
fn dashboard_wikilink_extract(
    repo_path: String,
    spec_name: String,
) -> Result<spec_views::WikilinkExtract, String> {
    spec_views::dashboard_wikilink_extract_run(&repo_path, &spec_name)
}

/// Wave 3 (2026-05-20) — shell out to `mustard-rt run memory cross-wave
/// --spec <name> --wave <n>` and return the markdown stdout. Empty string
/// when there is no cross-wave memory yet (the common case for early waves).
#[tauri::command]
fn dashboard_memory_cross_wave(
    repo_path: String,
    spec: String,
    wave: u32,
) -> Result<String, String> {
    spec_views::dashboard_memory_cross_wave_run(&repo_path, &spec, wave)
}

/// Wave 2 (2026-05-21, spec `2026-05-21-dashboard-spec-tabs`) — shell out to
/// `mustard-rt run wave-files --spec <name> --wave <N>` and return the typed
/// payload (real file count from the wave sub-spec's `## Arquivos` block plus
/// the full markdown for the wave drawer). Mirrors the spawn pattern used by
/// `dashboard_metrics_wave_status` / `dashboard_memory_cross_wave`.
#[tauri::command]
async fn dashboard_spec_wave_files(
    repo_path: String,
    spec: String,
    wave: u32,
) -> Result<spec_views::WaveFilesPayload, String> {
    spec_views::dashboard_spec_wave_files_run(&repo_path, &spec, wave)
}

/// Wave 1 (2026-05-21, spec `2026-05-21-dashboard-spec-tabs-polish`) — scan
/// `<repo>/.claude/spec/{spec}/wave-N-{role}/` and return the wave structure
/// declared on disk, independent of whether the SQLite event log has caught
/// up. The `SpecWavesTab` unions this with the projection from
/// `dashboard_spec_waves` so the tab shows the full wave plan during EXECUTE
/// (waves declared but not yet emitting events render as `queued`).
#[tauri::command]
async fn dashboard_spec_waves_planned(
    repo_path: String,
    spec: String,
) -> Result<Vec<spec_views::SpecWavePlanned>, String> {
    spec_views::dashboard_spec_waves_planned_run(&repo_path, &spec)
}

/// Wave 4 (2026-05-20) — delegate to `mustard-core::workspace_summary`.
/// Fixes the previous `events_per_minute` SQL filter that silently
/// short-circuited (returned the all-time count → `2904.0` in the audit) and
/// the `tokens_saved_today LIKE '%token%saved%'` query that never matched
/// any real event. The new projection counts events strictly within the
/// trailing 60-second window and sums RTK/hook/routing savings events.
#[tauri::command]
fn dashboard_workspace_summary(repo_path: String) -> Result<spec_views::WorkspaceSummary, String> {
    spec_views::workspace_summary_v2(&repo_path)
}

// ── Wave-6 hygiene observability ─────────────────────────────────────────────

/// Return hygiene health roll-up for a project's mustard.db. Fail-open:
/// returns an all-zeros `WorkspaceHealth` when the DB is absent.
#[tauri::command]
fn workspace_health(repo_path: String) -> spec_views::WorkspaceHealth {
    let base = std::path::PathBuf::from(&repo_path);
    match db::with_db(&base, spec_views::workspace_health_impl) {
        Some(Ok(h)) => h,
        _ => spec_views::WorkspaceHealth::default(),
    }
}

// ── Wave-7 telemetry aggregation commands ────────────────────────────────────

#[tauri::command]
fn dashboard_telemetry_phases(
    repo_path: String,
    time_range: String,
) -> Result<Vec<telemetry_agg::PhaseSummary>, String> {
    let base = std::path::PathBuf::from(&repo_path);
    match db::with_db(&base, |conn| telemetry_agg::telemetry_phases(conn, &time_range)) {
        Some(r) => r,
        None => Ok(vec![]),
    }
}

#[tauri::command]
fn dashboard_telemetry_timeline(
    repo_path: String,
    time_range: String,
    limit: Option<usize>,
) -> Result<Vec<telemetry_agg::TimelineEvent>, String> {
    let base = std::path::PathBuf::from(&repo_path);
    let cap = limit.unwrap_or(50);
    match db::with_db(&base, |conn| telemetry_agg::telemetry_timeline(conn, &time_range, cap)) {
        Some(r) => r,
        None => Ok(vec![]),
    }
}

#[tauri::command]
fn dashboard_telemetry_heatmap(
    repo_path: String,
    time_range: String,
) -> Result<Vec<telemetry_agg::HeatmapCell>, String> {
    let base = std::path::PathBuf::from(&repo_path);
    match db::with_db(&base, |conn| telemetry_agg::telemetry_heatmap(conn, &time_range)) {
        Some(r) => r,
        None => Ok(vec![]),
    }
}

#[tauri::command]
fn dashboard_telemetry_history(
    repo_path: String,
    time_range: String,
    limit: Option<usize>,
) -> Result<Vec<telemetry_agg::HistoryEntry>, String> {
    let base = std::path::PathBuf::from(&repo_path);
    let cap = limit.unwrap_or(50);
    match db::with_db(&base, |conn| telemetry_agg::telemetry_history(conn, &time_range, cap)) {
        Some(r) => r,
        None => Ok(vec![]),
    }
}

#[tauri::command]
fn dashboard_telemetry_criteria(
    repo_path: String,
    time_range: String,
) -> Result<Vec<telemetry_agg::AcceptanceCriterion>, String> {
    let base = std::path::PathBuf::from(&repo_path);
    match db::with_db(&base, |conn| telemetry_agg::telemetry_criteria(conn, &time_range)) {
        Some(r) => r,
        None => Ok(vec![]),
    }
}

#[tauri::command]
fn dashboard_telemetry_effort(
    repo_path: String,
    time_range: String,
) -> Result<telemetry_agg::EffortBreakdown, String> {
    let base = std::path::PathBuf::from(&repo_path);
    match db::with_db(&base, |conn| telemetry_agg::telemetry_effort(conn, &time_range)) {
        Some(r) => r,
        None => Ok(telemetry_agg::EffortBreakdown {
            top_files: vec![],
            top_tools: vec![],
            top_phases: vec![],
            top_agents: vec![],
        }),
    }
}

#[tauri::command]
fn dashboard_telemetry_agents(
    repo_path: String,
    time_range: String,
) -> Result<Vec<telemetry_agg::AgentDispatch>, String> {
    let base = std::path::PathBuf::from(&repo_path);
    match db::with_db(&base, |conn| telemetry_agg::telemetry_agents(conn, &time_range)) {
        Some(r) => r,
        None => Ok(vec![]),
    }
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
        // Shared DB handle (Wave 3): one `SqliteEventStore` per repo path, opened
        // once and reused, instead of a fresh connection per command. Registered
        // in managed state so it lives for the app's lifetime; the same cache is
        // also handed to `db::init_db_cache` so the free `db::with_db` helpers
        // reach it without threading `State<DbCache>` through every command.
        .manage(mustard_core::store::db_cache::DbCache::new())
        .setup(|app| {
            #[cfg(desktop)]
            app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
            // Hand the managed cache to the db module's process-global handle.
            // `DbCache` is `Clone` (its map lives behind an `Arc`), so the
            // managed copy and the `db` module's copy share the same open stores.
            let cache = app
                .state::<mustard_core::store::db_cache::DbCache>()
                .inner()
                .clone();
            db::init_db_cache(cache);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            dashboard_pipelines, dashboard_metrics, dashboard_knowledge,
            dashboard_subprojects, dashboard_recipes, dashboard_skills, dashboard_recent_events,
            dashboard_specs, dashboard_spec_markdown,
            dashboard_spec_complete, dashboard_spec_cancel, dashboard_spec_reactivate,
            dashboard_search_events, dashboard_search_knowledge,
            dashboard_telemetry, dashboard_live_activity, dashboard_friction,
            telemetry::dashboard_prompt_economy,
            telemetry::dashboard_economy_summary,
            telemetry::dashboard_economy_savings_breakdown,
            telemetry::dashboard_economy_context_routing,
            telemetry::dashboard_spec_trace,
            telemetry::collector_health,
            dashboard_consumption, dashboard_consumption_global,
            dashboard_activity_aggregated, dashboard_quality_metrics, dashboard_knowledge_browse,
            dashboard_watch_repos, dashboard_active_pipelines,
            dashboard_read_env, dashboard_write_env,
            discover_projects,
            mustard_install, mustard_update,
            projects::detect_project_mustard,
            projects::uninstall_mustard,
            artifact_update::artifact_update_check,
            artifact_update::artifact_update_apply,
            artifact_update::is_mustard_repo,
            dashboard_telemetry_phases,
            dashboard_telemetry_timeline,
            dashboard_telemetry_heatmap,
            dashboard_telemetry_history,
            dashboard_telemetry_criteria,
            dashboard_telemetry_effort,
            dashboard_telemetry_agents,
            amend_queries::amend_resolution_rate,
            amend_queries::amend_drift_rate,
            amend_queries::cross_session_amend_count,
            amend_queries::amend_window_duration,
            dashboard_spec_card,
            dashboard_spec_waves,
            dashboard_spec_quality,
            dashboard_spec_timeline,
            dashboard_spec_events,
            dashboard_spec_action,
            dashboard_spec_children,
            spec_children_tree,
            dashboard_workspace_summary,
            dashboard_metrics_wave_status,
            dashboard_wikilink_extract,
            dashboard_memory_cross_wave,
            dashboard_spec_wave_files,
            dashboard_spec_waves_planned,
            spec_views::dashboard_token_summary,
            spec_views::dashboard_month_activity,
            spec_views::dashboard_events_feed,
            prd_lapidator::lapidate_prd,
            prd_lapidator::check_claude_available,
            read_entity_registry,
            workspace_health
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
