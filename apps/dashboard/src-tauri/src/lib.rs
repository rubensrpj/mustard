mod artifact_update;
pub mod amend_queries;
pub mod commands;
mod discovery;
pub mod doctor;
pub mod economy;
mod file_read;
mod git_info;
mod prd_lapidator;
mod project_overview;
pub mod process_util;
mod projects;
pub mod spec_views;
pub mod telemetry;
pub mod telemetry_agg;
mod watcher;

use mustard_core::io::fs;
use serde::Serialize;
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

#[derive(Serialize, Default)]
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

#[derive(Serialize, Clone)]
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

/// Off-main-thread wrapper for [`dashboard_pipelines_impl`] — it runs the same
/// heavy per-spec fold as `dashboard_active_pipelines`, so on a cold cache it
/// pays the full workspace parse. A join error degrades to an empty list. See
/// [`dashboard_metrics`] for the rationale.
#[tauri::command]
async fn dashboard_pipelines(repo_path: String) -> Result<Vec<PipelineSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || dashboard_pipelines_impl(repo_path))
        .await
        .unwrap_or_else(|_| Ok(Vec::new()))
}

fn dashboard_pipelines_impl(repo_path: String) -> Result<Vec<PipelineSummary>, String> {
    // Onda 2: this legacy command has no live caller (superseded by
    // `dashboard_active_pipelines`). Alias it to the same NDJSON-backed
    // active-pipelines data, projected down to the leaner `PipelineSummary`
    // shape so any stray consumer still sees real rows.
    let actives = dashboard_active_pipelines_impl(repo_path)?;
    Ok(actives
        .into_iter()
        .map(|p| PipelineSummary {
            spec_name: p.spec_name,
            phase: p.phase,
            scope: String::new(),
            status: p.status,
            updated_at: p.updated_at,
        })
        .collect())
}

/// Off-main-thread wrapper for [`dashboard_metrics_impl`]. The body does a full
/// workspace walk; running it on the main thread froze the UI under the live
/// refresh burst. `spawn_blocking` moves the work off the UI thread so commands
/// for different projects run concurrently. A join error (panic in the closure)
/// degrades to a zeroed summary — never an Err toast (the failure-tolerant
/// contract). The sync `_impl` is kept so unit tests call it directly.
#[tauri::command]
async fn dashboard_metrics(repo_path: String) -> Result<MetricsSummary, String> {
    tauri::async_runtime::spawn_blocking(move || dashboard_metrics_impl(repo_path))
        .await
        .unwrap_or_else(|_| Ok(MetricsSummary::default()))
}

fn dashboard_metrics_impl(repo_path: String) -> Result<MetricsSummary, String> {
    // Onda 2: NDJSON-backed metrics. total_events = count over the complete
    // walker; last_event_at = max ts; agents_dispatched = agent_activity
    // total; tokens = measured() sum. sessions_recent = distinct session_ids
    // active within the open-session window.
    let base = PathBuf::from(&repo_path);
    let events = telemetry::walk_ndjson_events_cached(&base);
    let mut total_events = 0usize;
    let mut last_event_at: Option<String> = None;
    let mut recent_sessions: std::collections::HashSet<String> = std::collections::HashSet::new();
    let now_ms = chrono::Utc::now().timestamp_millis();
    const RECENT_WINDOW_MS: i64 = 15 * 60 * 1000;
    for ev in events.iter() {
        total_events += 1;
        if let Some(ts) = ev.get("ts").and_then(|t| t.as_str()) {
            if last_event_at.as_deref().map_or(true, |cur| ts > cur) {
                last_event_at = Some(ts.to_string());
            }
            if let (Some(sid), Some(ms)) = (
                ev.get("session_id").and_then(|s| s.as_str()).filter(|s| !s.is_empty()),
                telemetry::iso_to_ms_crate(ts),
            ) {
                if now_ms - ms <= RECENT_WINDOW_MS {
                    recent_sessions.insert(sid.to_string());
                }
            }
        }
    }
    let agents = telemetry::agent_activity(&base);
    let m = telemetry::measured(&base);
    Ok(MetricsSummary {
        total_events,
        sessions_recent: recent_sessions.len(),
        agents_dispatched: usize::try_from(agents.total_dispatches).unwrap_or(usize::MAX),
        last_event_at,
        tokens_total: m.tokens_total,
        tokens_today: m.tokens_today,
    })
}

#[tauri::command]
fn dashboard_knowledge(repo_path: String) -> Result<KnowledgeSummary, String> {
    // §5: there are NO knowledge events in the NDJSON stream. Read the on-disk
    // `.claude/knowledge/` files (markdown/JSON) and project them honestly. A
    // missing dir yields all-zeros (the empty state).
    let rows = read_knowledge_rows(&PathBuf::from(&repo_path));
    let patterns_count = rows.iter().filter(|r| r.type_ == "pattern").count();
    let conventions_count = rows.iter().filter(|r| r.type_ == "convention").count();
    let high_confidence_count = rows.iter().filter(|r| r.confidence >= 0.8).count();
    Ok(KnowledgeSummary {
        patterns_count,
        conventions_count,
        high_confidence_count,
    })
}

/// §5: project the on-disk `.claude/knowledge/*.md` files into [`KnowledgeRow`]s.
///
/// There are NO knowledge events in the NDJSON stream, so the honest source is
/// the captured-knowledge markdown the harness writes. Each file is YAML
/// frontmatter (`kind`, `captured_at`, `source_event`, `spec`) plus a markdown
/// body. We map:
///   * `id`          → file stem
///   * `type_`       → frontmatter `kind` (e.g. `decision`, `pattern`, `convention`)
///   * `name`        → first non-empty body line (heading stripped), truncated
///   * `description` → the full body, trimmed
///   * `confidence`  → frontmatter `confidence` if present (0..1), else 1.0 —
///                     captured decisions are confirmed, not probabilistic, so
///                     a confirmed entry reads as fully confident. No score is
///                     fabricated: when the file declares one we honour it.
///   * `source`      → frontmatter `spec` (falls back to `source_event`)
///
/// Fail-open: a missing dir / unreadable file yields an empty list.
fn read_knowledge_rows(base: &std::path::Path) -> Vec<KnowledgeRow> {
    let dir = base.join(".claude").join("knowledge");
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut rows: Vec<KnowledgeRow> = Vec::new();
    for entry in entries {
        let path = &entry.path;
        if entry.is_dir {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let (front, body) = split_frontmatter(&content);
        let kind = yaml_value(&front, "kind").unwrap_or_else(|| "decision".to_string());
        let source = yaml_value(&front, "spec")
            .or_else(|| yaml_value(&front, "source_event"));
        let confidence = yaml_value(&front, "confidence")
            .and_then(|v| v.parse::<f64>().ok())
            .map(|c| c.clamp(0.0, 1.0))
            .unwrap_or(1.0);
        let body_trim = body.trim();
        let name = body_trim
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(|l| l.trim_start_matches('#').trim())
            .map(|l| l.chars().take(120).collect::<String>())
            .unwrap_or_default();
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        rows.push(KnowledgeRow {
            id,
            type_: kind,
            name,
            description: body_trim.to_string(),
            confidence,
            source,
        });
    }
    // Most-recent first (file stems are ISO-ish timestamps → lexical sort).
    rows.sort_by(|a, b| b.id.cmp(&a.id));
    rows
}

/// Split a markdown document into `(frontmatter, body)`. When the file opens
/// with a `---` fence the frontmatter is everything up to the closing `---`;
/// otherwise the whole document is the body and the frontmatter is empty.
fn split_frontmatter(content: &str) -> (String, String) {
    let stripped = content.strip_prefix('\u{FEFF}').unwrap_or(content);
    if let Some(after) = stripped.strip_prefix("---\n").or_else(|| stripped.strip_prefix("---\r\n")) {
        if let Some(end) = after.find("\n---") {
            let front = after[..end].to_string();
            let rest = &after[end + 4..];
            let body = rest.strip_prefix('\n').or_else(|| rest.strip_prefix("\r\n")).unwrap_or(rest);
            return (front, body.to_string());
        }
    }
    (String::new(), content.to_string())
}

/// Read one `key: value` scalar out of a YAML frontmatter block. Returns the
/// trimmed, unquoted value or `None`.
fn yaml_value(front: &str, key: &str) -> Option<String> {
    for line in front.lines() {
        let mut parts = line.splitn(2, ':');
        let k = parts.next()?.trim();
        if !k.eq_ignore_ascii_case(key) {
            continue;
        }
        let v = parts.next()?.trim().trim_matches(|c| c == '"' || c == '\'');
        if v.is_empty() {
            return None;
        }
        return Some(v.to_string());
    }
    None
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SubprojectInfo {
    name: String,
    role: Option<String>,
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
    // Subprojects come from grain's repo model (`read_projects` → the scan
    // tool's `facts`; the model is never parsed directly). The old per-project
    // generated agents are gone, so `role` is no longer derived.
    let model = PathBuf::from(&repo_path).join(".claude").join("grain.model.json");
    let results = mustard_core::read_projects(&model)
        .into_iter()
        .map(|p| SubprojectInfo {
            name: if p.dir.is_empty() { p.name } else { p.dir },
            role: None,
        })
        .collect();
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

    // Per-subproject skills are no longer generated by mustard (the scan tool
    // writes nothing into subprojects), so there is no subproject skill walk.

    Ok(results)
}

/// Off-main-thread wrapper for [`dashboard_recent_events_impl`] (full workspace
/// walk + sort). A join error degrades to an empty feed. See
/// [`dashboard_metrics`] for the rationale.
#[tauri::command]
async fn dashboard_recent_events(
    repo_path: String,
    limit: Option<usize>,
) -> Result<Vec<RecentEvent>, String> {
    tauri::async_runtime::spawn_blocking(move || dashboard_recent_events_impl(repo_path, limit))
        .await
        .unwrap_or_else(|_| Ok(Vec::new()))
}

fn dashboard_recent_events_impl(repo_path: String, limit: Option<usize>) -> Result<Vec<RecentEvent>, String> {
    // Onda 2 (HIGH-VALUE): chronological tail over the complete walker
    // (spec `.events/` + wave subdirs + `.session/`). Newest first.
    let base = PathBuf::from(&repo_path);
    let events = telemetry::walk_ndjson_events_cached(&base);
    // Read-time attribution: resolve spec-less session events to their
    // time-ordered session→spec binding so per-spec slices of this feed surface
    // them. Built once over the full slice.
    let timeline = telemetry::build_session_spec_timeline_from(&events);
    // Sort a reference view by ts desc (ISO-8601 is lexically chronological);
    // ts-less rows sink. We sort `&Value` refs rather than the shared cached
    // slice itself — the cache hands out immutable data.
    let mut ordered: Vec<&serde_json::Value> = events.iter().collect();
    ordered.sort_by(|a, b| {
        let ta = a.get("ts").and_then(|t| t.as_str()).unwrap_or("");
        let tb = b.get("ts").and_then(|t| t.as_str()).unwrap_or("");
        tb.cmp(ta)
    });
    let cap = limit.unwrap_or(100).min(2000);
    Ok(ordered
        .into_iter()
        .take(cap)
        .map(|v| recent_event_from_value_attributed(v, Some(&timeline)))
        .collect())
}

/// Map a raw NDJSON record into the [`RecentEvent`] shape the dashboard's
/// activity feeds consume. Reuses the harness event NAME (`event` ?? `kind`)
/// and pulls the common attribution + tool/target fields out of the record /
/// payload. The `summary` is a compact human label derived per event family.
///
/// When the record carries no explicit `spec` and a `timeline` is supplied, the
/// `spec` field is resolved through the time-ordered session→spec binding. This
/// surfaces spec-less session events (`tool.use` / `agent.*` written under
/// `.claude/.session/{id}/`) in the per-spec slices the dashboard derives from
/// these feeds. An explicit non-empty `spec` is always honoured (never
/// overridden); pass `None` to skip attribution entirely.
fn recent_event_from_value_attributed(
    v: &serde_json::Value,
    timeline: Option<&telemetry::SessionSpecTimeline>,
) -> RecentEvent {
    let event_type = telemetry::event_name_of(v).to_string();
    let ts = v.get("ts").and_then(|t| t.as_str()).map(str::to_string);
    let payload = v.get("payload");
    let spec = v
        .get("spec")
        .or_else(|| payload.and_then(|p| p.get("spec")))
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            timeline
                .and_then(|t| t.attributed_spec(v))
                .map(str::to_string)
        });
    let wave = v
        .get("wave")
        .and_then(serde_json::Value::as_i64)
        .or_else(|| payload.and_then(|p| p.get("wave")).and_then(serde_json::Value::as_i64));
    // actor can be a bare string or an object { kind, id }.
    let (actor_kind, actor_id) = match v.get("actor") {
        Some(serde_json::Value::String(s)) => (None, Some(s.clone())),
        Some(serde_json::Value::Object(o)) => (
            o.get("kind").and_then(|k| k.as_str()).map(str::to_string),
            o.get("id").and_then(|i| i.as_str()).map(str::to_string),
        ),
        _ => (None, None),
    };
    let tool_name = payload
        .and_then(|p| p.get("tool").or_else(|| p.get("tool_name")))
        .and_then(|t| t.as_str())
        .map(str::to_string);
    let target = payload
        .and_then(|p| p.get("target"))
        .and_then(|t| {
            t.as_object().and_then(|o| {
                o.get("file_path")
                    .or_else(|| o.get("file"))
                    .or_else(|| o.get("command"))
                    .or_else(|| o.get("description"))
                    .and_then(|x| x.as_str())
            })
            .or_else(|| t.as_str())
        })
        .map(str::to_string);
    let phase = payload
        .and_then(|p| p.get("to").or_else(|| p.get("phase")))
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let summary = event_summary(&event_type, payload, tool_name.as_deref(), target.as_deref(), phase.as_deref());
    RecentEvent {
        event_type,
        ts,
        summary: Some(summary),
        spec,
        wave,
        actor_kind,
        actor_id,
        tool_name,
        target,
        phase,
    }
}

/// Compact ≤120-char human label for an event, per family. Shared by the
/// recent-events / search / feed / timeline aggregators.
fn event_summary(
    event_type: &str,
    payload: Option<&serde_json::Value>,
    tool_name: Option<&str>,
    target: Option<&str>,
    phase: Option<&str>,
) -> String {
    let s = match event_type {
        "tool.use" => match (tool_name, target) {
            (Some(t), Some(g)) => format!("{t} · {g}"),
            (Some(t), None) => t.to_string(),
            _ => "tool".to_string(),
        },
        "pipeline.phase" => match phase {
            Some(p) => format!("→ {p}"),
            None => "phase".to_string(),
        },
        "pipeline.status" => {
            let to = payload
                .and_then(|p| p.get("to"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            format!("status → {to}")
        }
        "agent.start" | "agent.stop" => {
            let at = payload
                .and_then(|p| p.get("subagentType").or_else(|| p.get("agent_type")))
                .and_then(|x| x.as_str())
                .unwrap_or("agent");
            format!("{event_type} {at}")
        }
        "qa.result" => {
            let overall = payload
                .and_then(|p| p.get("overall"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            format!("qa {overall}")
        }
        "review.result" => {
            let verdict = payload
                .and_then(|p| p.get("verdict").or_else(|| p.get("overall")))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            format!("review {verdict}")
        }
        "pipeline.change.request" => {
            let stage = payload
                .and_then(|p| p.get("stage"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let preview: String = payload
                .and_then(|p| p.get("prompt"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .chars()
                .take(60)
                .collect();
            match (stage.is_empty(), preview.is_empty()) {
                (false, false) => format!("solicitação ({stage}) — {preview}"),
                (true, false) => format!("solicitação — {preview}"),
                (false, true) => format!("solicitação ({stage})"),
                (true, true) => "solicitação".to_string(),
            }
        }
        other => other.to_string(),
    };
    s.chars().take(120).collect()
}

/// Off-main-thread wrapper for [`dashboard_specs_impl`] (spec-list walk; served
/// from the specs cache when warm). A join error degrades to an empty list.
/// See [`dashboard_metrics`] for the rationale.
#[tauri::command]
async fn dashboard_specs(repo_path: String) -> Result<Vec<SpecRow>, String> {
    tauri::async_runtime::spawn_blocking(move || dashboard_specs_impl(repo_path))
        .await
        .unwrap_or_else(|_| Ok(Vec::new()))
}

fn dashboard_specs_impl(repo_path: String) -> Result<Vec<SpecRow>, String> {
    let base = PathBuf::from(&repo_path);

    // The filesystem is the source of truth for spec existence. The walk also
    // covers wave-plan children, emitting them with parent set. Post-W6A there
    // is no SQLite event-log merge — phase/timestamps that the legacy DB
    // enriched here come back unset until the NDJSON projection lands (Onda 2);
    // `phase` falls back to the value parsed from spec.md/wave-plan.md frontmatter.
    // Served from the watcher-invalidated specs cache: spec.md is tiny markdown,
    // the list route must never wait on a directory walk when nothing changed.
    let fs_rows = specs_from_fs_cached(&base);

    let mut by_name: HashMap<String, SpecRow> = HashMap::new();
    for row in fs_rows.iter() {
        by_name.insert(row.name.clone(), row.clone());
    }

    // Top-level specs only. `specs_from_fs` walks wave-plan children and emits
    // them with `parent` set from the FS nesting (a `wave-N-{role}/` subdir), so
    // dropping `parent.is_some()` rows removes the wave subdirectories that were
    // leaking into the page as if they were standalone specs. The 8 real
    // top-level specs — including epic-children like `payable` whose
    // meta.json#parent points at a sibling epic — all carry `parent == None`
    // here (FS nesting, not meta.json), so they are kept. The inline tree pulls
    // waves from the per-spec `spec_children_tree` command, not from this list.
    let mut rows: Vec<SpecRow> = by_name
        .into_values()
        .filter(|r| r.parent.is_none())
        .collect();
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

/// Process-global, per-repo cache of the [`specs_from_fs`] walk (spec
/// `performance-dashboard-rotas-lentas-cache`, wave 1). spec.md is tiny
/// markdown, but the walk opens every `spec.md` / `wave-plan.md` under
/// `.claude/spec/` — on the list route that is pure latency when nothing
/// changed. The watcher invalidates the entry on any `spec`-kind fs-change
/// (see `watcher.rs`); the dashboard's own spec.md writes invalidate inline
/// via [`invalidate_specs_cache`]. Lock discipline mirrors the events cache:
/// the lock is held only for the O(1) probe / insert, never across the walk.
static SPECS_CACHE: std::sync::LazyLock<Mutex<HashMap<String, Arc<Vec<SpecRow>>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Cached counterpart of [`specs_from_fs`]: returns the shared spec-row list
/// for `base`, walking the directory only on a miss.
#[must_use]
fn specs_from_fs_cached(base: &std::path::Path) -> Arc<Vec<SpecRow>> {
    let key = base.to_string_lossy().into_owned();
    if let Ok(guard) = SPECS_CACHE.lock() {
        if let Some(hit) = guard.get(&key) {
            return Arc::clone(hit);
        }
    }
    // Cold miss: walk OUTSIDE the lock so parallel projects never serialise.
    let rows = Arc::new(specs_from_fs(base));
    if let Ok(mut guard) = SPECS_CACHE.lock() {
        return Arc::clone(guard.entry(key).or_insert(rows));
    }
    rows
}

/// Drop `repo`'s cached spec list so the next [`specs_from_fs_cached`] re-walks
/// the directory. Called by the watcher on a `spec`-kind fs-change and by the
/// dashboard's own spec.md writers. Fail-open: a poisoned lock is a no-op (the
/// stale list is corrected on the next change).
pub(crate) fn invalidate_specs_cache(repo: &str) {
    if let Ok(mut guard) = SPECS_CACHE.lock() {
        guard.remove(repo);
    }
}

/// Aggregated push payload for the `dashboard:specs-snapshot` event (spec
/// `performance-dashboard-rotas-lentas-cache`, wave 2): the spec list plus the
/// active-pipeline projections, rebuilt on a background thread by the watcher
/// and shipped ready to render. The frontend applies it via `setQueryData`
/// instead of refetching after a mass invalidation. `Clone` because
/// `tauri::Emitter::emit` requires `Serialize + Clone`.
#[derive(Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct SpecsSnapshot {
    pub repo_path: String,
    pub specs: Vec<SpecRow>,
    pub active_pipelines: Vec<ActivePipeline>,
}

/// Build the aggregated snapshot FROM THE INCREMENTAL CACHES: the spec list
/// comes from the watcher-invalidated [`SPECS_CACHE`] and the active-pipeline
/// projections fold the per-shard parsed-events cache
/// (`telemetry::walk_ndjson_events_cached`) — with warm caches this is
/// milliseconds, never a full workspace walk. Failure-tolerant like every
/// dashboard command: an error degrades to empty sections, never a panic on
/// the watcher's rebuild path.
#[must_use]
pub(crate) fn build_specs_snapshot(repo_path: &str) -> SpecsSnapshot {
    let specs = dashboard_specs_impl(repo_path.to_string()).unwrap_or_default();
    let active_pipelines =
        dashboard_active_pipelines_impl(repo_path.to_string()).unwrap_or_default();
    SpecsSnapshot {
        repo_path: repo_path.to_string(),
        specs,
        active_pipelines,
    }
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
fn specs_from_fs(base: &std::path::Path) -> Vec<SpecRow> {
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

/// Off-main-thread wrapper for [`dashboard_spec_markdown_impl`] (small file
/// read, but part of the spec-detail fan-out — no synchronous IO on the main
/// thread). A join error surfaces as the not-found `Err` the viewer already
/// maps to its "not available" state.
#[tauri::command]
async fn dashboard_spec_markdown(repo_path: String, spec_name: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || dashboard_spec_markdown_impl(repo_path, spec_name))
        .await
        .unwrap_or_else(|_| Err("spec markdown read failed".to_string()))
}

fn dashboard_spec_markdown_impl(repo_path: String, spec_name: String) -> Result<String, String> {
    let base = PathBuf::from(&repo_path).join(".claude").join("spec");
    // Reject traversal — spec_name is a single directory name, not a path.
    if spec_name.is_empty()
        || spec_name.contains('/')
        || spec_name.contains('\\')
        || spec_name.contains("..")
    {
        // Phase artifacts materialized by `apps/rt` live one level under the
        // spec dir at a fixed relative path: `{spec}/qa/report.md` and
        // `{spec}/review/verdict.md`. The viewer requests them with a composite
        // token (`{spec}/qa/report.md`); accept exactly those two suffixes after
        // an otherwise-valid spec segment, resolving relative to the spec dir
        // just like `spec.md`/`wave-plan.md` (cases 1/3). Anything else stays
        // rejected. Fail-open: a missing artifact returns `Err`, which the
        // viewer maps to its "not available" state — never an error toast.
        if let Some(rel) = ["qa/report.md", "review/verdict.md"]
            .into_iter()
            .find(|suffix| spec_name.ends_with(suffix))
        {
            let parent = &spec_name[..spec_name.len() - rel.len()];
            let parent = parent.strip_suffix('/').unwrap_or(parent);
            if !parent.is_empty()
                && !parent.contains('/')
                && !parent.contains('\\')
                && !parent.contains("..")
            {
                let mut artifact = base.join(parent);
                for seg in rel.split('/') {
                    artifact = artifact.join(seg);
                }
                if artifact.exists() {
                    return fs::read_to_string(&artifact).map_err(|e| e.to_string());
                }
                return Err(format!("spec markdown not found: {}", spec_name));
            }
        }
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

/// Emit `pipeline.status: <to>` via the per-spec NDJSON sink. Wave 6A of
/// [[2026-05-26-no-sqlite-git-source-of-truth]] retired the SQLite event
/// store; per-spec `.events/*.ndjson` files are now the canonical hot path.
/// Fail-open.
fn lib_emit_pipeline_status(repo_path: &str, spec: &str, to: &str) {
    let payload = serde_json::json!({ "from": serde_json::Value::Null, "to": to });
    lib_emit_ndjson(repo_path, spec, "pipeline.status", payload);
}

/// Append one event line to `.claude/spec/{spec}/.events/dashboard.ndjson`.
/// Reused by [`lib_emit_pipeline_status`], `spec_views::emit_pipeline_status`,
/// and `spec_views::emit_pipeline_removed`. Each line is a self-contained
/// JSON object — schema mirrors the `EventReader` lenient model
/// (`kind`, `payload`, optional metadata).
///
/// Fail-open: every IO error degrades to an `eprintln!` + return — emitting
/// telemetry must never block a user-facing Tauri command.
pub(crate) fn lib_emit_ndjson(
    repo_path: &str,
    spec: &str,
    kind: &str,
    payload: serde_json::Value,
) {
    use std::io::Write;
    let events_dir = std::path::Path::new(repo_path)
        .join(".claude")
        .join("spec")
        .join(spec)
        .join(".events");
    if let Err(e) = std::fs::create_dir_all(&events_dir) {
        eprintln!(
            "lib_emit_ndjson: create_dir {} failed: {e}",
            events_dir.display()
        );
        return;
    }
    let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    // `kind` here is the harness event NAME (e.g. "pipeline.status"). Emit it in
    // BOTH `event` and `kind` so the record is symmetric with CLI-emitted records:
    // `read_workspace_events`/`ndjson_to_harness` keys on `event`, while the
    // `walk_ndjson_events` readers fall back to `kind`. Writing only `kind` left
    // dashboard-emitted status events invisible to the history/criteria views.
    let line = serde_json::json!({
        "ts": ts,
        "event": kind,
        "kind": kind,
        "spec": spec,
        "actor": { "kind": "cli", "id": "dashboard" },
        "payload": payload,
    });
    let serialized = match serde_json::to_string(&line) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("lib_emit_ndjson: serialize failed: {e}");
            return;
        }
    };
    let path = events_dir.join("dashboard.ndjson");
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut file) => {
            if let Err(e) = writeln!(file, "{serialized}") {
                eprintln!("lib_emit_ndjson: write {} failed: {e}", path.display());
            }
            // Surgical cache maintenance: mark exactly this shard dirty so the
            // next read reflects the emit immediately, without waiting on the
            // watcher's debounce (and without a full re-parse).
            telemetry::invalidate_events_cache_path(repo_path, &path);
        }
        Err(e) => {
            eprintln!("lib_emit_ndjson: open {} failed: {e}", path.display());
        }
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
        return;
    }
    // The cached spec list parses this very header — drop it inline so the
    // status flip is visible on the next list call, ahead of the watcher.
    invalidate_specs_cache(repo_path);
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

/// Off-main-thread wrapper for [`dashboard_search_events_impl`] (full workspace
/// walk + sort + filter). A join error degrades to an empty result. See
/// [`dashboard_metrics`] for the rationale.
#[tauri::command]
async fn dashboard_search_events(
    repo_path: String,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<RecentEvent>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        dashboard_search_events_impl(repo_path, query, limit)
    })
    .await
    .unwrap_or_else(|_| Ok(Vec::new()))
}

fn dashboard_search_events_impl(repo_path: String, query: String, limit: Option<usize>) -> Result<Vec<RecentEvent>, String> {
    // Onda 2: case-insensitive substring filter over the same complete-walker
    // fold as `dashboard_recent_events`. Matches against the serialized record
    // (event name, spec, summary, tool, target). Newest first.
    let base = PathBuf::from(&repo_path);
    let needle = query.trim().to_lowercase();
    let events = telemetry::walk_ndjson_events_cached(&base);
    let timeline = telemetry::build_session_spec_timeline_from(&events);
    let mut ordered: Vec<&serde_json::Value> = events.iter().collect();
    ordered.sort_by(|a, b| {
        let ta = a.get("ts").and_then(|t| t.as_str()).unwrap_or("");
        let tb = b.get("ts").and_then(|t| t.as_str()).unwrap_or("");
        tb.cmp(ta)
    });
    let cap = limit.unwrap_or(100).min(2000);
    let rows: Vec<RecentEvent> = ordered
        .into_iter()
        .map(|v| recent_event_from_value_attributed(v, Some(&timeline)))
        .filter(|r| {
            if needle.is_empty() {
                return true;
            }
            let hay = format!(
                "{} {} {} {} {}",
                r.event_type,
                r.spec.as_deref().unwrap_or(""),
                r.summary.as_deref().unwrap_or(""),
                r.tool_name.as_deref().unwrap_or(""),
                r.target.as_deref().unwrap_or(""),
            )
            .to_lowercase();
            hay.contains(&needle)
        })
        .take(cap)
        .collect();
    Ok(rows)
}

#[tauri::command]
fn dashboard_search_knowledge(repo_path: String, query: String, limit: Option<usize>) -> Result<Vec<KnowledgeRow>, String> {
    // §5: substring search over the on-disk `.claude/knowledge/` projection.
    let needle = query.trim().to_lowercase();
    let rows: Vec<KnowledgeRow> = read_knowledge_rows(&PathBuf::from(&repo_path))
        .into_iter()
        .filter(|r| {
            if needle.is_empty() {
                return true;
            }
            format!("{} {} {} {}", r.type_, r.name, r.description, r.source.as_deref().unwrap_or(""))
                .to_lowercase()
                .contains(&needle)
        })
        .take(limit.unwrap_or(100).min(1000))
        .collect();
    Ok(rows)
}

/// Off-main-thread wrapper for [`dashboard_activity_aggregated_impl`] (full
/// workspace walk + group fold). A join error degrades to an empty result. See
/// [`dashboard_metrics`] for the rationale.
#[tauri::command]
async fn dashboard_activity_aggregated(
    repo_path: String,
    limit: Option<usize>,
) -> Result<Vec<ActivityGroup>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        dashboard_activity_aggregated_impl(repo_path, limit)
    })
    .await
    .unwrap_or_else(|_| Ok(Vec::new()))
}

fn dashboard_activity_aggregated_impl(repo_path: String, limit: Option<usize>) -> Result<Vec<ActivityGroup>, String> {
    // Onda 2: group `tool.use` / `agent.*` events by (spec, wave, action_kind)
    // over the complete walker. action_kind = tool name for `tool.use`, the
    // agent subtype for `agent.*`. Tracks count, min/max ts, token sums, and a
    // distinct-file count (from `tool.use` target.file_path).
    let base = PathBuf::from(&repo_path);
    let events = telemetry::walk_ndjson_events_cached(&base);
    // Read-time attribution so spec-less session `tool.use` / `agent.*` events
    // group under the spec their session was bound to at the time.
    let timeline = telemetry::build_session_spec_timeline_from(&events);

    struct Acc {
        spec: Option<String>,
        wave: Option<i64>,
        action_kind: Option<String>,
        count: i64,
        min_ts: Option<String>,
        max_ts: Option<String>,
        tokens_total: i64,
        files: std::collections::HashSet<String>,
    }
    let mut groups: HashMap<(String, i64, String), Acc> = HashMap::new();

    for v in events.iter() {
        let name = telemetry::event_name_of(v);
        let payload = v.get("payload");
        let action_kind: Option<String> = match name {
            "tool.use" => payload
                .and_then(|p| p.get("tool").or_else(|| p.get("tool_name")))
                .and_then(|t| t.as_str())
                .map(str::to_string),
            n if n.starts_with("agent.") => payload
                .and_then(|p| p.get("subagentType").or_else(|| p.get("agent_type")))
                .and_then(|t| t.as_str())
                .map(str::to_string)
                .or_else(|| Some(n.to_string())),
            _ => continue,
        };
        let spec = timeline.attributed_spec(v).map(str::to_string);
        let wave = v.get("wave").and_then(serde_json::Value::as_i64);
        let ts = v.get("ts").and_then(|t| t.as_str()).map(str::to_string);
        let tokens = v
            .get("tokens_in")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0)
            + v.get("tokens_out").and_then(serde_json::Value::as_i64).unwrap_or(0);
        let file = if name == "tool.use" {
            payload
                .and_then(|p| p.get("target"))
                .and_then(|t| t.as_object())
                .and_then(|o| o.get("file_path").or_else(|| o.get("file")))
                .and_then(|x| x.as_str())
                .map(str::to_string)
        } else {
            None
        };

        let key = (
            spec.clone().unwrap_or_default(),
            wave.unwrap_or(-1),
            action_kind.clone().unwrap_or_default(),
        );
        let entry = groups.entry(key).or_insert_with(|| Acc {
            spec: spec.clone(),
            wave,
            action_kind: action_kind.clone(),
            count: 0,
            min_ts: None,
            max_ts: None,
            tokens_total: 0,
            files: std::collections::HashSet::new(),
        });
        entry.count += 1;
        entry.tokens_total += tokens;
        if let Some(f) = file {
            entry.files.insert(f);
        }
        if let Some(t) = ts {
            if entry.min_ts.as_deref().map_or(true, |c| t.as_str() < c) {
                entry.min_ts = Some(t.clone());
            }
            if entry.max_ts.as_deref().map_or(true, |c| t.as_str() > c) {
                entry.max_ts = Some(t);
            }
        }
    }

    let mut rows: Vec<ActivityGroup> = groups
        .into_values()
        .map(|a| ActivityGroup {
            spec: a.spec,
            wave: a.wave,
            action_kind: a.action_kind,
            count: a.count,
            min_ts: a.min_ts,
            max_ts: a.max_ts,
            tokens_total: a.tokens_total,
            files_touched: i64::try_from(a.files.len()).unwrap_or(i64::MAX),
        })
        .collect();
    // Most-active groups first, then most-recent.
    rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| b.max_ts.cmp(&a.max_ts)));
    if let Some(n) = limit {
        rows.truncate(n);
    }
    Ok(rows)
}

/// Off-main-thread wrapper for [`dashboard_quality_metrics_impl`] (full
/// workspace fold). A join error degrades to a zeroed summary. See
/// [`dashboard_metrics`] for the rationale.
#[tauri::command]
async fn dashboard_quality_metrics(repo_path: String) -> Result<QualityMetrics, String> {
    tauri::async_runtime::spawn_blocking(move || dashboard_quality_metrics_impl(repo_path))
        .await
        .unwrap_or_else(|_| Ok(QualityMetrics::default()))
}

fn dashboard_quality_metrics_impl(repo_path: String) -> Result<QualityMetrics, String> {
    // Onda 2: derive quality from `review.result` / `qa.result` events folded
    // per spec via the core `project_quality` projection. pass@1 = share of
    // specs whose latest QA had zero fails; fix_loop_rate = share of specs with
    // ≥2 distinct qa.result runs (a re-run implies a fix loop). by_role is keyed
    // by spec here (no role dimension on qa events). Honest about thin data:
    // returns zeros when no review/qa events exist.
    let base = PathBuf::from(&repo_path);
    let events = telemetry::workspace_harness_events_cached(&base);

    // Distinct specs that emitted any qa.result.
    let mut qa_specs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut qa_runs_per_spec: HashMap<String, i64> = HashMap::new();
    for e in events.iter() {
        if e.event == "qa.result" {
            if let Some(spec) = e.spec.as_deref() {
                qa_specs.insert(spec.to_string());
                *qa_runs_per_spec.entry(spec.to_string()).or_insert(0) += 1;
            }
        }
    }

    let mut by_role: Vec<RoleQuality> = Vec::new();
    let mut pass_specs = 0i64;
    let mut fix_loop_specs = 0i64;
    for spec in &qa_specs {
        let rollup = mustard_core::view::projection::project_quality(spec, &events);
        let runs = *qa_runs_per_spec.get(spec).unwrap_or(&0);
        let passed_at_1 = if rollup.total > 0 && rollup.failed == 0 { 1.0 } else { 0.0 };
        if passed_at_1 > 0.0 {
            pass_specs += 1;
        }
        if runs >= 2 {
            fix_loop_specs += 1;
        }
        by_role.push(RoleQuality {
            role: spec.clone(),
            pass_at_1: passed_at_1,
            fix_loops: runs.saturating_sub(1),
            samples: runs,
        });
    }
    let spec_n = qa_specs.len() as f64;
    let pass_at_1 = if spec_n > 0.0 { pass_specs as f64 / spec_n } else { 0.0 };
    let fix_loop_rate = if spec_n > 0.0 { fix_loop_specs as f64 / spec_n } else { 0.0 };

    // avg phase duration: mean of completed wave durations across specs.
    let mut wave_durations: Vec<i64> = Vec::new();
    for spec in &qa_specs {
        for w in mustard_core::view::projection::project_waves(spec, &events) {
            if let Some(d) = w.duration_ms {
                if d >= 0 {
                    wave_durations.push(d);
                }
            }
        }
    }
    let avg_phase_duration_ms = if wave_durations.is_empty() {
        0.0
    } else {
        wave_durations.iter().sum::<i64>() as f64 / wave_durations.len() as f64
    };

    by_role.sort_by(|a, b| a.role.cmp(&b.role));
    Ok(QualityMetrics {
        pass_at_1,
        fix_loop_rate,
        avg_phase_duration_ms,
        by_role,
        slowest_waves: Vec::new(),
        tokens_by_phase: Vec::new(),
    })
}

#[tauri::command]
fn dashboard_knowledge_browse(repo_path: String, limit: Option<usize>) -> Result<Vec<KnowledgeRow>, String> {
    // §5: project the on-disk `.claude/knowledge/` files, newest first.
    let rows: Vec<KnowledgeRow> = read_knowledge_rows(&PathBuf::from(&repo_path))
        .into_iter()
        .take(limit.unwrap_or(200).min(2000))
        .collect();
    Ok(rows)
}

/// Off-main-thread wrapper for [`dashboard_telemetry_impl`] (several full spec
/// walks + an `rtk gain` subprocess). The heaviest single command — running it
/// on the main thread is the dominant freeze. A join error degrades to a zeroed
/// summary. See [`dashboard_metrics`] for the rationale.
#[tauri::command]
async fn dashboard_telemetry(repo_path: String) -> Result<telemetry::TelemetrySummary, String> {
    tauri::async_runtime::spawn_blocking(move || dashboard_telemetry_impl(repo_path))
        .await
        .unwrap_or_else(|_| Ok(telemetry::TelemetrySummary::default()))
}

fn dashboard_telemetry_impl(repo_path: String) -> Result<telemetry::TelemetrySummary, String> {
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
/// Off-main-thread: cheap when warm, but pays a cold disk read on the main
/// thread otherwise. A join error degrades to an empty list.
#[tauri::command]
async fn dashboard_friction(repo_path: String) -> Result<Vec<telemetry::FrictionEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let base = std::path::PathBuf::from(&repo_path);
        Ok(telemetry::friction_entries(&base))
    })
    .await
    .unwrap_or_else(|_| Ok(Vec::new()))
}

/// Live activity derived from mustard.db. Events are written by mustard-rt
/// on every hook dispatch, so the DB always reflects the current session.
/// Off-main-thread; a join error degrades to a zeroed summary.
#[tauri::command]
async fn dashboard_live_activity(repo_path: String) -> Result<telemetry::LiveActivity, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let base = std::path::PathBuf::from(&repo_path);
        Ok(telemetry::live_activity(&base))
    })
    .await
    .unwrap_or_else(|_| Ok(telemetry::LiveActivity::default()))
}

/// Build a [`ConsumptionSummary`] for one project root from the core NDJSON
/// economy readers. Reuses `economy_summary` (cost / total tokens / saved),
/// `metric_token_summary` (per-model token + cost split) and `per_spec_costs`
/// (top specs). `tokens_today` is recomputed from the measured token metric
/// channel filtered to the UTC day. Fail-open: every reader degrades to its
/// default on IO error, so a project with no events yields an all-zero summary.
fn consumption_for_root(root: &std::path::Path) -> ConsumptionSummary {
    use mustard_core::domain::economy::scope::ProjectPath as CoreProjectPath;
    use mustard_core::domain::economy::EconomyScope as CoreScope;

    let scope = CoreScope::Project(CoreProjectPath::new(root));
    let summary = mustard_core::domain::economy::economy_summary(root, scope.clone())
        .unwrap_or_default();
    let tokens = mustard_core::domain::economy::metric_token_summary(root, scope.clone())
        .unwrap_or_default();
    let spec_costs = mustard_core::domain::economy::per_spec_costs(root, scope)
        .unwrap_or_default();

    let tokens_total = u64::try_from(summary.total_tokens).unwrap_or(0);
    let cost_total_usd = summary.total_cost_usd_micros as f64 / 1_000_000.0;

    // Per-model rows: token split from the metric channel; cost is the
    // economy-summary total apportioned by token share (the metric channel has
    // no per-model cost, only per-model tokens).
    let by_model_total: i64 = tokens.by_model.iter().map(|b| b.input_tokens + b.output_tokens).sum();
    let by_model: Vec<ModelUsage> = tokens
        .by_model
        .iter()
        .map(|b| {
            let total = b.input_tokens + b.output_tokens;
            let pct = if by_model_total > 0 { total as f64 / by_model_total as f64 } else { 0.0 };
            ModelUsage {
                model: b.model.clone(),
                calls: u64::try_from(b.datapoint_count).unwrap_or(0),
                input_tokens: u64::try_from(b.input_tokens).unwrap_or(0),
                output_tokens: u64::try_from(b.output_tokens).unwrap_or(0),
                total_tokens: u64::try_from(total).unwrap_or(0),
                cost_usd: cost_total_usd * pct,
                pct_tokens: pct,
            }
        })
        .collect();

    let top_specs: Vec<SpecUsage> = spec_costs
        .iter()
        .take(10)
        .map(|s| SpecUsage {
            spec: s.spec_id.0.clone(),
            calls: u64::try_from(s.span_count).unwrap_or(0),
            total_tokens: u64::try_from(s.tokens).unwrap_or(0),
            cost_usd: s.cost_usd_micros as f64 / 1_000_000.0,
        })
        .collect();

    // tokens_today: re-fold the measured token metric for the UTC day.
    let (tokens_today, cost_today_usd) = consumption_today(root);

    ConsumptionSummary {
        tokens_total,
        tokens_today,
        cost_total_usd,
        cost_today_usd,
        by_model,
        by_agent_type: Vec::new(),
        top_specs,
        daily_series: Vec::new(),
    }
}

/// Today's measured token + cost totals for one root, folded from the OTEL
/// metric channel (`pipeline.telemetry.metric`: `claude_code.token.usage` for
/// tokens, `claude_code.cost.usage` for USD) filtered to the UTC day prefix.
fn consumption_today(root: &std::path::Path) -> (u64, f64) {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let events = telemetry::walk_ndjson_events_cached(root);
    let mut tokens = 0u64;
    let mut cost = 0.0f64;
    for v in events.iter() {
        if telemetry::event_name_of(v) != "pipeline.telemetry.metric" {
            continue;
        }
        let ts = v.get("ts").and_then(|t| t.as_str()).unwrap_or("");
        if !ts.starts_with(&today) {
            continue;
        }
        let payload = v.get("payload");
        let metric = payload.and_then(|p| p.get("metric")).and_then(|m| m.as_str()).unwrap_or("");
        let sum = payload.and_then(|p| p.get("sum")).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        match metric {
            "claude_code.token.usage" => tokens += sum as u64,
            "claude_code.cost.usage" => cost += sum,
            _ => {}
        }
    }
    (tokens, cost)
}

/// Per-workspace consumption + cost summary, folded from the NDJSON economy
/// channel. Returns zeros when no economy events exist for the project.
/// Off-main-thread (cold cache pays a full workspace parse); a join error
/// degrades to a zeroed summary.
#[tauri::command]
async fn dashboard_consumption(repo_path: String) -> Result<ConsumptionSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(consumption_for_root(&PathBuf::from(&repo_path)))
    })
    .await
    .unwrap_or_else(|_| Ok(ConsumptionSummary::default()))
}

/// Cross-project (global) consumption: walks every project discovered under
/// `projects_root`, sums tokens and cost per project + per model, builds a
/// merged 14-day daily series, and attaches the global RTK block.
#[tauri::command]
fn dashboard_consumption_global(projects_root: String) -> Result<GlobalConsumption, String> {
    let root = std::path::PathBuf::from(&projects_root);
    let projects = discovery::discover(&root)?;

    let mut out = GlobalConsumption::default();
    let mut model_totals: HashMap<String, ModelUsage> = HashMap::new();

    for p in projects {
        // Onda 2: each discovered project contributes a real NDJSON-folded
        // consumption row via the same `consumption_for_root` the per-project
        // command uses. Global token/cost totals and `by_model` accumulate
        // across every project.
        let c = consumption_for_root(std::path::Path::new(&p.path));
        out.tokens_total += c.tokens_total;
        out.tokens_today += c.tokens_today;
        out.cost_total_usd += c.cost_total_usd;
        out.cost_today_usd += c.cost_today_usd;
        for m in &c.by_model {
            let entry = model_totals.entry(m.model.clone()).or_insert_with(|| ModelUsage {
                model: m.model.clone(),
                ..ModelUsage::default()
            });
            entry.calls += m.calls;
            entry.input_tokens += m.input_tokens;
            entry.output_tokens += m.output_tokens;
            entry.total_tokens += m.total_tokens;
            entry.cost_usd += m.cost_usd;
        }
        let row = ProjectUsage {
            id: p.id.clone(),
            name: p.name.clone(),
            path: p.path.clone(),
            tokens_total: c.tokens_total,
            tokens_today: c.tokens_today,
            cost_total_usd: c.cost_total_usd,
            cost_today_usd: c.cost_today_usd,
            last_activity_ms: p.last_activity_ms,
        };
        out.by_project.push(row);
    }

    // Finalise per-model token-share once the global total is known.
    let global_tokens: u64 = model_totals.values().map(|m| m.total_tokens).sum();
    let mut by_model: Vec<ModelUsage> = model_totals.into_values().collect();
    for m in &mut by_model {
        m.pct_tokens = if global_tokens > 0 {
            m.total_tokens as f64 / global_tokens as f64
        } else {
            0.0
        };
    }
    by_model.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
    out.by_model = by_model;

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

// `Clone` because the watcher ships this inside the `dashboard:specs-snapshot`
// push payload and `tauri::Emitter::emit` requires `Serialize + Clone`.
#[derive(Serialize, Clone)]
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

/// Off-main-thread wrapper for [`dashboard_active_pipelines_impl`] (per-spec
/// card folds over the whole workspace). A join error degrades to an empty
/// list. See [`dashboard_metrics`] for the rationale.
#[tauri::command]
async fn dashboard_active_pipelines(repo_path: String) -> Result<Vec<ActivePipeline>, String> {
    tauri::async_runtime::spawn_blocking(move || dashboard_active_pipelines_impl(repo_path))
        .await
        .unwrap_or_else(|_| Ok(Vec::new()))
}

/// Shared core for every "card per listed spec" command: discover the
/// top-level spec names from the filesystem (via the watcher-invalidated specs
/// cache), build the attributed per-spec activity counts ONCE for the whole
/// workspace (folds spec-less session `tool.use`/`agent.*` onto the spec their
/// session was bound to — otherwise each row would re-walk the event log), and
/// map each spec through `spec_card_v2_with_counts`. A spec with no event
/// evidence (or a per-row projection error) yields `(name, None)` so each
/// caller picks its own degradation: the batch list substitutes the
/// "no-events" card, the active-pipelines list skips the row.
fn workspace_spec_cards(repo_path: &str) -> Vec<(String, Option<spec_views::SpecCard>)> {
    let base = PathBuf::from(repo_path);
    let mut names: Vec<String> = specs_from_fs_cached(&base)
        .iter()
        .filter(|r| r.parent.is_none()) // top-level specs only; waves nest inside
        .map(|r| r.name.clone())
        .collect();
    names.sort();
    names.dedup();

    let counts = telemetry::attributed_spec_counts(&base);

    names
        .into_iter()
        .map(|spec| {
            let card = spec_views::spec_card_v2_with_counts(repo_path, &spec, &counts)
                .ok()
                .flatten();
            (spec, card)
        })
        .collect()
}

fn dashboard_active_pipelines_impl(repo_path: String) -> Result<Vec<ActivePipeline>, String> {
    // Onda 2 (HIGH-VALUE): fold the NDJSON workspace once, then per discovered
    // spec build a SpecCard via the same `spec_card_v2` primitive the spec page
    // uses. Specs in a terminal status (completed / cancelled / closed-followup)
    // are dropped — the "PIPELINES ATIVOS" card lists only live work.
    let mut out: Vec<ActivePipeline> = Vec::new();
    for (_, card) in workspace_spec_cards(&repo_path) {
        let Some(card) = card else {
            continue; // no event evidence → not an active pipeline
        };
        if is_terminal_pipeline_status(&card.status) {
            continue;
        }
        out.push(ActivePipeline {
            spec_name: card.spec,
            status: card.status,
            phase: card.phase,
            current_wave: card.current_wave.and_then(|w| u32::try_from(w).ok()),
            total_waves: card.total_waves.and_then(|w| u32::try_from(w).ok()),
            model: card.model,
            has_dispatch_failure: false,
            failure_age_ms: None,
            tasks_pending: 0,
            tasks_in_progress: 0,
            tasks_completed: 0,
            updated_at: card.last_event_at,
        });
    }
    // Most-recently-active first.
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(out)
}

/// Spec statuses that mean "finished / parked" — excluded from the active
/// pipelines list. Mirrors `spec_views::is_terminal_status` (kept local to
/// avoid widening that module's privacy boundary).
fn is_terminal_pipeline_status(status: &str) -> bool {
    matches!(
        status,
        "completed" | "closed-followup" | "cancelled" | "no-events"
    )
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
/// Reads the repo model's known entity names (declaration names) from
/// `.claude/grain.model.json` **via the scan tool** — the dashboard never parses
/// the model directly. Used by the PRD lapidator's EntityPicker.
///
/// Returns an empty list when the model is missing (project not scanned yet) or
/// the scan tool is unavailable, so the UI never crashes. `read_entity_names`
/// is fail-open by construction.
#[tauri::command]
fn read_model_entities(repo_path: String) -> Result<Vec<String>, String> {
    let model = PathBuf::from(&repo_path)
        .join(".claude")
        .join("grain.model.json");
    Ok(mustard_core::read_entity_names(&model))
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
/// Off-main-thread wrapper for [`dashboard_spec_card_impl`]. The spec-detail
/// route fans 5 commands out in parallel; all of them now read the cached
/// workspace slice, but the cold rebuild is still a full parse — keep it off
/// the main thread (see [`dashboard_metrics`]). A join error degrades to the
/// "no-events" card (the failure-tolerant contract).
#[tauri::command]
async fn dashboard_spec_card(repo_path: String, spec: String) -> Result<spec_views::SpecCard, String> {
    let fallback_spec = spec.clone();
    tauri::async_runtime::spawn_blocking(move || dashboard_spec_card_impl(repo_path, spec))
        .await
        .unwrap_or_else(|_| Ok(no_events_spec_card(fallback_spec)))
}

fn dashboard_spec_card_impl(repo_path: String, spec: String) -> Result<spec_views::SpecCard, String> {
    match spec_views::spec_card_v2(&repo_path, &spec)? {
        Some(card) => Ok(card),
        None => Ok(no_events_spec_card(spec)),
    }
}

/// The empty-state card for a spec with no event evidence — also the join-error
/// degradation of the async wrapper (never an `Err` toast).
fn no_events_spec_card(spec: String) -> spec_views::SpecCard {
    spec_views::SpecCard {
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
        digest_used: false,
        source_reads_before_digest: 0,
    }
}

/// Batch counterpart of [`dashboard_spec_card`] for the Specs LIST route
/// (spec `sidebar-lento-lista-specs-dispara`): one command returns a card for
/// EVERY listed top-level spec, paying a single `attributed_spec_counts`
/// workspace fold instead of one per row — the page used to fan out N
/// `dashboard_spec_card` calls, each re-folding the whole event slice. A join
/// error degrades to an empty list (the failure-tolerant contract).
#[tauri::command]
async fn dashboard_spec_cards(repo_path: String) -> Result<Vec<spec_views::SpecCard>, String> {
    tauri::async_runtime::spawn_blocking(move || dashboard_spec_cards_impl(repo_path))
        .await
        .unwrap_or_else(|_| Ok(Vec::new()))
}

fn dashboard_spec_cards_impl(repo_path: String) -> Result<Vec<spec_views::SpecCard>, String> {
    // Parity with the per-spec command: a listed spec with no event evidence
    // still gets a card (the "no-events" empty state), so the list renders
    // every spec exactly as the old fan-out did.
    Ok(workspace_spec_cards(&repo_path)
        .into_iter()
        .map(|(spec, card)| card.unwrap_or_else(|| no_events_spec_card(spec)))
        .collect())
}

/// Off-main-thread + cached-slice wrapper (see [`dashboard_spec_card`]). A
/// join error degrades to an empty list.
#[tauri::command]
async fn dashboard_spec_waves(repo_path: String, spec: String) -> Result<Vec<spec_views::SpecWave>, String> {
    tauri::async_runtime::spawn_blocking(move || spec_views::spec_waves_v2(&repo_path, &spec))
        .await
        .unwrap_or_else(|_| Ok(Vec::new()))
}

/// Wave 3 (spec `checklist-progresso-por-onda`) — per-wave checklist progress
/// (`done`/`total`) folded from the `meta.json#checklist` sidecars plus the
/// `checklist.item.marked` NDJSON events. Fail-open: a spec with no checklist
/// data resolves to an empty vec so the frontend renders nothing rather than
/// a fabricated `0/0`.
#[tauri::command]
async fn dashboard_spec_checklist_progress(
    repo_path: String,
    spec: String,
) -> Result<Vec<spec_views::WaveChecklistProgress>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        spec_views::spec_checklist_progress_v2(&repo_path, &spec)
    })
    .await
    .unwrap_or_else(|_| Ok(Vec::new()))
}

#[tauri::command]
async fn dashboard_spec_quality(repo_path: String, spec: String) -> Result<Vec<spec_views::SpecQualityItem>, String> {
    tauri::async_runtime::spawn_blocking(move || spec_views::spec_quality_v2(&repo_path, &spec))
        .await
        .unwrap_or_else(|_| Ok(Vec::new()))
}

#[tauri::command]
async fn dashboard_spec_timeline(repo_path: String, spec: String) -> Result<Vec<spec_views::SpecTimelineNode>, String> {
    tauri::async_runtime::spawn_blocking(move || spec_views::spec_timeline_v2(&repo_path, &spec))
        .await
        .unwrap_or_else(|_| Ok(Vec::new()))
}

/// Off-main-thread wrapper for [`dashboard_spec_events_impl`]. A join error
/// degrades to an empty list.
#[tauri::command]
async fn dashboard_spec_events(
    repo_path: String,
    spec: String,
    filter: Option<spec_views::EventFilter>,
) -> Result<Vec<spec_views::TimelineEvent>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        dashboard_spec_events_impl(repo_path, spec, filter)
    })
    .await
    .unwrap_or_else(|_| Ok(Vec::new()))
}

fn dashboard_spec_events_impl(
    repo_path: String,
    spec: String,
    filter: Option<spec_views::EventFilter>,
) -> Result<Vec<spec_views::TimelineEvent>, String> {
    // Onda 2: per-spec event timeline from the same projection that backs
    // `dashboard_spec_timeline` (`spec_timeline_v2`), reshaped into the
    // `TimelineEvent` row the events panel consumes, with optional client-style
    // filtering (kinds / wave / substring) applied server-side.
    let nodes = spec_views::spec_timeline_v2(&repo_path, &spec)?;
    let f = filter.unwrap_or_default();
    let mut rows: Vec<spec_views::TimelineEvent> = Vec::new();
    for (i, n) in nodes.into_iter().enumerate() {
        if let Some(kinds) = &f.kinds {
            if !kinds.is_empty() && !kinds.iter().any(|k| k.eq_ignore_ascii_case(&n.kind)) {
                continue;
            }
        }
        if let Some(w) = f.wave {
            if n.wave != Some(w) {
                continue;
            }
        }
        if let Some(q) = &f.q {
            let q = q.trim().to_lowercase();
            if !q.is_empty() {
                let hay = format!(
                    "{} {} {}",
                    n.label,
                    n.payload_summary.as_deref().unwrap_or(""),
                    n.kind
                )
                .to_lowercase();
                if !hay.contains(&q) {
                    continue;
                }
            }
        }
        rows.push(spec_views::TimelineEvent {
            id: format!("{spec}-{i}"),
            ts: n.ts,
            phase: n.phase,
            spec: Some(spec.clone()),
            agent: if n.kind == "agent" { Some(n.label.clone()) } else { None },
            summary: n.payload_summary.unwrap_or(n.label),
        });
    }
    Ok(rows)
}

#[tauri::command]
fn dashboard_spec_action(repo_path: String, spec: String, action: String) -> Result<spec_views::SpecAction, String> {
    // Onda 2: actually perform the verb over the NDJSON sink via the same
    // `lib_emit_pipeline_status` the live `dashboard_spec_complete` / `_cancel`
    // / `_reactivate` commands use, instead of returning the error fallback.
    //   reopen → implementing   close → completed   remove → cancelled
    // The `### Status:` header is kept in sync for the FS-walk fallback.
    let verb = action.to_lowercase();
    let to = match verb.as_str() {
        "reopen" => "implementing",
        "close" => "completed",
        "remove" => "cancelled",
        other => return Err(format!("unknown action: {other}")),
    };
    lib_emit_pipeline_status(&repo_path, &spec, to);
    lib_sync_spec_status_header(&repo_path, &spec, to);
    Ok(spec_views::SpecAction {
        action,
        spec,
        result: "ok".to_string(),
        message: Some(to.to_string()),
    })
}

/// Wave-3 (2026-05-20, spec `2026-05-20-tactical-fix-via-sub-spec`) — list
/// sub-specs linked to `parent` via `spec.link` events. Delegates to
/// `spec_views::spec_children_v2`, which now spawns `mustard-rt run
/// spec-children` (the cross-developer UNION of events + `### Parent:`
/// headers — see W4A migration notes in `spec_views.rs`).
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
/// Off-main-thread wrapper (full workspace fold; cached slice when warm). A
/// join error degrades to the default summary.
#[tauri::command]
async fn dashboard_workspace_summary(repo_path: String) -> Result<spec_views::WorkspaceSummary, String> {
    tauri::async_runtime::spawn_blocking(move || spec_views::workspace_summary_v2(&repo_path))
        .await
        .unwrap_or_else(|_| Ok(spec_views::WorkspaceSummary::default()))
}

// ── Wave-6 hygiene observability ─────────────────────────────────────────────

/// Onda 2 (§5): honest hygiene health roll-up. There is no fabricated health
/// score — every field is a real count:
///   * `active`            — discovered specs whose latest projected status is
///                           non-terminal (the FS spec walk ∩ `spec_card_v2`).
///   * `suspects`          — distinct active specs with a `hygiene.detected`
///                           event in the last 7 days (sparse in practice).
///   * `autoclose_today`   — `hygiene.autoclose` events in the last 24h.
///   * `blocked` / `wave_failed` / `followup_open` — these qualifiers live in
///     spec `meta.json` flags, which are not folded here; left at 0 honestly
///     rather than guessed. `last_hygiene_run_at` is the max `hygiene.*` ts.
#[tauri::command]
async fn workspace_health(repo_path: String) -> spec_views::WorkspaceHealth {
    tauri::async_runtime::spawn_blocking(move || workspace_health_impl(repo_path))
        .await
        .unwrap_or_default()
}

fn workspace_health_impl(repo_path: String) -> spec_views::WorkspaceHealth {
    let base = PathBuf::from(&repo_path);

    // Active specs: FS-discovered top-level specs whose projection is non-terminal.
    let mut active = 0i64;
    let mut active_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for row in specs_from_fs_cached(&base).iter() {
        if row.parent.is_some() {
            continue;
        }
        if let Ok(Some(card)) = spec_views::spec_card_v2(&repo_path, &row.name) {
            if !is_terminal_pipeline_status(&card.status) {
                active += 1;
                active_names.insert(row.name.clone());
            }
        }
    }

    // Hygiene signals from the NDJSON stream.
    let now_ms = chrono::Utc::now().timestamp_millis();
    const DAY_MS: i64 = 24 * 60 * 60 * 1000;
    let events = telemetry::walk_ndjson_events_cached(&base);
    let mut suspect_specs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut autoclose_today = 0i64;
    let mut last_hygiene_run_at: Option<String> = None;
    for v in events.iter() {
        let name = telemetry::event_name_of(v);
        if !name.starts_with("hygiene.") {
            continue;
        }
        let ts = v.get("ts").and_then(|t| t.as_str()).unwrap_or("");
        if !ts.is_empty()
            && last_hygiene_run_at.as_deref().map_or(true, |c| ts > c)
        {
            last_hygiene_run_at = Some(ts.to_string());
        }
        let age = telemetry::iso_to_ms_crate(ts).map(|ms| now_ms - ms);
        match name {
            "hygiene.detected" => {
                if age.map_or(false, |a| a <= 7 * DAY_MS) {
                    if let Some(spec) = v.get("spec").and_then(|s| s.as_str()).filter(|s| !s.is_empty()) {
                        if active_names.contains(spec) {
                            suspect_specs.insert(spec.to_string());
                        }
                    }
                }
            }
            "hygiene.autoclose" => {
                if age.map_or(false, |a| a <= DAY_MS) {
                    autoclose_today += 1;
                }
            }
            _ => {}
        }
    }

    spec_views::WorkspaceHealth {
        active,
        suspects: i64::try_from(suspect_specs.len()).unwrap_or(i64::MAX),
        autoclose_today,
        blocked: 0,
        wave_failed: 0,
        followup_open: 0,
        last_hygiene_run_at,
        suspect_specs: suspect_specs.into_iter().collect(),
    }
}

// ── Wave-7 telemetry aggregation commands ────────────────────────────────────

/// Lower bound (epoch-ms) for a `time_range` token (`24h` / `7d` / `30d` / `all`).
/// Anything unrecognised → 0 (all time). Used by the telemetry-plane commands to
/// filter the NDJSON fold to a window.
fn time_range_floor_ms(time_range: &str) -> i64 {
    let now = chrono::Utc::now().timestamp_millis();
    let day = 24 * 60 * 60 * 1000;
    match time_range.trim().to_lowercase().as_str() {
        "24h" | "1d" | "today" => now - day,
        "7d" | "week" => now - 7 * day,
        "30d" | "month" => now - 30 * day,
        _ => 0,
    }
}

/// Off-main-thread wrapper for [`dashboard_telemetry_phases_impl`] (cold cache
/// pays the full workspace parse). A join error degrades to an empty list.
#[tauri::command]
async fn dashboard_telemetry_phases(
    repo_path: String,
    time_range: String,
) -> Result<Vec<telemetry_agg::PhaseSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        dashboard_telemetry_phases_impl(repo_path, time_range)
    })
    .await
    .unwrap_or_else(|_| Ok(Vec::new()))
}

fn dashboard_telemetry_phases_impl(
    repo_path: String,
    time_range: String,
) -> Result<Vec<telemetry_agg::PhaseSummary>, String> {
    // Onda 2: count `pipeline.phase` events per target phase over the complete
    // walker, with a 7-day daily sparkline (oldest first, 7 slots).
    let base = PathBuf::from(&repo_path);
    let floor = time_range_floor_ms(&time_range);
    let now = chrono::Utc::now().timestamp_millis();
    let day = 24 * 60 * 60 * 1000;
    let events = telemetry::walk_ndjson_events_cached(&base);

    struct Acc {
        count: i64,
        last: Option<String>,
        spark: [i64; 7],
    }
    let mut by_phase: HashMap<String, Acc> = HashMap::new();
    for v in events.iter() {
        if telemetry::event_name_of(v) != "pipeline.phase" {
            continue;
        }
        let ts = v.get("ts").and_then(|t| t.as_str()).unwrap_or("");
        let ms = match telemetry::iso_to_ms_crate(ts) {
            Some(m) if m >= floor => m,
            _ => continue,
        };
        let phase = v
            .get("payload")
            .and_then(|p| p.get("to").or_else(|| p.get("phase")))
            .and_then(|x| x.as_str())
            .unwrap_or("");
        if phase.is_empty() {
            continue;
        }
        let entry = by_phase.entry(phase.to_string()).or_insert(Acc {
            count: 0,
            last: None,
            spark: [0; 7],
        });
        entry.count += 1;
        if entry.last.as_deref().map_or(true, |c| ts > c) {
            entry.last = Some(ts.to_string());
        }
        // Sparkline bucket: how many days ago (0..6), slot 6 = today.
        let days_ago = ((now - ms) / day).clamp(0, 6) as usize;
        entry.spark[6 - days_ago] += 1;
    }

    let mut rows: Vec<telemetry_agg::PhaseSummary> = by_phase
        .into_iter()
        .map(|(phase, a)| telemetry_agg::PhaseSummary {
            phase,
            events_count: a.count,
            last_event_at: a.last,
            sparkline: a.spark.to_vec(),
        })
        .collect();
    rows.sort_by(|a, b| b.events_count.cmp(&a.events_count).then(a.phase.cmp(&b.phase)));
    Ok(rows)
}

/// Off-main-thread wrapper for [`dashboard_telemetry_timeline_impl`] (cold
/// cache pays the full workspace parse). A join error degrades to an empty list.
#[tauri::command]
async fn dashboard_telemetry_timeline(
    repo_path: String,
    time_range: String,
    limit: Option<usize>,
) -> Result<Vec<telemetry_agg::TimelineEvent>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        dashboard_telemetry_timeline_impl(repo_path, time_range, limit)
    })
    .await
    .unwrap_or_else(|_| Ok(Vec::new()))
}

fn dashboard_telemetry_timeline_impl(
    repo_path: String,
    time_range: String,
    limit: Option<usize>,
) -> Result<Vec<telemetry_agg::TimelineEvent>, String> {
    // Onda 2 (HIGH-VALUE): cross-spec chronological event list (newest first)
    // over the complete walker, reshaped into the `TimelineEvent` shape.
    let base = PathBuf::from(&repo_path);
    let floor = time_range_floor_ms(&time_range);
    let events = telemetry::walk_ndjson_events_cached(&base);
    let timeline = telemetry::build_session_spec_timeline_from(&events);
    let mut ordered: Vec<&serde_json::Value> = events.iter().collect();
    ordered.sort_by(|a, b| {
        let ta = a.get("ts").and_then(|t| t.as_str()).unwrap_or("");
        let tb = b.get("ts").and_then(|t| t.as_str()).unwrap_or("");
        tb.cmp(ta)
    });
    let cap = limit.unwrap_or(200).min(5000);
    let rows: Vec<telemetry_agg::TimelineEvent> = ordered
        .into_iter()
        .filter(|v| {
            v.get("ts")
                .and_then(|t| t.as_str())
                .and_then(telemetry::iso_to_ms_crate)
                .map_or(false, |ms| ms >= floor)
        })
        .take(cap)
        .enumerate()
        .map(|(i, v)| {
            let re = recent_event_from_value_attributed(v, Some(&timeline));
            telemetry_agg::TimelineEvent {
                id: format!("ev-{i}"),
                ts: re.ts.unwrap_or_default(),
                phase: re.phase,
                spec: re.spec,
                agent: re.actor_id,
                summary: re.summary.unwrap_or(re.event_type),
            }
        })
        .collect();
    Ok(rows)
}

/// Off-main-thread wrapper for [`dashboard_telemetry_heatmap_impl`] (cold
/// cache pays the full workspace parse). A join error degrades to an empty
/// list. The sync `_impl` is kept so unit tests call it directly.
#[tauri::command]
async fn dashboard_telemetry_heatmap(
    repo_path: String,
    time_range: String,
) -> Result<Vec<telemetry_agg::HeatmapCell>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        dashboard_telemetry_heatmap_impl(repo_path, time_range)
    })
    .await
    .unwrap_or_else(|_| Ok(Vec::new()))
}

fn dashboard_telemetry_heatmap_impl(
    repo_path: String,
    time_range: String,
) -> Result<Vec<telemetry_agg::HeatmapCell>, String> {
    // Onda 2 (HIGH-VALUE): bucket every event's ts by weekday (0=Sun) × hour.
    let base = PathBuf::from(&repo_path);
    let floor = time_range_floor_ms(&time_range);
    let events = telemetry::walk_ndjson_events_cached(&base);
    let mut cells: HashMap<(i64, i64), i64> = HashMap::new();
    for v in events.iter() {
        let ts = v.get("ts").and_then(|t| t.as_str()).unwrap_or("");
        let ms = match telemetry::iso_to_ms_crate(ts) {
            Some(m) if m >= floor => m,
            _ => continue,
        };
        // Derive weekday + hour (UTC) from epoch-ms without a calendar dep.
        let secs = ms.div_euclid(1000);
        let days = secs.div_euclid(86_400);
        // 1970-01-01 was a Thursday (=4). 0=Sun.
        let dow = (days.rem_euclid(7) + 4) % 7;
        let hour = secs.rem_euclid(86_400) / 3_600;
        *cells.entry((dow, hour)).or_insert(0) += 1;
    }
    let mut rows: Vec<telemetry_agg::HeatmapCell> = cells
        .into_iter()
        .map(|((dow, hour), count)| telemetry_agg::HeatmapCell {
            day_of_week: dow,
            hour,
            event_count: count,
        })
        .collect();
    rows.sort_by(|a, b| a.day_of_week.cmp(&b.day_of_week).then(a.hour.cmp(&b.hour)));
    Ok(rows)
}

/// Off-main-thread wrapper for [`dashboard_telemetry_history_impl`] (full
/// workspace fold + per-spec quality rollups). A join error degrades to an
/// empty list. See [`dashboard_metrics`] for the rationale.
#[tauri::command]
async fn dashboard_telemetry_history(
    repo_path: String,
    time_range: String,
    limit: Option<usize>,
) -> Result<Vec<telemetry_agg::HistoryEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        dashboard_telemetry_history_impl(repo_path, time_range, limit)
    })
    .await
    .unwrap_or_else(|_| Ok(Vec::new()))
}

fn dashboard_telemetry_history_impl(
    repo_path: String,
    time_range: String,
    limit: Option<usize>,
) -> Result<Vec<telemetry_agg::HistoryEntry>, String> {
    // (C) trio: per-spec `pipeline.status` transition timeline + per-phase event
    // counts + AC pass/total. Built from the cached workspace slice.
    let base = PathBuf::from(&repo_path);
    let floor = time_range_floor_ms(&time_range);
    let h_events = telemetry::workspace_harness_events_cached(&base);

    struct Acc {
        status: String,
        started_at: Option<String>,
        completed_at: Option<String>,
        per_phase: std::collections::HashMap<String, i64>,
    }
    let mut by_spec: HashMap<String, Acc> = HashMap::new();
    for e in h_events.iter() {
        let Some(spec) = e.spec.as_deref() else { continue };
        if telemetry::iso_to_ms_crate(&e.ts).map_or(true, |ms| ms < floor) {
            continue;
        }
        let entry = by_spec.entry(spec.to_string()).or_insert_with(|| Acc {
            status: String::new(),
            started_at: None,
            completed_at: None,
            per_phase: std::collections::HashMap::new(),
        });
        if entry.started_at.as_deref().map_or(true, |c| e.ts.as_str() < c) {
            entry.started_at = Some(e.ts.clone());
        }
        match e.event.as_str() {
            "pipeline.status" => {
                if let Some(to) = e.payload.get("to").and_then(|x| x.as_str()) {
                    entry.status = to.to_string();
                    if matches!(to, "completed" | "cancelled" | "closed-followup") {
                        entry.completed_at = Some(e.ts.clone());
                    }
                }
            }
            "pipeline.phase" => {
                if let Some(to) = e.payload.get("to").and_then(|x| x.as_str()) {
                    *entry.per_phase.entry(to.to_string()).or_insert(0) += 1;
                }
            }
            _ => {}
        }
    }

    let mut rows: Vec<telemetry_agg::HistoryEntry> = by_spec
        .into_iter()
        .map(|(spec, a)| {
            let rollup = mustard_core::view::projection::project_quality(&spec, &h_events);
            telemetry_agg::HistoryEntry {
                spec,
                status: a.status,
                started_at: a.started_at.unwrap_or_default(),
                completed_at: a.completed_at,
                duration_per_phase: a.per_phase,
                ac_passed: i64::from(rollup.passed),
                ac_total: i64::from(rollup.total),
            }
        })
        .collect();
    rows.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    if let Some(n) = limit {
        rows.truncate(n);
    }
    Ok(rows)
}

/// Off-main-thread wrapper for [`dashboard_telemetry_criteria_impl`]. A join
/// error degrades to an empty list.
#[tauri::command]
async fn dashboard_telemetry_criteria(
    repo_path: String,
    time_range: String,
) -> Result<Vec<telemetry_agg::AcceptanceCriterion>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        dashboard_telemetry_criteria_impl(repo_path, time_range)
    })
    .await
    .unwrap_or_else(|_| Ok(Vec::new()))
}

fn dashboard_telemetry_criteria_impl(
    repo_path: String,
    time_range: String,
) -> Result<Vec<telemetry_agg::AcceptanceCriterion>, String> {
    // (C) trio: AC rows across every spec via `project_quality` (qa.result).
    let base = PathBuf::from(&repo_path);
    let floor = time_range_floor_ms(&time_range);
    let events = telemetry::workspace_harness_events_cached(&base);
    let specs: std::collections::BTreeSet<String> = events
        .iter()
        .filter(|e| e.event == "qa.result")
        .filter_map(|e| e.spec.clone())
        .collect();
    let mut rows: Vec<telemetry_agg::AcceptanceCriterion> = Vec::new();
    for spec in specs {
        let rollup = mustard_core::view::projection::project_quality(&spec, &events);
        for c in rollup.criteria {
            // Window filter on the AC's last run.
            if let Some(run) = c.last_run_at.as_deref() {
                if telemetry::iso_to_ms_crate(run).map_or(false, |ms| ms < floor) {
                    continue;
                }
            }
            rows.push(telemetry_agg::AcceptanceCriterion {
                spec: spec.clone(),
                id: c.id,
                status: ac_status_word(c.status),
                last_run_at: c.last_run_at,
            });
        }
    }
    Ok(rows)
}

/// Lowercase string for a core `AcStatus` — the criteria command's status field.
fn ac_status_word(s: mustard_core::AcStatus) -> String {
    use mustard_core::AcStatus;
    match s {
        AcStatus::Pass => "pass",
        AcStatus::Fail => "fail",
        AcStatus::Skip => "skip",
        AcStatus::Pending => "pending",
    }
    .to_string()
}

/// Off-main-thread wrapper for [`dashboard_telemetry_effort_impl`] (cold cache
/// pays the full workspace parse). A join error degrades to an empty breakdown.
#[tauri::command]
async fn dashboard_telemetry_effort(
    repo_path: String,
    time_range: String,
) -> Result<telemetry_agg::EffortBreakdown, String> {
    tauri::async_runtime::spawn_blocking(move || {
        dashboard_telemetry_effort_impl(repo_path, time_range)
    })
    .await
    .unwrap_or_else(|_| Ok(telemetry_agg::EffortBreakdown::default()))
}

fn dashboard_telemetry_effort_impl(
    repo_path: String,
    time_range: String,
) -> Result<telemetry_agg::EffortBreakdown, String> {
    // (C) trio: top_files (`tool.use` target.file_path), top_tools
    // (`tool_breakdown`), top_phases (`pipeline.phase` counts as a duration
    // proxy), top_agents (`agent_activity`).
    let base = PathBuf::from(&repo_path);
    let floor = time_range_floor_ms(&time_range);
    let events = telemetry::walk_ndjson_events_cached(&base);

    let mut files: HashMap<String, i64> = HashMap::new();
    let mut phases: HashMap<String, i64> = HashMap::new();
    for v in events.iter() {
        let ts = v.get("ts").and_then(|t| t.as_str()).unwrap_or("");
        if telemetry::iso_to_ms_crate(ts).map_or(true, |ms| ms < floor) {
            continue;
        }
        match telemetry::event_name_of(v) {
            "tool.use" => {
                if let Some(f) = v
                    .get("payload")
                    .and_then(|p| p.get("target"))
                    .and_then(|t| t.as_object())
                    .and_then(|o| o.get("file_path").or_else(|| o.get("file")))
                    .and_then(|x| x.as_str())
                    .filter(|s| !s.is_empty())
                {
                    *files.entry(f.to_string()).or_insert(0) += 1;
                }
            }
            "pipeline.phase" => {
                if let Some(p) = v
                    .get("payload")
                    .and_then(|p| p.get("to").or_else(|| p.get("phase")))
                    .and_then(|x| x.as_str())
                    .filter(|s| !s.is_empty())
                {
                    *phases.entry(p.to_string()).or_insert(0) += 1;
                }
            }
            _ => {}
        }
    }

    let mut top_files: Vec<telemetry_agg::FileCount> = files
        .into_iter()
        .map(|(path, count)| telemetry_agg::FileCount { path, count })
        .collect();
    top_files.sort_by(|a, b| b.count.cmp(&a.count).then(a.path.cmp(&b.path)));
    top_files.truncate(15);

    let top_tools: Vec<telemetry_agg::ToolUseCount> = telemetry::tool_breakdown(&base)
        .into_iter()
        .map(|t| telemetry_agg::ToolUseCount {
            name: t.tool_name,
            count: i64::try_from(t.count).unwrap_or(i64::MAX),
        })
        .collect();

    let mut top_phases: Vec<telemetry_agg::PhaseEventCount> = phases
        .into_iter()
        .map(|(phase, count)| telemetry_agg::PhaseEventCount {
            phase,
            duration_ms: count,
        })
        .collect();
    top_phases.sort_by(|a, b| b.duration_ms.cmp(&a.duration_ms).then(a.phase.cmp(&b.phase)));

    let top_agents: Vec<telemetry_agg::AgentTypeCount> = telemetry::agent_activity(&base)
        .agents
        .into_iter()
        .map(|a| telemetry_agg::AgentTypeCount {
            agent_type: a.agent_type,
            count: i64::try_from(a.starts).unwrap_or(i64::MAX),
        })
        .collect();

    Ok(telemetry_agg::EffortBreakdown {
        top_files,
        top_tools,
        top_phases,
        top_agents,
    })
}

/// Off-main-thread wrapper for [`dashboard_telemetry_agents_impl`] (cold cache
/// pays the full workspace parse). A join error degrades to an empty list.
#[tauri::command]
async fn dashboard_telemetry_agents(
    repo_path: String,
    time_range: String,
) -> Result<Vec<telemetry_agg::AgentDispatch>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        dashboard_telemetry_agents_impl(repo_path, time_range)
    })
    .await
    .unwrap_or_else(|_| Ok(Vec::new()))
}

fn dashboard_telemetry_agents_impl(
    repo_path: String,
    time_range: String,
) -> Result<Vec<telemetry_agg::AgentDispatch>, String> {
    // Onda 2 (HIGH-VALUE): reshape `agent_activity` into the `AgentDispatch`
    // rows the telemetry page consumes. (time_range is accepted for contract
    // parity; agent_activity already folds the whole workspace — the page sorts
    // and trims client-side.)
    let _ = time_range;
    let base = PathBuf::from(&repo_path);
    let rows: Vec<telemetry_agg::AgentDispatch> = telemetry::agent_activity(&base)
        .agents
        .into_iter()
        .map(|a| telemetry_agg::AgentDispatch {
            subagent_type: a.agent_type,
            count: i64::try_from(a.starts).unwrap_or(i64::MAX),
            error_count: i64::try_from(a.errors).unwrap_or(i64::MAX),
            avg_duration_ms: i64::try_from(a.avg_duration_ms).unwrap_or(i64::MAX),
            last_dispatched_at: a.last_ts,
        })
        .collect();
    Ok(rows)
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
        // Wave 6A of `2026-05-26-no-sqlite-git-source-of-truth` retired the
        // shared relational handle. The dashboard now reads from per-spec
        // NDJSON / spec.md filesystem walks — no process-wide cache remains.
        // The setup hook only installs the updater plugin.
        .setup(|app| {
            #[cfg(desktop)]
            app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            dashboard_pipelines, dashboard_metrics, dashboard_knowledge,
            dashboard_subprojects, dashboard_skills, dashboard_recent_events,
            dashboard_specs, dashboard_spec_markdown, commands::specs::read_spec_meta,
            commands::settings::set_language,
            commands::settings::set_tone,
            commands::settings::read_settings,
            dashboard_spec_complete, dashboard_spec_cancel, dashboard_spec_reactivate,
            dashboard_search_events, dashboard_search_knowledge,
            dashboard_telemetry, dashboard_live_activity, dashboard_friction,
            telemetry::dashboard_prompt_economy,
            telemetry::dashboard_economy_summary,
            telemetry::dashboard_economy_savings_breakdown,
            telemetry::dashboard_economy_context_routing,
            telemetry::dashboard_economy_per_spec_costs,
            telemetry::dashboard_economy_per_wave_costs,
            telemetry::dashboard_spec_trace,
            telemetry::dashboard_sessions,
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
            doctor::doctor_status,
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
            dashboard_spec_cards,
            dashboard_spec_waves,
            dashboard_spec_checklist_progress,
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
            read_model_entities,
            workspace_health,
            economy::economy_summary,
            git_info::dashboard_git_info,
            git_info::dashboard_git_log,
            file_read::dashboard_read_file,
            project_overview::dashboard_project_overview
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod onda2_tests {
    use super::*;
    use tempfile::TempDir;

    fn write_event(dir: &std::path::Path, spec: &str, name: &str, body: &str) {
        let events_dir = dir.join(".claude").join("spec").join(spec).join(".events");
        std::fs::create_dir_all(&events_dir).unwrap();
        std::fs::write(events_dir.join(name), body).unwrap();
    }

    #[test]
    fn knowledge_rows_project_frontmatter_and_body() {
        let tmp = TempDir::new().unwrap();
        let kdir = tmp.path().join(".claude").join("knowledge");
        std::fs::create_dir_all(&kdir).unwrap();
        std::fs::write(
            kdir.join("20260101T000000Z-aaa.md"),
            "---\nkind: decision\nspec: my-spec\n---\n**D1** — keep state in meta.json\n",
        )
        .unwrap();
        std::fs::write(
            kdir.join("20260102T000000Z-bbb.md"),
            "---\nkind: pattern\nconfidence: 0.5\nspec: other\n---\nReuse the projection layer\n",
        )
        .unwrap();

        let rows = read_knowledge_rows(tmp.path());
        assert_eq!(rows.len(), 2);
        // Newest first (lexical stem desc).
        assert_eq!(rows[0].id, "20260102T000000Z-bbb");
        assert_eq!(rows[0].type_, "pattern");
        assert!((rows[0].confidence - 0.5).abs() < 1e-9);
        assert_eq!(rows[0].source.as_deref(), Some("other"));
        assert!(rows[0].name.contains("Reuse the projection"));
        // Decision with no confidence field defaults to 1.0 (confirmed).
        let d = rows.iter().find(|r| r.type_ == "decision").unwrap();
        assert!((d.confidence - 1.0).abs() < 1e-9);
        assert_eq!(d.source.as_deref(), Some("my-spec"));

        // Summary counts patterns/conventions/high-confidence honestly.
        let summary = dashboard_knowledge(tmp.path().to_string_lossy().into_owned()).unwrap();
        assert_eq!(summary.patterns_count, 1);
        assert_eq!(summary.conventions_count, 0);
        assert_eq!(summary.high_confidence_count, 1); // only the 1.0 decision
    }

    #[test]
    fn knowledge_empty_when_dir_absent() {
        let tmp = TempDir::new().unwrap();
        assert!(read_knowledge_rows(tmp.path()).is_empty());
        let s = dashboard_knowledge(tmp.path().to_string_lossy().into_owned()).unwrap();
        assert_eq!(s.patterns_count, 0);
    }

    #[test]
    fn recent_event_summary_for_tool_use() {
        let v = serde_json::json!({
            "event": "tool.use", "kind": "tool", "ts": "2026-05-27T09:00:00.000Z",
            "spec": "alpha", "wave": 2, "actor": {"kind": "agent", "id": "explore-1"},
            "payload": {"tool": "Read", "target": {"file_path": "src/foo.rs"}}
        });
        let re = recent_event_from_value_attributed(&v, None);
        assert_eq!(re.event_type, "tool.use");
        assert_eq!(re.spec.as_deref(), Some("alpha"));
        assert_eq!(re.wave, Some(2));
        assert_eq!(re.tool_name.as_deref(), Some("Read"));
        assert_eq!(re.target.as_deref(), Some("src/foo.rs"));
        assert_eq!(re.actor_kind.as_deref(), Some("agent"));
        assert_eq!(re.actor_id.as_deref(), Some("explore-1"));
        assert_eq!(re.summary.as_deref(), Some("Read · src/foo.rs"));
    }

    #[test]
    fn recent_events_are_newest_first() {
        let tmp = TempDir::new().unwrap();
        let lines = concat!(
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:00:00.000Z","spec":"a","payload":{"tool":"Read"}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:05:00.000Z","spec":"a","payload":{"tool":"Edit"}}"#, "\n",
        );
        write_event(tmp.path(), "a", "events.ndjson", lines);
        let rows = dashboard_recent_events_impl(tmp.path().to_string_lossy().into_owned(), None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].ts.as_deref(), Some("2026-05-27T09:05:00.000Z"));
        assert_eq!(rows[0].tool_name.as_deref(), Some("Edit"));
    }

    #[test]
    fn search_events_substring_filter() {
        let tmp = TempDir::new().unwrap();
        let lines = concat!(
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:00:00.000Z","spec":"a","payload":{"tool":"Read"}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:05:00.000Z","spec":"a","payload":{"tool":"Edit"}}"#, "\n",
        );
        write_event(tmp.path(), "a", "events.ndjson", lines);
        let rows = dashboard_search_events_impl(
            tmp.path().to_string_lossy().into_owned(),
            "edit".to_string(),
            None,
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tool_name.as_deref(), Some("Edit"));
    }

    #[test]
    fn activity_aggregated_groups_by_spec_wave_action() {
        let tmp = TempDir::new().unwrap();
        let lines = concat!(
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:00:00.000Z","spec":"a","wave":1,"payload":{"tool":"Read","target":{"file_path":"x.rs"}}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:01:00.000Z","spec":"a","wave":1,"payload":{"tool":"Read","target":{"file_path":"y.rs"}}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:02:00.000Z","spec":"a","wave":1,"payload":{"tool":"Edit","target":{"file_path":"x.rs"}}}"#, "\n",
        );
        write_event(tmp.path(), "a", "events.ndjson", lines);
        let rows = dashboard_activity_aggregated_impl(tmp.path().to_string_lossy().into_owned(), None).unwrap();
        let read = rows
            .iter()
            .find(|g| g.action_kind.as_deref() == Some("Read"))
            .expect("Read group");
        assert_eq!(read.count, 2);
        assert_eq!(read.files_touched, 2);
        assert_eq!(read.min_ts.as_deref(), Some("2026-05-27T09:00:00.000Z"));
        assert_eq!(read.max_ts.as_deref(), Some("2026-05-27T09:01:00.000Z"));
        assert_eq!(read.wave, Some(1));
    }

    #[test]
    fn heatmap_buckets_weekday_and_hour() {
        let tmp = TempDir::new().unwrap();
        // 2026-05-27 is a Wednesday (=3). 10:00 UTC.
        let lines = concat!(
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T10:00:00.000Z","spec":"a","payload":{"tool":"Read"}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T10:30:00.000Z","spec":"a","payload":{"tool":"Edit"}}"#, "\n",
        );
        write_event(tmp.path(), "a", "events.ndjson", lines);
        let cells = dashboard_telemetry_heatmap_impl(
            tmp.path().to_string_lossy().into_owned(),
            "all".to_string(),
        )
        .unwrap();
        let cell = cells
            .iter()
            .find(|c| c.day_of_week == 3 && c.hour == 10)
            .expect("Wed 10:00 cell");
        assert_eq!(cell.event_count, 2);
    }

    #[test]
    fn metrics_count_events_and_tokens() {
        let tmp = TempDir::new().unwrap();
        let lines = concat!(
            r#"{"event":"pipeline.telemetry.run","kind":"pipeline.telemetry.run","ts":"2026-05-27T09:00:00.000Z","spec":"a","payload":{},"tokens_in":1000,"tokens_out":500}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:01:00.000Z","spec":"a","payload":{"tool":"Read"}}"#, "\n",
        );
        write_event(tmp.path(), "a", "events.ndjson", lines);
        let m = dashboard_metrics_impl(tmp.path().to_string_lossy().into_owned()).unwrap();
        assert_eq!(m.total_events, 2);
        assert_eq!(m.tokens_total, 1500);
        assert_eq!(m.last_event_at.as_deref(), Some("2026-05-27T09:01:00.000Z"));
    }

    #[test]
    fn dashboard_specs_returns_parent_but_not_wave_children() {
        let tmp = TempDir::new().unwrap();
        let spec_root = tmp.path().join(".claude").join("spec");
        // A wave-plan parent: has wave-plan.md + a wave-1-x/spec.md child.
        let parent = spec_root.join("epic");
        std::fs::create_dir_all(&parent).unwrap();
        std::fs::write(parent.join("wave-plan.md"), "# epic plan\n").unwrap();
        let child = parent.join("wave-1-frontend");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(child.join("spec.md"), "# wave 1\n### Phase: EXECUTE\n").unwrap();
        // A standalone top-level spec for good measure.
        let solo = spec_root.join("solo");
        std::fs::create_dir_all(&solo).unwrap();
        std::fs::write(solo.join("spec.md"), "# solo\n").unwrap();

        // specs_from_fs still walks the wave child (used elsewhere).
        let fs_rows = specs_from_fs(tmp.path());
        assert!(
            fs_rows.iter().any(|r| r.name == "wave-1-frontend"),
            "specs_from_fs should still surface wave children"
        );

        // dashboard_specs filters to top-level only.
        let rows = dashboard_specs_impl(tmp.path().to_string_lossy().into_owned()).unwrap();
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"epic"), "parent spec must be kept");
        assert!(names.contains(&"solo"), "standalone spec must be kept");
        assert!(
            !names.contains(&"wave-1-frontend"),
            "wave subdirectory must be dropped"
        );
        assert!(rows.iter().all(|r| r.parent.is_none()));
    }

    #[test]
    fn specs_cache_serves_warm_list_until_invalidated() {
        // Wave-1 task 4: the spec LIST never waits on a directory walk when
        // warm; the watcher (kind `spec`) is the invalidation path.
        let tmp = TempDir::new().unwrap();
        let spec_root = tmp.path().join(".claude").join("spec");
        let solo = spec_root.join("solo");
        std::fs::create_dir_all(&solo).unwrap();
        std::fs::write(solo.join("spec.md"), "# solo\n").unwrap();

        let first = specs_from_fs_cached(tmp.path());
        assert!(first.iter().any(|r| r.name == "solo"));

        // A new spec lands on disk: the warm cache still serves the shared
        // list (same Arc — no re-walk happened).
        let late = spec_root.join("later");
        std::fs::create_dir_all(&late).unwrap();
        std::fs::write(late.join("spec.md"), "# later\n").unwrap();
        let warm = specs_from_fs_cached(tmp.path());
        assert!(
            std::sync::Arc::ptr_eq(&first, &warm),
            "a warm hit must share the Arc, not re-walk"
        );
        assert!(!warm.iter().any(|r| r.name == "later"));

        // Watcher-style invalidation → the fresh walk picks the new spec up.
        invalidate_specs_cache(&tmp.path().to_string_lossy());
        let fresh = specs_from_fs_cached(tmp.path());
        assert!(fresh.iter().any(|r| r.name == "later"));
    }

    #[test]
    fn spec_action_emits_status_and_returns_ok() {
        let tmp = TempDir::new().unwrap();
        // Seed a spec dir so the header sync has a file (sync is fail-open anyway).
        let spec_dir = tmp.path().join(".claude").join("spec").join("a");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# a\n### Status: implementing\n").unwrap();

        let res = dashboard_spec_action(
            tmp.path().to_string_lossy().into_owned(),
            "a".to_string(),
            "close".to_string(),
        )
        .unwrap();
        assert_eq!(res.result, "ok");
        assert_eq!(res.message.as_deref(), Some("completed"));
        // A pipeline.status event was appended to the dashboard sink.
        let ndjson = spec_dir.join(".events").join("dashboard.ndjson");
        let body = std::fs::read_to_string(&ndjson).unwrap();
        assert!(body.contains("pipeline.status"));
        assert!(body.contains("completed"));

        // Unknown verb still errors.
        assert!(dashboard_spec_action(
            tmp.path().to_string_lossy().into_owned(),
            "a".to_string(),
            "frobnicate".to_string(),
        )
        .is_err());
    }

    #[test]
    fn active_pipelines_excludes_terminal_and_no_events() {
        let tmp = TempDir::new().unwrap();
        // Spec "live": has phase events, non-terminal.
        write_event(
            tmp.path(),
            "live",
            "events.ndjson",
            concat!(
                r#"{"event":"pipeline.phase","kind":"pipeline","ts":"2026-05-27T09:00:00.000Z","spec":"live","payload":{"to":"EXECUTE"}}"#, "\n",
            ),
        );
        std::fs::write(
            tmp.path().join(".claude").join("spec").join("live").join("spec.md"),
            "# live\n",
        )
        .unwrap();
        // Spec "done": completed status → terminal.
        write_event(
            tmp.path(),
            "done",
            "events.ndjson",
            concat!(
                r#"{"event":"pipeline.phase","kind":"pipeline","ts":"2026-05-27T08:00:00.000Z","spec":"done","payload":{"to":"CLOSE"}}"#, "\n",
                r#"{"event":"pipeline.status","kind":"pipeline","ts":"2026-05-27T08:01:00.000Z","spec":"done","payload":{"to":"completed"}}"#, "\n",
            ),
        );
        std::fs::write(
            tmp.path().join(".claude").join("spec").join("done").join("spec.md"),
            "# done\n",
        )
        .unwrap();

        let actives =
            dashboard_active_pipelines_impl(tmp.path().to_string_lossy().into_owned()).unwrap();
        let names: Vec<&str> = actives.iter().map(|p| p.spec_name.as_str()).collect();
        assert!(names.contains(&"live"), "live spec should be active: {names:?}");
        assert!(!names.contains(&"done"), "completed spec must be excluded");
    }

    /// Write a spec-less session work event under `.claude/.session/{id}/.events/`,
    /// mirroring the on-disk shape the harness writes when the emitter sets
    /// `spec == null`.
    fn write_session_event(dir: &std::path::Path, session: &str, name: &str, body: &str) {
        let events_dir = dir
            .join(".claude")
            .join(".session")
            .join(session)
            .join(".events");
        std::fs::create_dir_all(&events_dir).unwrap();
        std::fs::write(events_dir.join(name), body).unwrap();
    }

    #[test]
    fn card_attributes_specless_session_tool_use_to_bound_spec() {
        // The reported card gap: the only work events for a live spec are
        // spec-less `tool.use` rows under `.session/{id}/.events/`. A
        // `pipeline.scope` binding (session=sess-1 → spec=alpha at an EARLIER ts)
        // lives under the spec's own `.events/`. The core fold keys on
        // `event.spec` and so reports tools 0 / arquivos 0; the dashboard layer
        // must re-attribute and surface non-zero tool + file counts.
        let tmp = TempDir::new().unwrap();
        // Binding event under the spec dir (carries explicit spec + session_id).
        let binding = r#"{"event":"pipeline.scope","kind":"pipeline","ts":"2026-05-27T09:00:00.000Z","session_id":"sess-1","spec":"alpha","payload":{"scope":"full"}}"#;
        write_event(tmp.path(), "alpha", "scope.ndjson", &format!("{binding}\n"));
        std::fs::write(
            tmp.path().join(".claude").join("spec").join("alpha").join("spec.md"),
            "# alpha\n",
        )
        .unwrap();
        // Two spec-less session tool.use events touching two distinct files,
        // both after the binding ts → attribute to alpha.
        let work = concat!(
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:30:00.000Z","session_id":"sess-1","spec":null,"payload":{"tool":"Edit","target":{"file_path":"src/live.rs"}}}"#, "\n",
            r#"{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:31:00.000Z","session_id":"sess-1","spec":null,"payload":{"tool":"Read","target":{"file_path":"src/other.rs"}}}"#, "\n",
        );
        write_session_event(tmp.path(), "sess-1", "work.ndjson", work);

        // Spec card: tools/files counts must include the attributed session work.
        let card = dashboard_spec_card_impl(
            tmp.path().to_string_lossy().into_owned(),
            "alpha".to_string(),
        )
        .unwrap();
        assert_eq!(card.tools_used, 2, "two attributed session tool.use events");
        assert_eq!(card.files_touched, 2, "two distinct attributed file targets");
        assert_eq!(
            card.last_event_at.as_deref(),
            Some("2026-05-27T09:31:00.000Z"),
            "last activity must reflect the attributed session event"
        );

        // Active-pipelines: the spec stays listed and its updated_at advances to
        // the attributed session activity.
        let actives =
            dashboard_active_pipelines_impl(tmp.path().to_string_lossy().into_owned()).unwrap();
        let alpha = actives
            .iter()
            .find(|p| p.spec_name == "alpha")
            .expect("alpha must be an active pipeline");
        assert_eq!(alpha.updated_at.as_deref(), Some("2026-05-27T09:31:00.000Z"));
    }

    #[test]
    fn spec_cards_batch_folds_attributed_counts_once() {
        // T1 contract (spec `sidebar-lento-lista-specs-dispara`): the batch
        // command returns one card per listed spec while paying exactly ONE
        // `attributed_spec_counts` workspace fold — the per-row re-fold was
        // the Specs-page latency bug. Counter is per-repo (TempDir), so
        // parallel tests never skew the delta.
        let tmp = TempDir::new().unwrap();
        for spec in ["alpha", "beta"] {
            let line = format!(
                r#"{{"event":"tool.use","kind":"tool","ts":"2026-05-27T09:00:00.000Z","spec":"{spec}","payload":{{"tool":"Read","target":{{"file_path":"src/x.rs"}}}}}}"#
            );
            write_event(tmp.path(), spec, "events.ndjson", &format!("{line}\n"));
            std::fs::write(
                tmp.path().join(".claude").join("spec").join(spec).join("spec.md"),
                format!("# {spec}\n"),
            )
            .unwrap();
        }
        // A third spec with NO events: the batch must still emit its card as
        // the "no-events" empty state (parity with the per-spec command).
        let gamma_dir = tmp.path().join(".claude").join("spec").join("gamma");
        std::fs::create_dir_all(&gamma_dir).unwrap();
        std::fs::write(gamma_dir.join("spec.md"), "# gamma\n").unwrap();

        let before = telemetry::attributed_spec_counts_calls(tmp.path());
        let cards =
            dashboard_spec_cards_impl(tmp.path().to_string_lossy().into_owned()).unwrap();
        assert_eq!(cards.len(), 3, "one card per listed spec");
        assert!(cards.iter().any(|c| c.spec == "alpha" && c.tools_used > 0));
        assert!(cards.iter().any(|c| c.spec == "beta" && c.tools_used > 0));
        assert!(
            cards.iter().any(|c| c.spec == "gamma" && c.status == "no-events"),
            "event-less spec degrades to the no-events card, not a dropped row"
        );
        assert_eq!(
            telemetry::attributed_spec_counts_calls(tmp.path()),
            before + 1,
            "N cards must cost exactly ONE attributed-counts fold"
        );
    }

    /// Write a spec's `meta.json` sidecar beside its `spec.md`.
    fn write_meta_json(dir: &std::path::Path, spec: &str, meta: &mustard_core::Meta) {
        let spec_dir = dir.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        mustard_core::write_meta(&spec_dir.join("meta.json"), meta).unwrap();
    }

    // ── Fix A: lifecycle status sourced from meta.json ───────────────────────
    #[test]
    fn meta_completed_overrides_event_status_to_completed() {
        // The events only reach `closed-followup` (no terminal `completed`
        // event), but `meta.json` says (Close, Completed). The card status must
        // reflect meta → `completed` so the frontend classifies it as Encerradas
        // (both the group and the filter bucket), not Ativas with a follow-up.
        let tmp = TempDir::new().unwrap();
        write_event(
            tmp.path(),
            "payable",
            "events.ndjson",
            concat!(
                r#"{"event":"pipeline.status","kind":"pipeline","ts":"2026-05-27T08:00:00.000Z","spec":"payable","payload":{"to":"approved"}}"#, "\n",
                r#"{"event":"pipeline.status","kind":"pipeline","ts":"2026-05-27T08:01:00.000Z","spec":"payable","payload":{"to":"closed-followup"}}"#, "\n",
            ),
        );
        std::fs::write(
            tmp.path().join(".claude").join("spec").join("payable").join("spec.md"),
            "# payable\n",
        )
        .unwrap();
        let mut meta = mustard_core::Meta::new(
            Some("Close"), Some("Completed"), Some("CLOSE"), None, None, None, None,
        );
        meta.is_wave_plan = Some(true);
        meta.total_waves = Some(2);
        write_meta_json(tmp.path(), "payable", &meta);

        let card = dashboard_spec_card_impl(
            tmp.path().to_string_lossy().into_owned(),
            "payable".to_string(),
        )
        .unwrap();
        assert_eq!(card.status, "completed", "meta.json (Close, Completed) must win over event-derived status");

        // And the active-pipelines list (which mirrors the Ativas taxonomy on
        // the backend) must drop it as terminal.
        let actives =
            dashboard_active_pipelines_impl(tmp.path().to_string_lossy().into_owned()).unwrap();
        assert!(
            !actives.iter().any(|p| p.spec_name == "payable"),
            "a meta-Completed spec must not appear as an active pipeline",
        );
    }

    #[test]
    fn meta_absent_falls_back_to_event_status() {
        // No meta.json → keep the event-derived status (here `closed-followup`).
        let tmp = TempDir::new().unwrap();
        write_event(
            tmp.path(),
            "nometa",
            "events.ndjson",
            concat!(
                r#"{"event":"pipeline.status","kind":"pipeline","ts":"2026-05-27T08:01:00.000Z","spec":"nometa","payload":{"to":"closed-followup"}}"#, "\n",
            ),
        );
        std::fs::write(
            tmp.path().join(".claude").join("spec").join("nometa").join("spec.md"),
            "# nometa\n",
        )
        .unwrap();
        let card = dashboard_spec_card_impl(
            tmp.path().to_string_lossy().into_owned(),
            "nometa".to_string(),
        )
        .unwrap();
        assert_eq!(card.status, "closed-followup", "no meta.json → event status preserved");
    }

    // ── Fix B: AC list carries the parsed description ────────────────────────
    #[test]
    fn quality_ac_label_carries_parsed_spec_description() {
        let tmp = TempDir::new().unwrap();
        // qa.result event with NO label → projection falls back to bare id.
        write_event(
            tmp.path(),
            "acme",
            "events.ndjson",
            concat!(
                r#"{"event":"qa.result","kind":"qa","ts":"2026-05-27T09:00:00.000Z","spec":"acme","payload":{"criteria":[{"id":"AC-1","status":"pass"},{"id":"AC-2","status":"fail"}]}}"#, "\n",
            ),
        );
        std::fs::write(
            tmp.path().join(".claude").join("spec").join("acme").join("spec.md"),
            "# acme\n\n## Critérios de Aceitação\n\n- **AC-1** — Build passes on Windows.\n  Command: `rtk cargo build`\n- **AC-2** — Lint is clean.\n  Command: `rtk lint`\n",
        )
        .unwrap();
        let items = spec_views::spec_quality_v2(
            &tmp.path().to_string_lossy(),
            "acme",
        )
        .unwrap();
        let ac1 = items.iter().find(|i| i.ac_id == "AC-1").expect("AC-1");
        let ac2 = items.iter().find(|i| i.ac_id == "AC-2").expect("AC-2");
        assert_eq!(ac1.ac_label.as_deref(), Some("Build passes on Windows."));
        assert_eq!(ac2.ac_label.as_deref(), Some("Lint is clean."));
        // Status is not dropped by the enrichment.
        assert_eq!(ac1.status, "pass");
        assert_eq!(ac2.status, "fail");
    }

    // ── Fix C: wave list carries the role (+ summary) ────────────────────────
    #[test]
    fn waves_carry_role_and_summary_from_wave_plan() {
        let tmp = TempDir::new().unwrap();
        // Dispatch with no `role` in payload → projection role is None, so the
        // dashboard must enrich it from wave-plan.md / the wave dir name.
        write_event(
            tmp.path(),
            "epic",
            "events.ndjson",
            concat!(
                r#"{"event":"pipeline.task.dispatch","kind":"pipeline","ts":"2026-05-27T09:00:00.000Z","spec":"epic","payload":{"wave":1,"name":"Wave 1"}}"#, "\n",
                r#"{"event":"pipeline.task.complete","kind":"pipeline","ts":"2026-05-27T09:05:00.000Z","spec":"epic","payload":{"wave":1,"name":"Wave 1"}}"#, "\n",
            ),
        );
        let spec_dir = tmp.path().join(".claude").join("spec").join("epic");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# epic\n").unwrap();
        // wave-N-{role} dir (role fallback) + wave-plan.md table (role + summary).
        std::fs::create_dir_all(spec_dir.join("wave-1-impl")).unwrap();
        std::fs::write(
            spec_dir.join("wave-plan.md"),
            "# epic\n\n| Wave | Spec | Role | Depends on | Summary |\n| --- | --- | --- | --- | --- |\n| 1 | [[wave-1-impl]] | impl | — | Implement the backend reader |\n",
        )
        .unwrap();

        let waves = spec_views::spec_waves_v2(
            &tmp.path().to_string_lossy(),
            "epic",
        )
        .unwrap();
        let w1 = waves.iter().find(|w| w.wave == 1).expect("wave 1");
        assert_eq!(w1.role.as_deref(), Some("impl"), "role from wave-plan.md");
        assert_eq!(
            w1.summary.as_deref(),
            Some("Implement the backend reader"),
            "summary from wave-plan.md",
        );
    }
}
