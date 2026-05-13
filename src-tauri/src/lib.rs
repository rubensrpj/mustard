mod discovery;
pub mod db;

use serde::Serialize;
use std::collections::HashSet;
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
            if !skill_path.exists() {
                continue;
            }
            let content = match std::fs::read_to_string(&skill_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if !content.starts_with("---\n") {
                continue;
            }
            let mut name = String::new();
            let mut description = String::new();
            let lines: Vec<&str> = content.lines().collect();
            let mut i = 1usize;
            while i < lines.len() && lines[i] != "---" {
                let line = lines[i];
                if let Some(pos) = line.find(':') {
                    let key = line[..pos].trim();
                    let val = line[pos + 1..].trim();
                    let val = val.trim_matches(|c| c == '\'' || c == '"');
                    if key == "name" {
                        name = val.to_string();
                    } else if key == "description" {
                        if val == "|" || val == ">" {
                            // multi-line: grab first non-empty indented continuation
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
            if name.is_empty() {
                name = entry.file_name().to_string_lossy().to_string();
            }
            results.push(SkillMeta { name, description, source: source.to_string() });
        }
    }
    Ok(results)
}

#[tauri::command]
fn dashboard_recent_events(repo_path: String, limit: Option<usize>) -> Result<Vec<RecentEvent>, String> {
    let base = PathBuf::from(&repo_path);
    let cap = limit.unwrap_or(20);
    if let Some(r) = db::with_db(&base, |c| db::recent_events_from_db(c, cap)) {
        return r;
    }
    let path = base.join(".claude").join(".harness").join("events.jsonl");
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let valid: Vec<serde_json::Value> = content
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .collect();
    let slice = if valid.len() > cap { &valid[valid.len() - cap..] } else { &valid[..] };
    let results = slice.iter().map(|v| RecentEvent {
        event_type: v["event"].as_str().or_else(|| v["type"].as_str()).unwrap_or("unknown").to_string(),
        ts: v["ts"].as_str().or_else(|| v["timestamp"].as_str()).map(|s| s.to_string()),
        summary: v["summary"].as_str().or_else(|| v["description"].as_str()).map(|s| s.to_string()),
    }).collect();
    Ok(results)
}

#[tauri::command]
fn dashboard_specs(repo_path: String) -> Result<Vec<SpecRow>, String> {
    let base = PathBuf::from(&repo_path);
    match db::with_db(&base, db::specs_from_db) {
        Some(r) => r,
        None => Ok(vec![]),
    }
}

#[tauri::command]
fn dashboard_spec_markdown(repo_path: String, spec_name: String) -> Result<String, String> {
    let base = PathBuf::from(&repo_path).join(".claude").join("spec");
    for sub in ["active", "completed"] {
        let path = base.join(sub).join(&spec_name).join("spec.md");
        if path.exists() {
            return std::fs::read_to_string(&path).map_err(|e| e.to_string());
        }
    }
    Err(format!("spec markdown not found: {}", spec_name))
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
fn discover_projects(root: String) -> Result<Vec<discovery::Project>, String> {
    discovery::discover(std::path::Path::new(&root))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            #[cfg(desktop)]
            app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            dashboard_pipelines, dashboard_metrics, dashboard_knowledge,
            dashboard_subprojects, dashboard_recipes, dashboard_skills, dashboard_recent_events,
            dashboard_specs, dashboard_spec_markdown, dashboard_search_events, dashboard_search_knowledge,
            discover_projects
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
