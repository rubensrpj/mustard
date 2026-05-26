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
    /// Canonical `Stage` word: `Plan` | `Execute` | `Analyze` | `QaReview` |
    /// `ReviewPending` | `QaPending` | `Close`.
    ///
    /// `ReviewPending` / `QaPending` are post-execute states surfaced when all
    /// waves are done but REVIEW or QA still has work — the orchestrator must
    /// dispatch the matching agent before emitting `pipeline.complete`. See
    /// `nextAction` for the explicit next step.
    pub stage: Option<String>,
    /// Operational spec path (root `spec.md` or `wave-N-{role}/spec.md`).
    #[serde(rename = "operationalSpecPath")]
    pub operational_spec_path: Option<String>,
    /// Whether the spec uses a wave plan.
    #[serde(rename = "isWavePlan")]
    pub is_wave_plan: bool,
    /// Current wave index (0-based, matching `wave-N-*` directory names).
    /// `0` when not a wave plan or when no waves have completed yet.
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
    /// **Explicit** next step the orchestrator must take. One of:
    /// `dispatch-review`, `run-qa`, `emit-complete`, or `null` (mid-execute).
    /// Pairs with [`Self::review_roles`] / [`Self::qa_command`] when relevant.
    ///
    /// This field is the canonical post-execute signal — when `nextAction` is
    /// non-null, the orchestrator must NOT freelance: do exactly what it says.
    #[serde(rename = "nextAction", skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
    /// Roles to dispatch REVIEW agents for. Populated when `nextAction ==
    /// "dispatch-review"`. Derived from the spec's `review/spec.md` (if
    /// present) or from the union of `wave-N-{role}` dirs.
    #[serde(rename = "reviewRoles", skip_serializing_if = "Vec::is_empty")]
    pub review_roles: Vec<String>,
    /// Shell-ready command to run QA. Populated when `nextAction == "run-qa"`.
    #[serde(rename = "qaCommand", skip_serializing_if = "Option::is_none")]
    pub qa_command: Option<String>,
}

/// Run `mustard-rt run resume-bootstrap`.
///
/// Fail-open: every step degrades to `null`/`false` on error; the process
/// always exits 0 and prints a JSON document on stdout.
pub fn run(spec: &str, json_flag: bool) {
    let project = PathBuf::from(project_dir());
    let spec_dir = project.join(".claude").join("spec").join(spec);

    // Emit a fresh `pipeline.scope` event so `current_spec` in subsequent
    // calls within the same session returns this spec (not a stale closed one).
    // Idempotent: last-write-wins; fail-open — a DB error must not block output.
    emit_scope_for_session(&project, spec);

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
        out.total_waves = if out.is_wave_plan {
            v.total_waves.unwrap_or(0)
        } else {
            0
        };
        if out.is_wave_plan {
            // Re-derive current wave 0-based from completed_waves instead of
            // trusting `v.current_wave` which defaults to 1 (1-based legacy).
            // 0 completed waves → wave 0 is current; N completed waves → wave N.
            out.current_wave = v.completed_waves.iter().max().map_or(0, |&m| m + 1);
        }
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
    // Note: wave directories are 0-based in Mustard (wave-0-*, wave-1-*, …).
    // When no events exist yet, current_wave stays 0 — do NOT bump to 1.

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

    // --- waveModel: wave-plan.md Modelo column → meta.json model → feature default. ---
    if out.is_wave_plan {
        // Try the wave-plan table first (when a "Modelo" column exists).
        let plan_model = wave_plan_path
            .exists()
            .then(|| mfs::read_to_string(&wave_plan_path).ok())
            .flatten()
            .and_then(|t| extract_wave_model(&t, out.current_wave));
        // Fall back to the parent spec's meta.json `model` field.
        let meta_model = plan_model.or_else(|| read_meta_model(&spec_dir));
        // Last resort: feature intent → opus.
        out.wave_model = Some(meta_model.unwrap_or_else(|| "opus".to_string()));
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

    // --- Post-execute REVIEW/QA gate (2026-05-25 deep-refactor follow-up). ---
    //
    // When all waves are done (currentWave >= totalWaves) — or, in non-wave
    // mode, when stage is Close — the orchestrator must NOT freelance into
    // `pipeline.complete`. Inspect REVIEW + QA event state and surface an
    // explicit `nextAction` (with companion fields). Fail-open: if the events
    // dir is unreadable, we take the conservative path → ReviewPending.
    apply_post_execute_gate(&project, spec, &spec_dir, &mut out);

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

/// True when the spec has finished EXECUTE (all declared waves are done, or
/// the non-wave spec reached `Close` stage).
fn execute_complete(out: &ResumeBootstrap) -> bool {
    if out.is_wave_plan {
        out.total_waves > 0 && out.current_wave >= out.total_waves
    } else {
        out.stage.as_deref() == Some("Close")
    }
}

/// Read the spec's per-spec NDJSON event log and return `(qa_pass, has_review,
/// review_rejected)`.
///
/// - `qa_pass` — last `qa.result` has `overall == "pass"`.
/// - `has_review` — at least one `review.result` event exists for the spec.
/// - `review_rejected` — the most recent `review.result` has
///   `verdict == "rejected"`.
fn read_review_qa_state(spec_dir: &Path) -> (bool, bool, bool) {
    let events_dir = spec_dir.join(".events");
    let mut events =
        mustard_core::projection::read_harness_events_from_ndjson_dir(&events_dir);
    events.sort_by(|a, b| a.ts.cmp(&b.ts));

    let mut last_qa_overall: Option<String> = None;
    let mut has_review = false;
    let mut last_review_verdict: Option<String> = None;
    for ev in &events {
        match ev.event.as_str() {
            "qa.result" => {
                last_qa_overall = ev
                    .payload
                    .get("overall")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
            }
            "review.result" => {
                has_review = true;
                last_review_verdict = ev
                    .payload
                    .get("verdict")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
            }
            _ => {}
        }
    }
    let qa_pass = last_qa_overall.as_deref() == Some("pass");
    let review_rejected = last_review_verdict.as_deref() == Some("rejected");
    (qa_pass, has_review, review_rejected)
}

/// Roles to dispatch REVIEW agents for. Order of preference:
/// 1. Roles declared in the spec's `review/spec.md` (if a `## Roles` section
///    exists) — out of scope for this wave; reserved for a future enhancement.
/// 2. The union of `wave-N-{role}` dir suffixes (deduplicated, sorted).
/// 3. A fallback `["mixed"]` when no waves declare a role.
fn derive_review_roles(spec_dir: &Path) -> Vec<String> {
    let Ok(entries) = mfs::read_dir(spec_dir) else {
        return vec!["mixed".to_string()];
    };
    let mut roles: Vec<String> = Vec::new();
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let name = &entry.file_name;
        let Some(rest) = name.strip_prefix("wave-") else {
            continue;
        };
        let digit_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
        if digit_end == 0 {
            continue;
        }
        let after = &rest[digit_end..];
        let Some(role) = after.strip_prefix('-') else {
            continue;
        };
        if role.is_empty() {
            continue;
        }
        if !roles.iter().any(|r| r == role) {
            roles.push(role.to_string());
        }
    }
    if roles.is_empty() {
        return vec!["mixed".to_string()];
    }
    roles.sort();
    roles
}

/// Surface the post-execute next action on `out`. When `execute_complete` is
/// false this is a no-op — the orchestrator is still mid-execute and no signal
/// is needed.
fn apply_post_execute_gate(
    _project: &Path,
    spec: &str,
    spec_dir: &Path,
    out: &mut ResumeBootstrap,
) {
    if !execute_complete(out) {
        return;
    }
    // Read REVIEW + QA state from the per-spec NDJSON log.
    let (qa_pass, has_review, review_rejected) = read_review_qa_state(spec_dir);

    if qa_pass {
        // Everything green — safe to close.
        out.stage = Some("Close".to_string());
        out.next_action = Some("emit-complete".to_string());
        return;
    }
    if has_review && !review_rejected {
        // REVIEW landed (and not rejected), but QA hasn't passed yet → run QA.
        out.stage = Some("QaPending".to_string());
        out.next_action = Some("run-qa".to_string());
        out.qa_command = Some(format!("mustard-rt run qa-run --spec {spec}"));
        return;
    }
    // No REVIEW yet, OR REVIEW was rejected → dispatch REVIEW agents.
    out.stage = Some("ReviewPending".to_string());
    out.next_action = Some("dispatch-review".to_string());
    out.review_roles = derive_review_roles(spec_dir);
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
    // Wave directories are 0-based: `current` is the first incomplete wave.
    // When nothing is done yet, current = 0; after N waves complete, current = N.
    let current = done.min(total.saturating_sub(1));
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
        .is_some_and(|s| s.eq_ignore_ascii_case("plan"));
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
        .map_or(0, |at_ms| now_ms - at_ms);
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

    // Last `pipeline.resume_mode` ts (or NULL). W5: lifecycle rows live in
    // `pipeline_events` (column `kind`).
    let last_resume_ts: Option<String> = conn
        .query_row(
            "SELECT ts FROM pipeline_events \
             WHERE kind = ?1 AND spec = ?2 \
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
                "SELECT 1 FROM pipeline_events \
                 WHERE kind = ?1 AND spec = ?2 AND ts > ?3 LIMIT 1",
                rusqlite::params![EVENT_PIPELINE_WAVE_COMPLETE, spec, ts],
                |_| Ok(()),
            )
            .is_ok(),
        None => conn
            .query_row(
                "SELECT 1 FROM pipeline_events \
                 WHERE kind = ?1 AND spec = ?2 LIMIT 1",
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
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
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
    println!(
        "nextAction       : {}",
        out.next_action.clone().unwrap_or_else(|| "—".into())
    );
    if !out.review_roles.is_empty() {
        println!("reviewRoles      : {}", out.review_roles.join(","));
    }
    if let Some(q) = out.qa_command.as_deref() {
        println!("qaCommand        : {q}");
    }
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

/// Emit a fresh `pipeline.scope` event for the resumed spec so
/// `last_pipeline_scope_for_session` returns this spec in subsequent calls
/// within the same Claude session (prevents stale closed-spec attribution).
///
/// Fail-open: any store error is silently discarded.
fn emit_scope_for_session(project: &Path, spec: &str) {
    let Ok(store) = SqliteEventStore::for_project(project) else {
        return;
    };
    let ts = now_iso8601();
    let sid = session_id();
    let _ = store.append_pipeline_event(
        &ts,
        Some(&sid),
        Some(spec),
        None,
        mustard_core::model::event::EVENT_PIPELINE_SCOPE,
        None,
        Some(r#"{"scope":"resumed"}"#),
    );
}

/// Read the `model` field from a spec directory's `meta.json`. Returns `None`
/// when the file is absent or the field is missing/empty.
fn read_meta_model(spec_dir: &Path) -> Option<String> {
    let text = mfs::read_to_string(spec_dir.join("meta.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let model = v.get("model")?.as_str()?;
    if model.is_empty() {
        return None;
    }
    Some(model.to_string())
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

    // -----------------------------------------------------------------------
    // Post-execute REVIEW/QA gate (2026-05-25 deep-refactor follow-up).
    // -----------------------------------------------------------------------

    /// Seed a `.events/<sid>.ndjson` line under the spec dir directly — bypasses
    /// the writer so tests stay hermetic.
    fn write_event_line(spec_dir: &Path, kind: &str, payload: &str, ts: &str) {
        let events_dir = spec_dir.join(".events");
        std::fs::create_dir_all(&events_dir).unwrap();
        let line = format!(
            "{{\"ts\":\"{ts}\",\"event\":\"{kind}\",\"kind\":\"qa\",\"spec\":\"demo\",\"payload\":{payload}}}\n"
        );
        let path = events_dir.join("test.ndjson");
        let prev = std::fs::read_to_string(&path).unwrap_or_default();
        std::fs::write(&path, prev + &line).unwrap();
    }

    /// `execute_complete` is `true` once `currentWave >= totalWaves` in a
    /// wave-plan spec.
    #[test]
    fn execute_complete_true_when_all_waves_done() {
        let mut out = ResumeBootstrap {
            is_wave_plan: true,
            current_wave: 13,
            total_waves: 13,
            ..Default::default()
        };
        assert!(execute_complete(&out));
        out.current_wave = 12;
        assert!(!execute_complete(&out));
    }

    /// All waves done + no events → `ReviewPending` + `dispatch-review` +
    /// reviewRoles derived from wave subdirs.
    #[test]
    fn post_execute_gate_signals_review_pending_when_no_events() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        // Two wave subdirs declaring `rt` and `cli` roles.
        std::fs::create_dir_all(spec_dir.join("wave-0-rt")).unwrap();
        std::fs::create_dir_all(spec_dir.join("wave-1-cli")).unwrap();

        let mut out = ResumeBootstrap {
            is_wave_plan: true,
            current_wave: 2,
            total_waves: 2,
            ..Default::default()
        };
        apply_post_execute_gate(dir.path(), "demo", spec_dir, &mut out);

        assert_eq!(out.stage.as_deref(), Some("ReviewPending"));
        assert_eq!(out.next_action.as_deref(), Some("dispatch-review"));
        assert_eq!(out.review_roles, vec!["cli".to_string(), "rt".to_string()]);
        assert!(out.qa_command.is_none());
    }

    /// Approved REVIEW + no QA → `QaPending` + `run-qa` + qaCommand.
    #[test]
    fn post_execute_gate_signals_qa_pending_after_approved_review() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        write_event_line(
            spec_dir,
            "review.result",
            r#"{"verdict":"approved","spec":"demo"}"#,
            "2026-05-25T10:00:00.000Z",
        );

        let mut out = ResumeBootstrap {
            is_wave_plan: true,
            current_wave: 5,
            total_waves: 5,
            ..Default::default()
        };
        apply_post_execute_gate(dir.path(), "demo", spec_dir, &mut out);

        assert_eq!(out.stage.as_deref(), Some("QaPending"));
        assert_eq!(out.next_action.as_deref(), Some("run-qa"));
        assert_eq!(
            out.qa_command.as_deref(),
            Some("mustard-rt run qa-run --spec demo")
        );
        assert!(out.review_roles.is_empty());
    }

    /// Passing QA → `Close` + `emit-complete`.
    #[test]
    fn post_execute_gate_allows_close_when_qa_passed() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        write_event_line(
            spec_dir,
            "review.result",
            r#"{"verdict":"approved","spec":"demo"}"#,
            "2026-05-25T10:00:00.000Z",
        );
        write_event_line(
            spec_dir,
            "qa.result",
            r#"{"overall":"pass","spec":"demo","criteria":[]}"#,
            "2026-05-25T10:05:00.000Z",
        );

        let mut out = ResumeBootstrap {
            is_wave_plan: true,
            current_wave: 5,
            total_waves: 5,
            ..Default::default()
        };
        apply_post_execute_gate(dir.path(), "demo", spec_dir, &mut out);

        assert_eq!(out.stage.as_deref(), Some("Close"));
        assert_eq!(out.next_action.as_deref(), Some("emit-complete"));
    }

    /// Rejected REVIEW (regardless of staleness) → `ReviewPending` again.
    #[test]
    fn post_execute_gate_returns_to_review_when_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        std::fs::create_dir_all(spec_dir.join("wave-0-mixed")).unwrap();
        write_event_line(
            spec_dir,
            "review.result",
            r#"{"verdict":"rejected","spec":"demo"}"#,
            "2026-05-25T10:00:00.000Z",
        );

        let mut out = ResumeBootstrap {
            is_wave_plan: true,
            current_wave: 1,
            total_waves: 1,
            ..Default::default()
        };
        apply_post_execute_gate(dir.path(), "demo", spec_dir, &mut out);

        assert_eq!(out.stage.as_deref(), Some("ReviewPending"));
        assert_eq!(out.next_action.as_deref(), Some("dispatch-review"));
        assert_eq!(out.review_roles, vec!["mixed".to_string()]);
    }

    /// Mid-execute (currentWave < totalWaves) → gate is a no-op; no nextAction.
    #[test]
    fn post_execute_gate_is_noop_mid_execute() {
        let dir = tempfile::tempdir().unwrap();
        let mut out = ResumeBootstrap {
            is_wave_plan: true,
            current_wave: 3,
            total_waves: 5,
            stage: Some("Execute".to_string()),
            ..Default::default()
        };
        apply_post_execute_gate(dir.path(), "demo", dir.path(), &mut out);
        assert!(out.next_action.is_none());
        assert_eq!(out.stage.as_deref(), Some("Execute"));
    }

    /// `derive_review_roles` falls back to `["mixed"]` when no wave dirs exist.
    #[test]
    fn derive_review_roles_falls_back_to_mixed() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(derive_review_roles(dir.path()), vec!["mixed".to_string()]);
    }
}
