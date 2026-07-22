//! Post-execute REVIEW/QA gate (2026-05-25 deep-refactor follow-up).
//!
//! When all waves are done (`currentWave >= totalWaves`) — or, in non-wave
//! mode, when stage is `Close` — the orchestrator must NOT freelance into
//! `pipeline.complete`. This module inspects the per-spec REVIEW + QA event
//! state and surfaces an explicit `nextAction` (with companion fields) on the
//! DTO. Fail-open: if the events dir is unreadable we take the conservative
//! path → `ReviewPending`.

use super::ResumeBootstrap;
use mustard_core::io::fs as mfs;
use serde_json::Value;
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
        mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir);
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

/// D5 — the entry-into-Execute hard-gate. A Full-scope spec must NOT begin
/// EXECUTE without an explicit `/spec` approval event. This complements the
/// `scope_guard` write hook: the hook blocks production edits, this blocks the
/// resume engine from *advancing the orchestrator into* Execute in the first
/// place.
///
/// When the spec is Full scope, its resolved stage would put it at/after
/// Execute, and no `pipeline.status: approved` event exists, this rewrites the
/// bootstrap back to a `Plan` / `await-approval` signal so the orchestrator
/// stops and runs `/spec`. Everything else is a no-op:
/// - non-Full specs (Light/Touch) — no PLAN approval gate;
/// - specs still in Plan/Analyze — not trying to execute yet;
/// - specs with an approval event — the resume-after-approve path.
///
/// Fail-open: a missing/unreadable `meta.json` or events dir leaves `out`
/// untouched (we cannot prove the spec is an unapproved Full spec).
pub(super) fn block_unapproved_execute(spec_dir: &Path, out: &mut ResumeBootstrap) {
    // Resolve scope from the spec's meta.json (the single source of truth).
    let Some(meta) = mustard_core::read_meta(&spec_dir.join("meta.json")) else {
        return;
    };
    let is_full = meta
        .scope
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase().starts_with("full"))
        .unwrap_or(false);
    if !is_full {
        return;
    }

    // Only gate when the resolved stage is at/after Execute. A spec still in
    // Plan/Analyze has not tried to execute, so there is nothing to block.
    let stage = out.stage.as_deref().unwrap_or("");
    let executing = matches!(stage, "Execute" | "QaReview" | "ReviewPending" | "QaPending");
    if !executing {
        return;
    }

    if approval_event_present(spec_dir) {
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
    // unreadable meta means we cannot prove this is a wave-less Full → allow.
    let Some(meta) = mustard_core::read_meta(&spec_dir.join("meta.json")) else {
        return;
    };
    let is_full = meta
        .scope
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase().starts_with("full"))
        .unwrap_or(false);
    if !is_full {
        return; // Light / Touch — no wave invariant.
    }

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

/// `true` when the spec's per-spec NDJSON log carries a `pipeline.status` event
/// with `to == "approved"` — the canonical `/spec` approval signal (D5).
fn approval_event_present(spec_dir: &Path) -> bool {
    let events_dir = spec_dir.join(".events");
    let events =
        mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir);
    events.iter().any(|ev| {
        ev.event == "pipeline.status"
            && ev.payload.get("to").and_then(Value::as_str) == Some("approved")
    })
}

/// Surface the post-execute next action on `out`. When `execute_complete` is
/// false this is a no-op — the orchestrator is still mid-execute and no signal
/// is needed.
pub(super) fn apply_post_execute_gate(
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

#[cfg(test)]
mod tests {
    use super::super::ResumeBootstrap;
    use super::*;

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

    /// AC2 (regression): the events `/review` emits today — `review.start` +
    /// `review.complete`, but NO `review.result` — do NOT satisfy the gate.
    /// This reproduces the false-positive `ReviewPending` the fix targets: only
    /// a `review.result` verdict advances past REVIEW, so a review that finished
    /// without emitting one still (correctly) reports pending.
    #[test]
    fn post_execute_gate_review_start_complete_without_result_is_review_pending() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        std::fs::create_dir_all(spec_dir.join("wave-0-mixed")).unwrap();
        // The two events `/review` emits today — neither is a `review.result`.
        write_event_line(
            spec_dir,
            "review.start",
            r#"{"spec":"demo","target":"dev"}"#,
            "2026-05-25T10:00:00.000Z",
        );
        write_event_line(
            spec_dir,
            "review.complete",
            r#"{"spec":"demo","target":"dev"}"#,
            "2026-05-25T10:01:00.000Z",
        );

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

    // --- D5: entry-into-Execute approval hard-gate -------------------------

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

    /// ALLOW: an approval event lets the Full spec proceed to Execute.
    #[test]
    fn allows_full_execute_with_approval() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        seed_meta_scope(spec_dir, "full");
        write_event_line(
            spec_dir,
            "pipeline.status",
            r#"{"to":"approved","spec":"demo"}"#,
            "2026-06-02T09:00:00.000Z",
        );
        let mut out = ResumeBootstrap {
            stage: Some("Execute".to_string()),
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
