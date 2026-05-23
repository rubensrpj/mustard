//! `mustard-rt run resume-bootstrap` — single-shot resume decision engine.
//!
//! Replaces Steps 0 + 0.5 + 1 + 5 + parts of Step 2 of the legacy
//! `resume-flow.md` ref. One process call resolves: mode (`continued` |
//! `reanalyzed` | `ask`), spec stage, the operational spec path (root
//! `spec.md` or `wave-N-{role}/spec.md`), wave progress, stub flag, dispatch
//! failure replay, whether to refresh `diff` / `context-slice`, the wave's
//! model, a `## Resumo` one-liner, and the discovered agent roles. Emits a
//! `pipeline.resume_mode` event before returning (idempotent — skips if a
//! recent one already exists for the spec).
//!
//! ## Fail-open contract
//!
//! ANY IO error — missing spec dir, missing event store, unparseable header —
//! degrades the affected field to `null`/`false`. The process never panics and
//! always exits 0; the orchestrator gets a partial JSON document instead of
//! an error.

use crate::run::env::{project_dir, session_id};
use crate::run::event_projections::{pipeline_state_for_spec, PipelineStateView};
use crate::util::now_iso8601;
use mustard_core::fs as mfs;
use mustard_core::model::event::{
    Actor, ActorKind, HarnessEvent, PipelineDispatchFailurePayload, SCHEMA_VERSION,
    EVENT_PIPELINE_RESUME_MODE, EVENT_PIPELINE_WAVE_COMPLETE,
};
use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::SqliteEventStore;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};

/// Window inside which auto-continue applies (10 minutes since last event).
const AUTO_CONTINUE_TTL_MS: i64 = 10 * 60 * 1_000;
/// Window inside which a freshly emitted `pipeline.resume_mode` event suppresses
/// re-emission (idempotency — 10 seconds).
const RESUME_MODE_DEBOUNCE_MS: i64 = 10 * 1_000;
/// Cap on the `## Resumo` first-line snippet.
const SUMMARY_CAP: usize = 200;

/// One-shot JSON output of `resume-bootstrap`.
#[derive(Debug, Serialize, Default)]
pub struct ResumeBootstrap {
    /// `continued` | `reanalyzed` | `ask`.
    pub mode: String,
    /// Canonical `Stage` word: `Plan` | `Execute` | `Analyze` | `QaReview` | `Close`.
    pub stage: Option<String>,
    /// Operational spec path (root `spec.md` or `wave-N-{role}/spec.md`).
    #[serde(rename = "operationalSpecPath")]
    pub operational_spec_path: Option<String>,
    /// Whether the spec uses a wave plan.
    #[serde(rename = "isWavePlan")]
    pub is_wave_plan: bool,
    /// Current wave (1-based). `0` when not a wave plan.
    #[serde(rename = "currentWave")]
    pub current_wave: u32,
    /// Total wave count. `0` when not a wave plan.
    #[serde(rename = "totalWaves")]
    pub total_waves: u32,
    /// `true` when the operational spec is a stub (Stage: Plan + no `## Files`/`## Tasks`).
    #[serde(rename = "isStub")]
    pub is_stub: bool,
    /// Most recent unrecovered dispatch failure (if any, within 10 min).
    #[serde(rename = "lastDispatchFailure", skip_serializing_if = "Option::is_none")]
    pub last_dispatch_failure: Option<serde_json::Value>,
    /// Whether the agent prompt should include a fresh `diff-context`.
    #[serde(rename = "needsDiff")]
    pub needs_diff: bool,
    /// Whether the agent prompt should refresh the `context-slice`.
    #[serde(rename = "needsContextSlice")]
    pub needs_context_slice: bool,
    /// Model declared for the current wave (e.g. `"opus"` / `"sonnet"`).
    #[serde(rename = "waveModel", skip_serializing_if = "Option::is_none")]
    pub wave_model: Option<String>,
    /// First non-empty line of the `## Resumo` / `## Summary` section, capped.
    #[serde(rename = "specSummary")]
    pub spec_summary: String,
    /// Roles discovered for the current wave (e.g. `["ui"]`).
    #[serde(rename = "agentRoles")]
    pub agent_roles: Vec<String>,
}

/// Run `mustard-rt run resume-bootstrap`.
///
/// Fail-open: every step degrades to `null`/`false` on error; the process
/// always exits 0 and prints a JSON document on stdout.
pub fn run(spec: &str, json_flag: bool) {
    let project = PathBuf::from(project_dir());
    let spec_dir = project.join(".claude").join("spec").join(spec);

    let mut out = ResumeBootstrap {
        mode: "ask".to_string(),
        ..Default::default()
    };

    // --- Load pipeline state (fail-open: missing store → defaults preserved). ---
    let view: Option<PipelineStateView> = SqliteEventStore::for_project(&project)
        .ok()
        .and_then(|store| pipeline_state_for_spec(&store, spec, Some(&spec_dir)));

    // --- Detect wave-plan + total waves (event-first, FS fallback). ---
    let wave_plan_path = spec_dir.join("wave-plan.md");
    let has_wave_plan = wave_plan_path.exists();
    out.is_wave_plan = view
        .as_ref()
        .and_then(|v| v.is_wave_plan)
        .unwrap_or(has_wave_plan);

    if let Some(v) = view.as_ref() {
        out.current_wave = if out.is_wave_plan { v.current_wave } else { 0 };
        out.total_waves = if out.is_wave_plan {
            v.total_waves.unwrap_or(0)
        } else {
            0
        };
    } else if out.is_wave_plan {
        // No events yet, but a plan exists on disk — fall back to FS scan.
        let (current, total) = count_wave_progress_from_fs(&spec_dir);
        out.current_wave = current;
        out.total_waves = total;
    }

    if out.is_wave_plan {
        // Always cross-check against the FS: a wave-plan that grew after the
        // first `pipeline.scope` event was emitted will declare more waves
        // than the event remembers. Trust the larger of the two.
        let (_, fs_total) = count_wave_progress_from_fs(&spec_dir);
        if fs_total > out.total_waves {
            out.total_waves = fs_total;
        }
    }
    if out.is_wave_plan && out.current_wave == 0 {
        out.current_wave = 1;
    }

    // --- Resolve operational spec path. ---
    let op_path = if out.is_wave_plan {
        find_wave_spec_path(&spec_dir, out.current_wave)
            .unwrap_or_else(|| spec_dir.join("spec.md"))
    } else {
        spec_dir.join("spec.md")
    };
    if op_path.exists() {
        out.operational_spec_path = Some(relativize(&project, &op_path));
    }

    // --- Stage + stub detection from the operational spec head. ---
    let head = op_path
        .exists()
        .then(|| read_first_lines(&op_path, 30))
        .flatten()
        .unwrap_or_default();
    out.stage = detect_stage(&head, view.as_ref());
    out.is_stub = detect_stub(&head);

    // --- specSummary: first non-empty line of `## Resumo` / `## Summary`. ---
    let body = op_path
        .exists()
        .then(|| mfs::read_to_string(&op_path).ok())
        .flatten()
        .unwrap_or_default();
    out.spec_summary = extract_summary(&body);

    // --- waveModel from wave-plan.md table row of the current wave. ---
    if out.is_wave_plan && wave_plan_path.exists() {
        if let Ok(plan_text) = mfs::read_to_string(&wave_plan_path) {
            out.wave_model = extract_wave_model(&plan_text, out.current_wave);
        }
    }

    // --- agentRoles: derive from the wave subdir name (`wave-N-{role}`) when
    //     wave-plan; otherwise empty. ---
    if out.is_wave_plan {
        if let Some(role) = derive_role_from_wave_path(&op_path) {
            out.agent_roles.push(role);
        }
    }

    // --- lastDispatchFailure (already TTL-filtered by `pipeline_state_for_spec`). ---
    let dispatch_failure = view.as_ref().and_then(|v| v.last_dispatch_failure.clone());
    if let Some(fail) = dispatch_failure.as_ref() {
        out.last_dispatch_failure = Some(render_dispatch_failure(fail));
    }

    // --- needsDiff / needsContextSlice: any `pipeline.wave.complete` since the
    //     last `pipeline.resume_mode`? Same boolean for both. ---
    let (needs_refresh, last_resume_age_ms) = compute_needs_refresh(&project, spec);
    out.needs_diff = needs_refresh;
    out.needs_context_slice = needs_refresh;

    // --- Mode decision. ---
    out.mode = decide_mode(view.as_ref(), dispatch_failure.as_ref());

    // --- Emit `pipeline.resume_mode` (idempotent: skip if a fresh one exists). ---
    if last_resume_age_ms.unwrap_or(i64::MAX) > RESUME_MODE_DEBOUNCE_MS {
        emit_resume_mode(&project, spec, &out.mode);
    }

    // --- Output. ---
    if json_flag {
        let pretty = serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string());
        println!("{pretty}");
    } else {
        print_table(&out);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compact spec path relative to the project root (forward-slash separators).
fn relativize(project: &Path, abs: &Path) -> String {
    let stripped = abs.strip_prefix(project).unwrap_or(abs);
    stripped.to_string_lossy().replace('\\', "/")
}

/// Read up to the first `n` lines of a file. `None` on IO error.
fn read_first_lines(path: &Path, n: usize) -> Option<String> {
    let text = mfs::read_to_string(path).ok()?;
    let mut out = String::new();
    for (i, line) in text.lines().enumerate() {
        if i >= n {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}

/// Walk the spec dir for `wave-{N}-*/spec.md`. Returns the first match.
fn find_wave_spec_path(spec_dir: &Path, wave: u32) -> Option<PathBuf> {
    let entries = mfs::read_dir(spec_dir).ok()?;
    let prefix = format!("wave-{wave}-");
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        if !entry.file_name.starts_with(&prefix) {
            continue;
        }
        let candidate = entry.path.join("spec.md");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Best-effort FS-side (current, total) progress for a wave-plan when no
/// events are available. `current = done + 1` capped at `total`.
fn count_wave_progress_from_fs(spec_dir: &Path) -> (u32, u32) {
    let Ok(entries) = mfs::read_dir(spec_dir) else {
        return (0, 0);
    };
    let mut total: u32 = 0;
    let mut done: u32 = 0;
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let name = &entry.file_name;
        if !name.starts_with("wave-") {
            continue;
        }
        // Must be `wave-<digits>-...`.
        let after = &name[5..];
        let digits_end = after.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
        if digits_end == 0 || !after[digits_end..].starts_with('-') {
            continue;
        }
        total += 1;
        let spec_md = entry.path.join("spec.md");
        if let Some(head) = read_first_lines(&spec_md, 30) {
            let stage = parse_header_value(&head, "stage").unwrap_or_default();
            let outcome = parse_header_value(&head, "outcome").unwrap_or_default();
            if stage.eq_ignore_ascii_case("close") && outcome.eq_ignore_ascii_case("completed") {
                done += 1;
            }
        }
    }
    let current = (done + 1).min(total.max(1));
    (current, total)
}

/// Parse `### Key: value` from a header block.
fn parse_header_value(text: &str, key_lower: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("### ") else {
            continue;
        };
        let Some(colon) = rest.find(':') else {
            continue;
        };
        let k = rest[..colon].trim();
        if k.eq_ignore_ascii_case(key_lower) {
            let v = rest[colon + 1..].trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Detect the canonical stage word from the spec head, falling back to the
/// pipeline state view's `status`.
fn detect_stage(head: &str, view: Option<&PipelineStateView>) -> Option<String> {
    if let Some(stage) = parse_header_value(head, "stage") {
        return Some(normalise_stage(&stage));
    }
    if let Some(v) = view {
        if let Some(s) = v.status.as_deref() {
            return Some(normalise_stage(s));
        }
    }
    None
}

/// Map a stage/status spelling to the canonical PascalCase form.
fn normalise_stage(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "plan" | "planning" => "Plan".to_string(),
        "execute" | "implementing" => "Execute".to_string(),
        "analyze" | "analysing" | "analyzing" => "Analyze".to_string(),
        "qareview" | "qa-review" | "qa_review" | "reviewing" => "QaReview".to_string(),
        "close" | "closed" | "closed-followup" | "completed" => "Close".to_string(),
        other => {
            // Title-case fallback.
            let mut chars = other.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
            }
        }
    }
}

/// A stub is `Stage: Plan` with no `## Files`/`## Arquivos`/`## Tasks`/`## Tarefas`
/// section in the first ~30 lines.
fn detect_stub(head: &str) -> bool {
    let is_plan = parse_header_value(head, "stage")
        .map(|s| s.eq_ignore_ascii_case("plan"))
        .unwrap_or(false);
    if !is_plan {
        return false;
    }
    let has_files_or_tasks = head.lines().any(|l| {
        let t = l.trim_start();
        if !t.starts_with("## ") {
            return false;
        }
        let after = t.trim_start_matches('#').trim_start();
        let lower = after.to_lowercase();
        lower.starts_with("files")
            || lower.starts_with("arquivos")
            || lower.starts_with("tasks")
            || lower.starts_with("tarefas")
    });
    !has_files_or_tasks
}

/// Extract first non-empty line under `## Resumo` or `## Summary`, capped to
/// [`SUMMARY_CAP`] chars. Empty when neither heading exists.
fn extract_summary(body: &str) -> String {
    let mut in_section = false;
    for line in body.lines() {
        let trimmed = line.trim_end();
        if !in_section {
            let t = trimmed.trim_start();
            if t.starts_with("## ") {
                let after = t.trim_start_matches('#').trim();
                let lower = after.to_lowercase();
                if lower == "resumo" || lower == "summary" {
                    in_section = true;
                }
            }
            continue;
        }
        // We are inside the section — first non-empty line wins.
        if trimmed.trim().is_empty() {
            continue;
        }
        if trimmed.trim_start().starts_with("## ") {
            // Section ended before a content line — bail.
            return String::new();
        }
        let snippet: String = trimmed.trim().chars().take(SUMMARY_CAP).collect();
        return snippet;
    }
    String::new()
}

/// Pull the `Modelo` column for the given wave from a wave-plan table row.
///
/// The table shape (per the canonical wave-plan template) is:
///
/// `| Wave | Spec | Role | Modelo | Depende de | Resumo |`
///
/// We scan rows whose first cell parses as `<digits>` and match the wave
/// number. The model cell is the 4th data cell (index 3 after the empty
/// pre-`|` split entry).
fn extract_wave_model(plan_text: &str, wave: u32) -> Option<String> {
    for line in plan_text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = trimmed
            .trim_start_matches('|')
            .trim_end_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.len() < 4 {
            continue;
        }
        // First cell must be wave number.
        let label = cells[0]
            .trim_start_matches(['W', 'w'])
            .trim()
            .to_string();
        let Ok(n) = label.parse::<u32>() else {
            continue;
        };
        if n != wave {
            continue;
        }
        let model = cells[3].trim();
        if model.is_empty() || model == "—" || model == "-" {
            return None;
        }
        return Some(model.to_string());
    }
    None
}

/// Derive the role token from a wave spec path like
/// `.claude/spec/{name}/wave-{N}-{role}/spec.md`.
fn derive_role_from_wave_path(spec_path: &Path) -> Option<String> {
    let parent = spec_path.parent()?;
    let dir_name = parent.file_name()?.to_string_lossy();
    // Strip `wave-<digits>-` prefix.
    let rest = dir_name.strip_prefix("wave-")?;
    let digit_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
    if digit_end == 0 {
        return None;
    }
    let after = &rest[digit_end..];
    let role = after.strip_prefix('-')?;
    if role.is_empty() {
        return None;
    }
    Some(role.to_string())
}

/// Render the dispatch failure payload as JSON, including `ageMs`.
fn render_dispatch_failure(fail: &PipelineDispatchFailurePayload) -> serde_json::Value {
    let now_ms = i64::try_from(crate::util::now_millis()).unwrap_or(i64::MAX);
    let age_ms = fail
        .at
        .as_deref()
        .and_then(crate::run::complete_spec::parse_iso_millis)
        .map(|at_ms| now_ms - at_ms)
        .unwrap_or(0);
    json!({
        "at": fail.at.clone().unwrap_or_default(),
        "ageMs": age_ms,
        "agentType": fail.agent_type.clone().unwrap_or_default(),
        "description": fail.description.clone().unwrap_or_default(),
        "prompt": fail.prompt.clone().unwrap_or_default(),
    })
}

/// Returns `(needs_refresh, last_resume_mode_age_ms)`.
///
/// `needs_refresh` is `true` when at least one `pipeline.wave.complete` event
/// landed since the most recent `pipeline.resume_mode` event for this spec.
fn compute_needs_refresh(project: &Path, spec: &str) -> (bool, Option<i64>) {
    let Some(conn) = open_conn(project) else {
        return (false, None);
    };
    let now_ms = i64::try_from(crate::util::now_millis()).unwrap_or(i64::MAX);

    // Last `pipeline.resume_mode` ts (or NULL).
    let last_resume_ts: Option<String> = conn
        .query_row(
            "SELECT ts FROM events \
             WHERE event = ?1 AND spec = ?2 \
             ORDER BY id DESC LIMIT 1",
            rusqlite::params![EVENT_PIPELINE_RESUME_MODE, spec],
            |row| row.get(0),
        )
        .ok();
    let last_resume_ms = last_resume_ts
        .as_deref()
        .and_then(crate::run::complete_spec::parse_iso_millis);
    let last_resume_age = last_resume_ms.map(|ms| now_ms - ms);

    // Any wave-complete since the last resume_mode?
    let needs = match last_resume_ts.as_deref() {
        Some(ts) => conn
            .query_row(
                "SELECT 1 FROM events \
                 WHERE event = ?1 AND spec = ?2 AND ts > ?3 LIMIT 1",
                rusqlite::params![EVENT_PIPELINE_WAVE_COMPLETE, spec, ts],
                |_| Ok(()),
            )
            .is_ok(),
        None => conn
            .query_row(
                "SELECT 1 FROM events \
                 WHERE event = ?1 AND spec = ?2 LIMIT 1",
                rusqlite::params![EVENT_PIPELINE_WAVE_COMPLETE, spec],
                |_| Ok(()),
            )
            .is_ok(),
    };
    (needs, last_resume_age)
}

/// Open a raw rusqlite [`Connection`] to the project's harness DB. `None` on
/// any failure (fail-open).
fn open_conn(project: &Path) -> Option<Connection> {
    let store = SqliteEventStore::for_project(project).ok()?;
    let db_path = store.path().to_path_buf();
    let conn = Connection::open(&db_path).ok()?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(5_000));
    Some(conn)
}

/// Decide the resume mode from the view + dispatch failure state.
///
/// - `continued` — recent events, no dispatch failure, status is in-progress.
/// - `reanalyzed` — pipeline was abandoned (no events for a while) AND no
///   dispatch failure.
/// - `ask` — dispatch failure present OR no state at all.
fn decide_mode(
    view: Option<&PipelineStateView>,
    dispatch_failure: Option<&PipelineDispatchFailurePayload>,
) -> String {
    if dispatch_failure.is_some() {
        return "ask".to_string();
    }
    let Some(v) = view else {
        return "ask".to_string();
    };
    let last_ts = v
        .tasks
        .iter()
        .filter_map(|t| t.dispatched_at.clone())
        .max();
    let now_ms = i64::try_from(crate::util::now_millis()).unwrap_or(i64::MAX);
    let age_ms = last_ts
        .as_deref()
        .and_then(crate::run::complete_spec::parse_iso_millis)
        .map(|at| now_ms - at);
    match age_ms {
        Some(ms) if ms <= AUTO_CONTINUE_TTL_MS => "continued".to_string(),
        Some(_) => "reanalyzed".to_string(),
        // No task dispatch yet — orchestrator decides.
        None => "ask".to_string(),
    }
}

/// Emit a `pipeline.resume_mode` event (fail-open).
fn emit_resume_mode(project: &Path, spec: &str, mode: &str) {
    let Ok(store) = SqliteEventStore::for_project(project) else {
        return;
    };
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("resume-bootstrap".to_string()),
            actor_type: None,
        },
        event: EVENT_PIPELINE_RESUME_MODE.to_string(),
        payload: json!({ "mode": mode }),
        spec: Some(spec.to_string()),
    };
    let _ = store.append(&event);
}

/// Compact text-table fallback when `--json` is not requested.
fn print_table(out: &ResumeBootstrap) {
    println!("mode             : {}", out.mode);
    println!("stage            : {}", out.stage.clone().unwrap_or_else(|| "—".into()));
    println!(
        "operationalSpec  : {}",
        out.operational_spec_path.clone().unwrap_or_else(|| "—".into())
    );
    println!("isWavePlan       : {}", out.is_wave_plan);
    println!("currentWave      : {}", out.current_wave);
    println!("totalWaves       : {}", out.total_waves);
    println!("isStub           : {}", out.is_stub);
    let failure_str = match out.last_dispatch_failure.as_ref() {
        None => "(none)".to_string(),
        Some(v) => format!(
            "{} @ {}ms ago",
            v.get("agentType").and_then(|x| x.as_str()).unwrap_or("?"),
            v.get("ageMs").and_then(|x| x.as_i64()).unwrap_or(0)
        ),
    };
    println!("lastDispatchFail : {failure_str}");
    println!("needsDiff        : {}", out.needs_diff);
    println!("needsContextSlice: {}", out.needs_context_slice);
    println!(
        "waveModel        : {}",
        out.wave_model.clone().unwrap_or_else(|| "—".into())
    );
    println!("specSummary      : {}", out.spec_summary);
    println!("agentRoles       : {}", out.agent_roles.join(","));
}

// ---------------------------------------------------------------------------
// Reuse-friendly helpers also consumed by `agent_prompt_render`.
// ---------------------------------------------------------------------------

/// Resolve the operational spec path for a given spec + optional wave.
///
/// Mirrors the logic [`run`] uses internally so the prompt renderer can pick
/// the same file without re-deriving it.
#[must_use]
pub fn resolve_operational_spec_path(spec_dir: &Path, wave: Option<u32>) -> PathBuf {
    if let Some(w) = wave {
        if let Some(p) = find_wave_spec_path(spec_dir, w) {
            return p;
        }
    }
    spec_dir.join("spec.md")
}

/// Read `Modelo` from the wave-plan for the given wave (exported for the
/// renderer). Returns `None` when not a wave plan or no row matches.
#[must_use]
pub fn read_wave_model(spec_dir: &Path, wave: u32) -> Option<String> {
    let plan = spec_dir.join("wave-plan.md");
    let text = mfs::read_to_string(&plan).ok()?;
    extract_wave_model(&text, wave)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_stage_canonicalises_common_words() {
        assert_eq!(normalise_stage("Plan"), "Plan");
        assert_eq!(normalise_stage("execute"), "Execute");
        assert_eq!(normalise_stage("implementing"), "Execute");
        assert_eq!(normalise_stage("QaReview"), "QaReview");
        assert_eq!(normalise_stage("closed-followup"), "Close");
    }

    #[test]
    fn detect_stub_requires_plan_stage_and_no_files_tasks() {
        let stub = "### Stage: Plan\n### Outcome: Active\n\n## Resumo\n…\n";
        assert!(detect_stub(stub));
        let not_stub = "### Stage: Plan\n## Files\n- a.rs\n";
        assert!(!detect_stub(not_stub));
        let not_plan = "### Stage: Execute\n";
        assert!(!detect_stub(not_plan));
    }

    #[test]
    fn extract_summary_takes_first_non_empty_line() {
        let body = "# Title\n\n## Resumo\n\nFirst real line.\nSecond.\n\n## Network\n";
        assert_eq!(extract_summary(body), "First real line.");
    }

    #[test]
    fn extract_summary_handles_portuguese_and_english_headings() {
        let pt = "## Resumo\nlinha pt\n";
        let en = "## Summary\nen line\n";
        assert_eq!(extract_summary(pt), "linha pt");
        assert_eq!(extract_summary(en), "en line");
    }

    #[test]
    fn extract_wave_model_parses_canonical_table() {
        let plan = "\
| Wave | Spec | Role | Modelo | Depende de | Resumo |
|------|------|------|--------|------------|--------|
| 1 | [[wave-1-general]] | general | opus | — | foo |
| 2 | [[wave-2-ui]] | ui | sonnet | [[1]] | bar |
";
        assert_eq!(extract_wave_model(plan, 1).as_deref(), Some("opus"));
        assert_eq!(extract_wave_model(plan, 2).as_deref(), Some("sonnet"));
        assert_eq!(extract_wave_model(plan, 9), None);
    }

    #[test]
    fn derive_role_from_wave_path_works() {
        let p = Path::new("/x/.claude/spec/foo/wave-5-ui/spec.md");
        assert_eq!(derive_role_from_wave_path(p).as_deref(), Some("ui"));
        let p2 = Path::new("/x/.claude/spec/foo/spec.md");
        assert_eq!(derive_role_from_wave_path(p2), None);
    }

    #[test]
    fn resolve_operational_spec_path_uses_wave_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let wave_dir = dir.path().join("wave-5-ui");
        std::fs::create_dir_all(&wave_dir).unwrap();
        std::fs::write(wave_dir.join("spec.md"), "x").unwrap();
        std::fs::write(dir.path().join("spec.md"), "y").unwrap();
        let p = resolve_operational_spec_path(dir.path(), Some(5));
        assert!(p.ends_with("wave-5-ui/spec.md") || p.ends_with("wave-5-ui\\spec.md"));
        let q = resolve_operational_spec_path(dir.path(), None);
        assert!(q.ends_with("spec.md"));
        assert!(!q.to_string_lossy().contains("wave-5-ui"));
    }
}
