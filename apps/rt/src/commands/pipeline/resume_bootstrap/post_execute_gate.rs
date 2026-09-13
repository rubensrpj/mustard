//! Post-execute REVIEW/QA gate (2026-05-25 deep-refactor follow-up).
//!
//! When all waves are done (`currentWave >= totalWaves`) — or, in non-wave
//! mode, when stage is `Close` — the orchestrator must NOT freelance into
//! `pipeline.complete`. This module inspects the per-spec REVIEW + QA event
//! state and surfaces an explicit `nextAction` (with companion fields) on the
//! DTO. Fail-open: if the events dir is unreadable we take the conservative
//! path → `ReviewPending`.

use super::ResumeBootstrap;
use crate::shared::spec_state::DiskSpecState;
use mustard_core::domain::spec_state::{self, SpecState as _};
use mustard_core::io::fs as mfs;
use std::path::Path;

/// True when the spec has finished EXECUTE (all declared waves are done, or
/// the non-wave spec reached `Close` stage).
pub(super) fn execute_complete(out: &ResumeBootstrap) -> bool {
    if out.is_wave_plan {
        out.total_waves > 0 && out.current_wave >= out.total_waves
    } else {
        out.stage.as_deref() == Some("Close")
    }
}

/// O QA e a revisão da spec `spec`, lidos do `spec.ndjson` dela pela
/// interface [`SpecState`]: `(qa_pass, has_review, review_rejected)`.
///
/// - `qa_pass` — cada critério tem a última execução aprovada.
/// - `has_review` — alguma onda tem veredito.
/// - `review_rejected` — o último veredito de alguma onda reprovou: a
///   aprovação de uma onda não esconde a reprovação de outra, e a spec não
///   segue para o QA com uma reprovação sem resposta.
///
/// Sem arquivo de eventos, nada passou e nada foi revisto.
pub(crate) fn read_review_qa_state(project: &Path, spec: &str) -> (bool, bool, bool) {
    let Some(log) = DiskSpecState::new(project).log(spec) else {
        return (false, false, false);
    };
    let review = spec_state::review(&log);
    (spec_state::qa(&log).passed_all(), review.any, review.rejected)
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

/// The entry-into-Execute hard-gate. A Full-scope spec must NOT begin EXECUTE
/// without the approved state. This complements the write gate: the gate
/// blocks production edits, this blocks the resume engine from *advancing the
/// orchestrator into* Execute in the first place.
///
/// When the spec is Full scope, its resolved stage would put it at/after
/// Execute, and its state is not approved (`out.approved_by_user`, read once
/// from `spec.ndjson` by the caller), this rewrites the bootstrap back to a
/// `Plan` / `await-approval` signal so the orchestrator stops and asks for the
/// approval. Everything else is a no-op:
/// - non-Full specs (Light/Touch) — no PLAN approval gate;
/// - specs still in Plan/Analyze — not trying to execute yet;
/// - approved specs — the resume-after-approve path.
///
/// Fail-open: a missing/unreadable `meta.json` leaves `out` untouched (we
/// cannot prove the spec is an unapproved Full spec).
pub(super) fn block_unapproved_execute(spec_dir: &Path, out: &mut ResumeBootstrap) {
    // Resolve scope from the spec's meta.json (the single source of truth).
    // Not Full (or unreadable) → this gate is not its business.
    if full_scope_meta(spec_dir).is_none() {
        return;
    }

    // Only gate when the resolved stage is at/after Execute. A spec still in
    // Plan/Analyze has not tried to execute, so there is nothing to block.
    let stage = out.stage.as_deref().unwrap_or("");
    let executing = matches!(stage, "Execute" | "QaReview" | "ReviewPending" | "QaPending");
    if !executing {
        return;
    }

    if out.approved_by_user {
        return; // Resume-after-approve — proceed.
    }

    // Unapproved Full spec trying to execute → halt at the approval gate.
    out.stage = Some("Plan".to_string());
    out.next_action = Some("await-approval".to_string());
}

/// Invariant safety-net (2026-06-02-full-sempre-uma-wave): a **Full**-scope
/// spec must NOT begin EXECUTE without **≥1 wave**.
///
/// The invariant (encoded in
/// [`mustard_core::domain::spec::contract::ContractViolation::FullScopeNoWaves`])
/// is that every Full spec decomposes into a parent *orchestrator* doc plus at
/// least one executing *wave* subagent — there is no "Full with zero waves".
/// `spec-draft` already floors `total_waves` to 1 and `plan-materialize`
/// materialises the wave dirs, so a wave-less Full reaching Execute is a defect
/// (a hand-edited / legacy "limbo" spec). This gate exercises the invariant at
/// the resume/Execute boundary at runtime.
///
/// On violation it **BLOCKS** (it does NOT silently auto-scaffold — blocking is
/// explicit and surfaces operator action) and resets the bootstrap toward
/// `Plan` with an actionable `next_action` so the orchestrator runs
/// `plan-materialize` before Execute. The token names the PUBLISHED command:
/// `wave-scaffold` was absorbed into `plan-materialize` and no longer exists on
/// the CLI surface, so an obedient agent following the old token called nothing.
///
/// Wave evidence is read from `out` (already resolved from events + the FS
/// earlier in `run`): a wave-plan (`is_wave_plan`) OR `total_waves >= 1`. A
/// properly-decomposed Full — and the resume of an already-running Full (which
/// is, by definition, a wave plan) — therefore passes.
///
/// MUST NOT block: Light / Touch specs (no wave model at all); a decomposed
/// Full (`is_wave_plan` or `total_waves >= 1`); a Full still in Plan/Analyze
/// (not trying to execute yet). FAIL-OPEN: a missing/unreadable `meta.json`
/// leaves `out` untouched (we cannot prove it is a wave-less Full).
///
/// Runs BEFORE [`block_unapproved_execute`] is irrelevant to order — the two
/// gates are independent (approval vs decomposition); both reset toward Plan.
pub(super) fn block_full_without_wave(spec_dir: &Path, out: &mut ResumeBootstrap) {
    // Resolve scope from meta.json (single source of truth). Fail-open: an
    // unreadable meta — or a Light / Touch spec, which has no wave invariant —
    // means we cannot prove this is a wave-less Full → allow.
    let Some(meta) = full_scope_meta(spec_dir) else {
        return;
    };

    // Only gate when the resolved stage is at/after Execute. A spec still in
    // Plan/Analyze has not tried to execute, so there is nothing to block.
    let stage = out.stage.as_deref().unwrap_or("");
    let executing = matches!(stage, "Execute" | "QaReview" | "ReviewPending" | "QaPending");
    if !executing {
        return;
    }

    // Wave evidence: a wave-plan on disk / in events, or a declared total ≥ 1.
    // `meta.is_wave_plan` is the persisted flag; `out.*` is the live-resolved
    // view (events + FS). Either being positive means the Full was decomposed
    // (or is already running its waves) → allow.
    let has_wave = out.is_wave_plan
        || out.total_waves >= 1
        || meta.is_wave_plan == Some(true)
        || meta.total_waves.unwrap_or(0) >= 1;
    if has_wave {
        return; // Decomposed (or already-running) Full → proceed.
    }

    // Wave-less Full trying to execute → BLOCK and route back to decompose.
    out.stage = Some("Plan".to_string());
    out.next_action = Some("await-plan-materialize".to_string());
    out.spec_summary =
        "BLOCKED: Full scope requires ≥1 wave — decompose via plan-materialize before Execute"
            .to_string();
}

/// Read `<spec>/meta.json` and return it ONLY when it declares a **Full**-scope
/// spec (`scope` starts with `full` after a case-insensitive trim — `"full"` or
/// `"full (wave plan)"`).
///
/// The single home for "is this the Full-scope gate's business?", shared by the
/// three gates below so their scope test cannot drift. `None` means *not our
/// business* for two different reasons that call for the same answer: an
/// unreadable / absent `meta.json` (fail-open — we cannot prove anything) and a
/// Light / Touch spec (no plan-approval or wave invariant at all).
fn full_scope_meta(spec_dir: &Path) -> Option<mustard_core::Meta> {
    let meta = mustard_core::read_meta(&spec_dir.join("meta.json"))?;
    let is_full = meta
        .scope
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase().starts_with("full"))
        .unwrap_or(false);
    is_full.then_some(meta)
}

/// Name the step an APPROVED Full plan implies, instead of leaving the caller to
/// infer it.
///
/// The gap this closes: [`apply_post_execute_gate`] only speaks once EXECUTE is
/// complete, and the two blocking gates only speak at/after Execute. So a Full
/// spec that IS approved and is still resolved to `Plan` came back with
/// `stage: "Plan"`, `approvedByUser: true` and NO `nextAction` — and the caller
/// had to know, from a reference document, that this exact combination means
/// "do not re-present, do not re-approve, just start". A deterministic decision
/// delegated to a model is precisely what this binary exists to prevent, so the
/// state gets its own token in the same vocabulary as `await-approval` /
/// `await-plan-materialize`:
///
/// - waves materialised → `dispatch-wave`, plus [`ResumeBootstrap::dispatch_command`]
///   naming the PUBLISHED command that starts the round (`wave-advance`);
/// - no wave yet → `await-plan-materialize`, the existing token for exactly
///   that remedy (`plan-materialize`), so an approved-but-undecomposed Full is
///   not left silent either.
///
/// Advisory in effect — it only ever FILLS an empty `nextAction`, and never
/// rewrites `stage`. MUST NOT speak when: another gate already answered
/// (`next_action` is set); the spec is not resolved to `Plan`; the spec is not
/// Full; or no approval is on record. FAIL-OPEN: an unreadable `meta.json`
/// leaves `out` untouched.
pub(super) fn signal_approved_plan_ready(
    spec: &str,
    spec_dir: &Path,
    out: &mut ResumeBootstrap,
) {
    if out.next_action.is_some() {
        return; // A gate above already named the step — never overwrite it.
    }
    if out.stage.as_deref() != Some("Plan") {
        return;
    }
    let Some(meta) = full_scope_meta(spec_dir) else {
        return;
    };
    // Approved = the spec's approved state, already resolved onto `out`.
    if !out.approved_by_user {
        return;
    }

    // Same wave evidence `block_full_without_wave` reads: live-resolved view
    // (events + FS) or the persisted sidecar flags.
    let has_wave = out.is_wave_plan
        || out.total_waves >= 1
        || meta.is_wave_plan == Some(true)
        || meta.total_waves.unwrap_or(0) >= 1;
    if !has_wave {
        out.next_action = Some("await-plan-materialize".to_string());
        return;
    }
    out.next_action = Some("dispatch-wave".to_string());
    out.dispatch_command = Some(format!("mustard-rt run wave-advance --spec {spec}"));
}

/// Surface the post-execute next action on `out`. When `execute_complete` is
/// false this is a no-op — the orchestrator is still mid-execute and no signal
/// is needed.
pub(super) fn apply_post_execute_gate(
    project: &Path,
    spec: &str,
    spec_dir: &Path,
    out: &mut ResumeBootstrap,
) {
    if !execute_complete(out) {
        return;
    }
    // Read REVIEW + QA state from the spec's `spec.ndjson`.
    let (qa_pass, has_review, review_rejected) = read_review_qa_state(project, spec);

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

#[cfg(test)]
mod tests {
    use super::super::ResumeBootstrap;
    use super::*;

    use crate::shared::spec_state::{seed_runs, seed_verdict};

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

    /// A spec file with criteria and no verdict is still `ReviewPending`: only
    /// a recorded verdict advances past REVIEW.
    #[test]
    fn post_execute_gate_without_a_verdict_is_review_pending() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        std::fs::create_dir_all(spec_dir.join("wave-0-mixed")).unwrap();
        seed_runs(dir.path(), "demo", &[None]);

        let mut out = ResumeBootstrap {
            is_wave_plan: true,
            current_wave: 3,
            total_waves: 3,
            ..Default::default()
        };
        apply_post_execute_gate(dir.path(), "demo", spec_dir, &mut out);

        assert_eq!(out.stage.as_deref(), Some("ReviewPending"));
        assert_eq!(out.next_action.as_deref(), Some("dispatch-review"));
    }

    /// Approved REVIEW + no QA → `QaPending` + `run-qa` + qaCommand.
    #[test]
    fn post_execute_gate_signals_qa_pending_after_approved_review() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        let criteria = seed_runs(dir.path(), "demo", &[None]);
        seed_verdict(dir.path(), "demo", 1, "approved", criteria[0]);

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
        let criteria = seed_runs(dir.path(), "demo", &[Some("pass")]);
        seed_verdict(dir.path(), "demo", 1, "approved", criteria[0]);

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
        let criteria = seed_runs(dir.path(), "demo", &[None]);
        seed_verdict(dir.path(), "demo", 1, "rejected", criteria[0]);

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

    /// A rejected verdict of wave 2 recorded BEFORE an approved wave 1 is NOT
    /// masked by the later approval — the gate must still route back to
    /// REVIEW, not sail to QA.
    #[test]
    fn post_execute_gate_rejected_wave_not_masked_by_another_waves_approval() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        std::fs::create_dir_all(spec_dir.join("wave-0-rt")).unwrap();
        let criteria = seed_runs(dir.path(), "demo", &[None]);
        seed_verdict(dir.path(), "demo", 2, "rejected", criteria[0]);
        seed_verdict(dir.path(), "demo", 1, "approved", criteria[0]);
        let mut out = ResumeBootstrap {
            is_wave_plan: true,
            current_wave: 1,
            total_waves: 1,
            ..Default::default()
        };
        apply_post_execute_gate(dir.path(), "demo", spec_dir, &mut out);
        assert_eq!(out.stage.as_deref(), Some("ReviewPending"));
        assert_eq!(out.next_action.as_deref(), Some("dispatch-review"));
    }

    /// Once EVERY wave's LATEST verdict is approved (here wave 2 was rejected
    /// then fixed → approved last), the gate proceeds to QA.
    #[test]
    fn post_execute_gate_all_waves_approved_runs_qa() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        let criteria = seed_runs(dir.path(), "demo", &[None]);
        seed_verdict(dir.path(), "demo", 2, "rejected", criteria[0]);
        seed_verdict(dir.path(), "demo", 1, "approved", criteria[0]);
        seed_verdict(dir.path(), "demo", 2, "approved", criteria[0]);
        let mut out = ResumeBootstrap {
            is_wave_plan: true,
            current_wave: 1,
            total_waves: 1,
            ..Default::default()
        };
        apply_post_execute_gate(dir.path(), "demo", spec_dir, &mut out);
        assert_eq!(out.stage.as_deref(), Some("QaPending"));
        assert_eq!(out.next_action.as_deref(), Some("run-qa"));
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

    // --- The entry-into-Execute approval hard-gate ------------------------

    /// Seed the spec dir's `meta.json` with a scope.
    fn seed_meta_scope(spec_dir: &Path, scope: &str) {
        std::fs::create_dir_all(spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("meta.json"),
            format!("{{\"scope\":\"{scope}\",\"stage\":\"Plan\",\"outcome\":\"Active\"}}"),
        )
        .unwrap();
    }

    /// DENY: a Full spec resolved to Execute with no approval event is reset to
    /// `Plan` / `await-approval`.
    #[test]
    fn blocks_full_execute_without_approval() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        seed_meta_scope(spec_dir, "full (wave plan)");
        let mut out = ResumeBootstrap {
            stage: Some("Execute".to_string()),
            ..Default::default()
        };
        block_unapproved_execute(spec_dir, &mut out);
        assert_eq!(out.stage.as_deref(), Some("Plan"));
        assert_eq!(out.next_action.as_deref(), Some("await-approval"));
    }

    /// ALLOW: the approved state lets the Full spec proceed to Execute.
    #[test]
    fn allows_full_execute_with_approval() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        seed_meta_scope(spec_dir, "full");
        let mut out = ResumeBootstrap {
            stage: Some("Execute".to_string()),
            approved_by_user: true,
            ..Default::default()
        };
        block_unapproved_execute(spec_dir, &mut out);
        assert_eq!(out.stage.as_deref(), Some("Execute"));
        assert!(out.next_action.is_none());
    }

    /// ALLOW: a Light spec is never gated, even resolved to Execute.
    #[test]
    fn allows_light_execute_without_approval() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        seed_meta_scope(spec_dir, "light");
        let mut out = ResumeBootstrap {
            stage: Some("Execute".to_string()),
            ..Default::default()
        };
        block_unapproved_execute(spec_dir, &mut out);
        assert_eq!(out.stage.as_deref(), Some("Execute"));
    }

    /// ALLOW: a Full spec still in Plan is not yet executing → no-op.
    #[test]
    fn allows_full_still_in_plan() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        seed_meta_scope(spec_dir, "full");
        let mut out = ResumeBootstrap {
            stage: Some("Plan".to_string()),
            ..Default::default()
        };
        block_unapproved_execute(spec_dir, &mut out);
        assert_eq!(out.stage.as_deref(), Some("Plan"));
        assert!(out.next_action.is_none());
    }

    // --- Invariant safety-net: Full scope ⇒ ≥1 wave -----------------------

    /// DENY: a Full spec resolved to Execute with ZERO waves (no wave-plan,
    /// `total_waves == 0`) is reset to `Plan` / `await-plan-materialize` with
    /// the actionable BLOCKED message. The token and the message must name the
    /// PUBLISHED command — `wave-scaffold` was absorbed into `plan-materialize`
    /// and is not on the CLI surface.
    #[test]
    fn blocked_full_spec_awaits_plan_materialize() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        seed_meta_scope(spec_dir, "full");
        let mut out = ResumeBootstrap {
            stage: Some("Execute".to_string()),
            is_wave_plan: false,
            total_waves: 0,
            ..Default::default()
        };
        block_full_without_wave(spec_dir, &mut out);
        assert_eq!(out.stage.as_deref(), Some("Plan"));
        assert_eq!(out.next_action.as_deref(), Some("await-plan-materialize"));
        assert!(
            out.spec_summary.contains("BLOCKED")
                && out.spec_summary.contains("plan-materialize"),
            "block message must be actionable: {}",
            out.spec_summary
        );
        assert!(
            !out.spec_summary.contains("wave-scaffold"),
            "the message must not name the absorbed command: {}",
            out.spec_summary
        );
    }

    /// ALLOW: a decomposed Full (live-resolved `is_wave_plan` + `total_waves ≥
    /// 1`) proceeds to Execute — the invariant is satisfied.
    #[test]
    fn allows_decomposed_full_execute() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        seed_meta_scope(spec_dir, "full (wave plan)");
        let mut out = ResumeBootstrap {
            stage: Some("Execute".to_string()),
            is_wave_plan: true,
            total_waves: 1,
            ..Default::default()
        };
        block_full_without_wave(spec_dir, &mut out);
        assert_eq!(out.stage.as_deref(), Some("Execute"));
        assert!(out.next_action.is_none());
    }

    /// ALLOW: an already-running Full (wave plan with progress) is never
    /// blocked — it carries a wave plan by definition.
    #[test]
    fn allows_running_full_execute() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        seed_meta_scope(spec_dir, "full");
        let mut out = ResumeBootstrap {
            stage: Some("Execute".to_string()),
            is_wave_plan: true,
            current_wave: 2,
            total_waves: 4,
            ..Default::default()
        };
        block_full_without_wave(spec_dir, &mut out);
        assert_eq!(out.stage.as_deref(), Some("Execute"));
        assert!(out.next_action.is_none());
    }

    /// ALLOW: a Light spec is never gated, even resolved to Execute with no
    /// waves (Light has no wave model at all).
    #[test]
    fn allows_light_execute_without_wave() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        seed_meta_scope(spec_dir, "light");
        let mut out = ResumeBootstrap {
            stage: Some("Execute".to_string()),
            is_wave_plan: false,
            total_waves: 0,
            ..Default::default()
        };
        block_full_without_wave(spec_dir, &mut out);
        assert_eq!(out.stage.as_deref(), Some("Execute"));
        assert!(out.next_action.is_none());
    }

    /// ALLOW: a Full spec still in Plan is not executing → no-op (no block).
    #[test]
    fn allows_full_wave_gate_still_in_plan() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        seed_meta_scope(spec_dir, "full");
        let mut out = ResumeBootstrap {
            stage: Some("Plan".to_string()),
            is_wave_plan: false,
            total_waves: 0,
            ..Default::default()
        };
        block_full_without_wave(spec_dir, &mut out);
        assert_eq!(out.stage.as_deref(), Some("Plan"));
        assert!(out.next_action.is_none());
    }

    /// FAIL-OPEN: an unreadable / missing `meta.json` leaves `out` untouched —
    /// we cannot prove the spec is a wave-less Full, so we allow.
    #[test]
    fn wave_gate_fail_open_on_missing_meta() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path(); // no meta.json written
        let mut out = ResumeBootstrap {
            stage: Some("Execute".to_string()),
            is_wave_plan: false,
            total_waves: 0,
            ..Default::default()
        };
        block_full_without_wave(spec_dir, &mut out);
        assert_eq!(out.stage.as_deref(), Some("Execute"));
        assert!(out.next_action.is_none());
    }

    // --- Approved-but-not-started: `dispatch-wave` ------------------------

    /// An APPROVED Full spec still resolved to `Plan` now NAMES its next step.
    ///
    /// Asserts the new signal (`dispatch-wave` + the published command that
    /// implies) and that the old behaviour is gone: the same input used to
    /// return `nextAction: null`, leaving "just start" to be inferred from a
    /// reference document. The unapproved control proves the token is earned by
    /// the approval, not handed out to every Plan-stage Full.
    #[test]
    fn an_approved_plan_that_never_started_names_its_next_action() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        seed_meta_scope(spec_dir, "full (wave plan)");

        // Unapproved Full in Plan — untouched (the approval gate owns that).
        let mut unapproved = ResumeBootstrap {
            stage: Some("Plan".to_string()),
            is_wave_plan: true,
            total_waves: 3,
            ..Default::default()
        };
        signal_approved_plan_ready("demo", spec_dir, &mut unapproved);
        assert!(
            unapproved.next_action.is_none(),
            "an unapproved Full must not be told to dispatch"
        );

        // Approved (the state resolved onto `out`) + waves materialised.
        let mut out = ResumeBootstrap {
            stage: Some("Plan".to_string()),
            approved_by_user: true,
            is_wave_plan: true,
            total_waves: 3,
            ..Default::default()
        };
        signal_approved_plan_ready("demo", spec_dir, &mut out);
        assert_eq!(
            out.next_action.as_deref(),
            Some("dispatch-wave"),
            "the old `nextAction: null` for this state must be gone"
        );
        assert_eq!(
            out.dispatch_command.as_deref(),
            Some("mustard-rt run wave-advance --spec demo"),
            "the token must name the published command it implies"
        );
        // Advisory in effect: the stage is NOT rewritten, and no re-approval is
        // requested.
        assert_eq!(out.stage.as_deref(), Some("Plan"));
        assert_ne!(out.next_action.as_deref(), Some("await-approval"));
    }

    /// The new signal never overwrites a gate that already spoke, and an
    /// approved Full with NO wave is routed to decompose rather than dispatch.
    #[test]
    fn approved_plan_signal_yields_to_existing_gate_and_routes_waveless_full() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        seed_meta_scope(spec_dir, "full");

        // A gate above already answered → untouched.
        let mut spoken = ResumeBootstrap {
            stage: Some("Plan".to_string()),
            approved_by_user: true,
            is_wave_plan: true,
            total_waves: 2,
            next_action: Some("await-approval".to_string()),
            ..Default::default()
        };
        signal_approved_plan_ready("demo", spec_dir, &mut spoken);
        assert_eq!(spoken.next_action.as_deref(), Some("await-approval"));
        assert!(spoken.dispatch_command.is_none());

        // Approved Full with zero waves → decompose first, still explicit.
        let mut waveless = ResumeBootstrap {
            stage: Some("Plan".to_string()),
            approved_by_user: true,
            ..Default::default()
        };
        signal_approved_plan_ready("demo", spec_dir, &mut waveless);
        assert_eq!(
            waveless.next_action.as_deref(),
            Some("await-plan-materialize")
        );
        assert!(waveless.dispatch_command.is_none());
    }

    /// ALLOW: a Full meta that persisted `isWavePlan: true` / `totalWaves ≥ 1`
    /// is allowed even if the live-resolved `out.*` view is still default —
    /// the persisted flag is honoured as wave evidence.
    #[test]
    fn allows_full_with_persisted_wave_meta() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        std::fs::create_dir_all(spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("meta.json"),
            r#"{"scope":"full","stage":"Execute","outcome":"Active","isWavePlan":true,"totalWaves":1}"#,
        )
        .unwrap();
        let mut out = ResumeBootstrap {
            stage: Some("Execute".to_string()),
            is_wave_plan: false,
            total_waves: 0,
            ..Default::default()
        };
        block_full_without_wave(spec_dir, &mut out);
        assert_eq!(out.stage.as_deref(), Some("Execute"));
        assert!(out.next_action.is_none());
    }
}
