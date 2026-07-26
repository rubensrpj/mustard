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
/// - `review_rejected` — ANY subproject's most recent `review.result` has
///   `verdict == "rejected"` (grouped by the payload `subproject`; absent/null
///   → `"."`). Per-subproject, NOT a single global-latest verdict: a rejected
///   review of subproject B reviewed before an approved A must still report a
///   rejection, otherwise a later approval of one subproject would mask an
///   earlier rejection of another and the spec would sail into QA/CLOSE with an
///   unaddressed rejection. The untagged `.` (whole-project) group — the 1a
///   SubagentStop hook records it per review return, alongside the authoritative
///   `review-result --subproject` records — is ignored when real
///   subproject-tagged reviews exist (it is hook noise then, mirroring
///   `wave_advance` which ignores `.` as never-touched), and honored only as the
///   SOLE group (a genuine root/whole-project review).
fn read_review_qa_state(spec_dir: &Path) -> (bool, bool, bool) {
    let events_dir = spec_dir.join(".events");
    let mut events =
        mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir);
    events.sort_by(|a, b| a.ts.cmp(&b.ts));

    let mut last_qa_overall: Option<String> = None;
    let mut has_review = false;
    // Latest verdict per subproject (later `ts` overwrites, events are sorted).
    let mut latest_verdict_by_sub: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
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
                let sub = ev
                    .payload
                    .get("subproject")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(".")
                    .to_string();
                if let Some(verdict) = ev.payload.get("verdict").and_then(|v| v.as_str()) {
                    latest_verdict_by_sub.insert(sub, verdict.to_string());
                }
            }
            _ => {}
        }
    }
    let qa_pass = last_qa_overall.as_deref() == Some("pass");
    // When real (subproject-tagged) reviews exist, the untagged `.` group is the
    // 1a hook's per-return noise — exclude it (mirror `wave_advance`, which never
    // treats `.` as a touched subproject). `.` counts only as the sole group.
    let has_real_sub = latest_verdict_by_sub.keys().any(|k| k != ".");
    let review_rejected = latest_verdict_by_sub
        .iter()
        .filter(|(sub, _)| !(has_real_sub && sub.as_str() == "."))
        .any(|(_, v)| v == "rejected");
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
    // Approved = the user's own marker (`<spec>/.approved-by-user`, already
    // resolved onto `out`) OR the emitted `draft→approved` signal. Either proves
    // the approval gesture happened; requiring both would re-refuse a spec
    // approved through the other door.
    if !(out.approved_by_user || approval_event_present(spec_dir)) {
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

    /// Pilar 1b (per-subproject): a rejected review of subproject `b` reviewed
    /// BEFORE an approved `a` is NOT masked by the later approval — the gate must
    /// still route back to REVIEW, not sail to QA. The old global-latest check
    /// saw `a: approved` last and wrongly proceeded.
    #[test]
    fn post_execute_gate_rejected_subproject_not_masked_by_later_approval() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        std::fs::create_dir_all(spec_dir.join("wave-0-rt")).unwrap();
        write_event_line(
            spec_dir,
            "review.result",
            r#"{"verdict":"rejected","subproject":"b","spec":"demo"}"#,
            "2026-05-25T10:00:00.000Z",
        );
        write_event_line(
            spec_dir,
            "review.result",
            r#"{"verdict":"approved","subproject":"a","spec":"demo"}"#,
            "2026-05-25T10:01:00.000Z",
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
    }

    /// Per-subproject: once EVERY subproject's LATEST review is approved (here
    /// `b` was rejected then fixed → approved last), the gate proceeds to QA.
    #[test]
    fn post_execute_gate_all_subprojects_approved_runs_qa() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        write_event_line(
            spec_dir,
            "review.result",
            r#"{"verdict":"rejected","subproject":"b","spec":"demo"}"#,
            "2026-05-25T10:00:00.000Z",
        );
        write_event_line(
            spec_dir,
            "review.result",
            r#"{"verdict":"approved","subproject":"a","spec":"demo"}"#,
            "2026-05-25T10:01:00.000Z",
        );
        write_event_line(
            spec_dir,
            "review.result",
            r#"{"verdict":"approved","subproject":"b","spec":"demo"}"#,
            "2026-05-25T10:02:00.000Z",
        );
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

    /// Pilar 1b — the untagged `.` (whole-project) record the 1a hook writes per
    /// review return must NOT block QA when every real subproject-tagged review
    /// is approved. A stale `.`=rejected (e.g. a missed final re-review) is hook
    /// noise here, ignored — mirroring `wave_advance`.
    #[test]
    fn post_execute_gate_ignores_dot_hook_noise_when_real_subs_approved() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        write_event_line(
            spec_dir,
            "review.result",
            r#"{"verdict":"approved","subproject":"a","spec":"demo"}"#,
            "2026-05-25T10:00:00.000Z",
        );
        write_event_line(
            spec_dir,
            "review.result",
            r#"{"verdict":"approved","subproject":"b","spec":"demo"}"#,
            "2026-05-25T10:01:00.000Z",
        );
        // A hook `.` record (no subproject) is rejected — noise, must be ignored.
        write_event_line(
            spec_dir,
            "review.result",
            r#"{"verdict":"rejected","spec":"demo"}"#,
            "2026-05-25T10:02:00.000Z",
        );
        let mut out = ResumeBootstrap {
            is_wave_plan: true,
            current_wave: 1,
            total_waves: 1,
            ..Default::default()
        };
        apply_post_execute_gate(dir.path(), "demo", spec_dir, &mut out);
        assert_eq!(
            out.stage.as_deref(),
            Some("QaPending"),
            "the '.' hook noise must not block QA when real subprojects are approved"
        );
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

        // Approved (user marker resolved onto `out`) + waves materialised.
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
