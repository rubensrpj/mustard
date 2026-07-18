//! `mustard-rt run resume-bootstrap` — single-shot resume decision engine.
//!
//! Replaces Steps 0 + 0.5 + 1 + 5 + parts of Step 2 of the legacy
//! `resume-flow.md` ref (now `plugin/refs/spec/resume-loop.md § B`). One process call resolves: mode (`continued` |
//! `reanalyzed` | `ask`), spec stage, the operational spec path (root
//! `spec.md` or `wave-N-{role}/spec.md`), wave progress, stub flag, dispatch
//! failure replay, whether to refresh `diff` / `context-slice`,
//! a `## Resumo` one-liner, and the discovered agent roles. Emits a
//! `pipeline.resume_mode` event before returning (idempotent — skips if a
//! recent one already exists for the spec).
//!
//! ## Fail-open contract
//!
//! ANY IO error — missing spec dir, missing events dir, unparseable header —
//! degrades the affected field to `null`/`false`. The process never panics and
//! always exits 0; the orchestrator gets a partial JSON document instead of
//! an error.
//!
//! ## Module layout
//!
//! `run` coordinates the concerns; each lives in a focused submodule:
//! - [`stage_resolver`] — spec-head parsing (stage / stub / summary / objective).
//! - [`wave_progress`] — wave-plan FS reconnaissance + per-wave model/role.
//! - [`mode_decision`] — `continued`/`reanalyzed`/`ask` + refresh signal.
//! - [`dispatch_failure`] — dispatch-failure JSON rendering.
//! - [`post_execute_gate`] — REVIEW/QA gate + `nextAction`.
//! - [`context_loader`] — W6 disciplined context (prune + `_context.md`).
//! - [`event_emission`] — `pipeline.scope` / `pipeline.resume_mode` emission.

mod context_loader;
mod dispatch_failure;
mod event_emission;
mod mode_decision;
mod post_execute_gate;
mod stage_resolver;
mod wave_progress;

use crate::commands::event::event_projections::{pipeline_state_from_events, PipelineStateView};
use crate::shared::context::project_dir;
use mustard_core::domain::model::event::PipelineDispatchFailurePayload;
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs as mfs;
use mustard_core::view::projection::read_workspace_events;
use serde::Serialize;
use std::path::{Path, PathBuf};

// Re-exports so this module's `run` body (and the unit tests below, which use
// `super::*`) can call the extracted concerns module-qualified-free where it
// reads cleaner, and so cross-submodule shared helpers resolve via `super::`.
use context_loader::{generate_context_on_resume, load_pruned_prior_summaries, wikilinked_summary_targets};
use dispatch_failure::render_dispatch_failure;
use event_emission::{emit_resume_mode, emit_scope_for_session};
use mode_decision::{compute_needs_refresh, decide_mode};
use post_execute_gate::{apply_post_execute_gate, block_full_without_wave, block_unapproved_execute};
use stage_resolver::{
    detect_stage, detect_stub, extract_summary, read_first_lines, relativize,
};
use wave_progress::{
    count_wave_progress_from_fs, derive_role_from_wave_path, find_wave_spec_path,
};

/// Window inside which auto-continue applies (10 minutes since last event).
const AUTO_CONTINUE_TTL_MS: i64 = 10 * 60 * 1_000;
/// Window inside which a freshly emitted `pipeline.resume_mode` event suppresses
/// re-emission (idempotency — 10 seconds).
const RESUME_MODE_DEBOUNCE_MS: i64 = 10 * 1_000;
/// Cap on the `## Resumo` first-line snippet.
const SUMMARY_CAP: usize = 200;

/// Hard token budget for resume bootstrap context loading (Spec A v4 / W6 / AC-A-10).
///
/// `_summary.md` files from prior waves are pruned to this cap so the orchestrator
/// can start a resumed wave without the legacy ~60k-token bloat
/// ([[feedback_resume_flow_bloat]]).
pub const RESUME_TOKEN_BUDGET: usize = 10_000;

/// Priority assigned to the most recent prior wave summary (decays linearly
/// down to 1 for the oldest). Caller is responsible for sorting by this value
/// before calling [`prune_to_budget`]; higher = kept first.
const PRIORITY_BASE: u8 = 200;

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
    /// Spec A v4 / W6 — estimated token usage of the pruned prior-wave context
    /// loaded by this bootstrap. Bounded by [`RESUME_TOKEN_BUDGET`] (AC-A-10).
    /// `0` when no prior summaries were available (first wave, fresh spec).
    #[serde(rename = "tokensUsed")]
    pub tokens_used: usize,
    /// Spec A v4 / W6 — number of `_summary.md` files that fit inside the budget.
    /// Surfaces how aggressive the pruning was without spilling the whole list.
    #[serde(rename = "summariesLoaded")]
    pub summaries_loaded: usize,
    /// Spec A v4 / W6 — resolved path of the `_context.md` rendered for the
    /// current wave (relative to project root). `None` when generation was
    /// skipped (non-wave spec, first wave with no inheritance, or write error).
    #[serde(rename = "contextPath", skip_serializing_if = "Option::is_none")]
    pub context_path: Option<String>,
}

/// Run `mustard-rt run resume-bootstrap`.
///
/// Fail-open: every step degrades to `null`/`false` on error; the process
/// always exits 0 and prints a JSON document on stdout.
pub fn run(spec: &str, json_flag: bool) {
    let project = PathBuf::from(project_dir());
    // Fail-open: the I1 guard rejecting the root OR `spec` failing slug
    // validation folds to `compose_unchecked` inside the resolver, so the
    // spec-dir path always flows through the canonical accessor surface.
    let spec_dir = ClaudePaths::spec_dir_or_unchecked(&project, spec);

    // Emit a fresh `pipeline.scope` event so `current_spec` in subsequent
    // calls within the same session returns this spec (not a stale closed one).
    // Idempotent: last-write-wins; fail-open — a DB error must not block output.
    emit_scope_for_session(&project, spec);

    let mut out = ResumeBootstrap {
        mode: "ask".to_string(),
        ..Default::default()
    };

    // --- Load pipeline state (fail-open: missing events dir → defaults preserved). ---
    // W8A-1 (no-sqlite): the legacy event-store replay was replaced by the
    // NDJSON workspace walker. `pipeline_state_from_events` is unchanged —
    // same fold, different source.
    let events = read_workspace_events(&project);
    let view: Option<PipelineStateView> =
        pipeline_state_from_events(&events, spec, Some(&spec_dir));

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
            // Wave directories are 1-based (`wave-1-*` is the first wave; there
            // is no `wave-0-*`), matching `plan-from-spec` / `wave-scaffold`. So
            // the first wave to dispatch is 1, not 0: 0 completed → wave 1; last
            // completed wave M → wave M+1. This mirrors the projection's own
            // `current_wave` (max+1, default 1). The earlier 0-based re-derivation
            // pointed `operationalSpecPath` at `wave-0-*` (missing) on the very
            // first dispatch, silently falling back to the parent `spec.md`.
            out.current_wave = v.completed_waves.iter().max().map_or(1, |&m| m + 1);
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
    // Note: wave directories are 1-based in Mustard (wave-1-*, wave-2-*, …);
    // there is no wave-0. When no events exist yet, current_wave is the first
    // wave: 1.

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
    out.stage = detect_stage(&op_path, &head, view.as_ref());
    out.is_stub = detect_stub(&op_path, &head);

    // --- specSummary: first non-empty line of `## Resumo` / `## Summary`. ---
    let body = op_path
        .exists()
        .then(|| mfs::read_to_string(&op_path).ok())
        .flatten()
        .unwrap_or_default();
    out.spec_summary = extract_summary(&body);

    // --- agentRoles: derive from the wave subdir name (`wave-N-{role}`) when
    //     wave-plan; otherwise empty. ---
    if out.is_wave_plan {
        if let Some(role) = derive_role_from_wave_path(&op_path) {
            out.agent_roles.push(role);
        }
    }

    // --- lastDispatchFailure (already TTL-filtered by `pipeline_state_from_events`). ---
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

    // --- D5: entry-into-Execute approval hard-gate. ---
    //
    // A Full-scope spec must NOT begin EXECUTE without an explicit `/spec`
    // approval event. Runs BEFORE the post-execute gate so an unapproved Full
    // spec is reset to `Plan` / `await-approval` rather than being routed into
    // REVIEW/QA. Fail-open inside the helper. This is the resume-engine
    // complement to the `scope_guard` write hook (which blocks production
    // edits at PreToolUse).
    block_unapproved_execute(&spec_dir, &mut out);

    // --- Invariant safety-net: Full scope ⇒ ≥1 wave. ---
    //
    // A Full-scope spec must NOT begin EXECUTE without ≥1 wave (a wave-plan /
    // `total_waves >= 1` / a `wave-N-*` dir). `spec-draft` floors `total_waves`
    // to 1 and `wave-scaffold` materialises the waves, so a wave-less Full
    // reaching Execute is a defect (hand-edited / legacy "limbo"). This BLOCKS
    // (does NOT auto-scaffold) and resets toward Plan with an actionable
    // message, exercising `contract::FullScopeNoWaves` at runtime. Fail-open
    // inside the helper; independent of the approval gate above.
    block_full_without_wave(&spec_dir, &mut out);

    // --- Post-execute REVIEW/QA gate (2026-05-25 deep-refactor follow-up). ---
    //
    // When all waves are done (currentWave >= totalWaves) — or, in non-wave
    // mode, when stage is Close — the orchestrator must NOT freelance into
    // `pipeline.complete`. Inspect REVIEW + QA event state and surface an
    // explicit `nextAction` (with companion fields). Fail-open: if the events
    // dir is unreadable, we take the conservative path → ReviewPending.
    apply_post_execute_gate(&project, spec, &spec_dir, &mut out);

    // --- Spec A v4 / W6 — disciplined context load (AC-A-10). ---
    //
    // Read prior-wave `_summary.md` files, prune them to the
    // [`RESUME_TOKEN_BUDGET`] cap (T6.3) — but only the ones whose names appear
    // as wikilinks inside the operational wave spec (T6.4). Result: even a
    // 12-wave spec starts resume well below 10 000 tokens.
    if out.is_wave_plan {
        let op_body = op_path
            .exists()
            .then(|| mfs::read_to_string(&op_path).ok())
            .flatten()
            .unwrap_or_default();
        let allowed = wikilinked_summary_targets(&op_body);
        let (kept_texts, used_tokens, kept_count) = load_pruned_prior_summaries(
            &spec_dir,
            out.current_wave,
            allowed.as_ref(),
            &op_body,
            RESUME_TOKEN_BUDGET,
        );
        out.tokens_used = used_tokens;
        out.summaries_loaded = kept_count;

        // T6.5 — generate `_context.md` on resume. Inheritance is the same
        // wikilink set we just pruned (so the file the agent reads matches the
        // prefix we loaded). Fail-open: write errors leave `context_path = None`.
        if let Some(written) = generate_context_on_resume(
            &spec_dir,
            out.current_wave,
            &kept_texts,
            &op_body,
        ) {
            out.context_path = Some(relativize(&project, &written));
        }
    }

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
    // W6#3: surface the W6 budget metrics in the text-table form so callers
    // who don't pass `--json` still see how the budget was spent.
    println!("tokensUsed       : {}", out.tokens_used);
    println!("summariesLoaded  : {}", out.summaries_loaded);
    if let Some(p) = out.context_path.as_deref() {
        println!("contextPath      : {p}");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_stage_canonicalises_common_words() {
        use stage_resolver::normalise_stage;
        assert_eq!(normalise_stage("Plan"), "Plan");
        assert_eq!(normalise_stage("execute"), "Execute");
        assert_eq!(normalise_stage("implementing"), "Execute");
        assert_eq!(normalise_stage("QaReview"), "QaReview");
        assert_eq!(normalise_stage("closed-followup"), "Close");
    }

    #[test]
    fn detect_stub_requires_plan_stage_and_no_files_tasks() {
        // No meta.json beside this path → `detect_stage` falls back to the
        // legacy `### Stage:` header in the supplied `head` text.
        let no_meta = std::path::Path::new("/nonexistent/spec/path/spec.md");
        let stub = "### Stage: Plan\n### Outcome: Active\n\n## Resumo\n…\n";
        assert!(detect_stub(no_meta, stub));
        let not_stub = "### Stage: Plan\n## Files\n- a.rs\n";
        assert!(!detect_stub(no_meta, not_stub));
        let not_plan = "### Stage: Execute\n";
        assert!(!detect_stub(no_meta, not_plan));
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
