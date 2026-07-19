---
id: spec.field-report-fix-package-sialia
---

# Field-report fix package (sialia btw 2026-07-18), five fronts: (1) /spec picker 'ar' shortcut promises approve+execute inline but a Full plan structurally requires the observer-minted approval marker (ExitPlanMode/AskUserQuestion PostToolUse) plus the .clarified marker — reword the shortcut prose in plugin/commands/spec.md and refs/spec/resume-loop.md so 'r' means 'execute immediately after real approval', and make approve-spec report ALL missing gate requirements in one refusal instead of failing one gate at a time; (2) dispatch emits bare subagent_type strings (mustard-review) that the Agent tool rejects in consumer projects where plugin agents are namespaced mustard:mustard-review — apply the plugin namespace at the single resolver recommended_subagent_type in apps/rt/src/commands/agent/render/role.rs, normalize prefix-stripping in subagent_inject role_is_readonly (also add missing mustard-patterns there), sync the prose tables that hardcode the mapping; (3) qa-run judges an AC by exit code alone so a test command that runs ZERO tests passes green — add an optional Expect: evidence regex field to AcItem parsed from the AC block, pass requires exit 0 AND stdout/stderr match when declared, and extend analyze-validation weak-ac lint to WARN test-shaped ACs lacking Expect:; (4) wave traceability_gaps in wave_scaffold.rs only checks the plan's own acceptance lines so a parent-spec AC no wave satisfies slips through — extend defined set with the parent spec.md Acceptance Criteria ids and promote the coverage gap to a gated outcome with env mode; (5) submodule git boundary is detected (workspace is_git_repo_root, scan hollow_submodules, work_unit_open init_submodules) but never reaches the census Project model, DispatchItem/AdvanceItem, rendered agent prompts, or work_branch_gate — add an own-git-root flag to Project in packages/core/src/domain/scan.rs, thread it through dispatch_plan and wave_advance into agent-prompt-render output so implementer prompts state the boundary (separate commits, never bump the superproject gitlink), and make work_branch_gate resolve the work branch base from the submodule's own default branch when the edited file lives inside a nested git root

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

Field-report fix package (sialia btw 2026-07-18), five fronts: (1) /spec picker 'ar' shortcut promises approve+execute inline but a Full plan structurally requires the observer-minted approval marker (ExitPlanMode/AskUserQuestion PostToolUse) plus the .clarified marker — reword the shortcut prose in plugin/commands/spec.md and refs/spec/resume-loop.md so 'r' means 'execute immediately after real approval', and make approve-spec report ALL missing gate requirements in one refusal instead of failing one gate at a time; (2) dispatch emits bare subagent_type strings (mustard-review) that the Agent tool rejects in consumer projects where plugin agents are namespaced mustard:mustard-review — apply the plugin namespace at the single resolver recommended_subagent_type in apps/rt/src/commands/agent/render/role.rs, normalize prefix-stripping in subagent_inject role_is_readonly (also add missing mustard-patterns there), sync the prose tables that hardcode the mapping; (3) qa-run judges an AC by exit code alone so a test command that runs ZERO tests passes green — add an optional Expect: evidence regex field to AcItem parsed from the AC block, pass requires exit 0 AND stdout/stderr match when declared, and extend analyze-validation weak-ac lint to WARN test-shaped ACs lacking Expect:; (4) wave traceability_gaps in wave_scaffold.rs only checks the plan's own acceptance lines so a parent-spec AC no wave satisfies slips through — extend defined set with the parent spec.md Acceptance Criteria ids and promote the coverage gap to a gated outcome with env mode; (5) submodule git boundary is detected (workspace is_git_repo_root, scan hollow_submodules, work_unit_open init_submodules) but never reaches the census Project model, DispatchItem/AdvanceItem, rendered agent prompts, or work_branch_gate — add an own-git-root flag to Project in packages/core/src/domain/scan.rs, thread it through dispatch_plan and wave_advance into agent-prompt-render output so implementer prompts state the boundary (separate commits, never bump the superproject gitlink), and make work_branch_gate resolve the work branch base from the submodule's own default branch when the edited file lives inside a nested git root.

Anchors (from scan):
- apps/rt/src/hooks/observe/approval_marker_observer.rs (spec, approve, full, plan)
- packages/core/src/domain/model/pipeline.rs (package, execute, full, plan)
- apps/dashboard/src/hooks/useSpecWavesPlanned.ts (spec, plan)
- apps/dashboard/src-tauri/src/project_overview.rs (package, inline)
- apps/scan/src/classify.rs (requires, marker)
- apps/cli/build.rs (full)
- apps/mcp/src/lib.rs (report, spec)
- apps/rt/src/hooks/observe/plan_approval_observer.rs (spec, approve, full, plan)
- apps/rt/src/commands/pipeline/resume_bootstrap/post_execute_gate.rs (approve, execute, full, plan)
- apps/rt/src/hooks/write/scope_guard.rs (spec, approve, inline, full)
- apps/rt/src/registry.rs (approve, plan, observer, approval)
- apps/rt/src/shared/context.rs (spec, approve, approval, marker)

Recurring slices (precedent to mirror): Function+Report (×4), Error+Report (×2)

Why now: this package comes from a field report (a "btw" — a brutally honest usage review) written on 2026-07-18 while running a real feature on the sialia consumer project. Three of the five findings are recurrences of known debt that has bitten before (the submodule boundary is on its third report), and one of them — the Quality Assurance (QA) gate passing green while ZERO tests ran — is an integrity hole: a gate that is satisfied by vacuum gives false confidence, which is worse than having no gate. The dispatch-name bug (front 2) breaks every consumer-project wave dispatch today and must ship before the next field run.

## Users/Stakeholders

- Mustard maintainers (this repo): fronts 1-4 harden the gates they rely on when dogfooding.
- Consumer-project operators (e.g. sialia): front 2 unbreaks wave dispatch; front 5 stops the harness from being blind to the submodule (a git repository embedded inside another) they manage by hand today.
- The pipeline's own agents: front 5 tells an implementer agent, inside its prompt, that its subproject is a separate git repository — knowledge that today only the human has.

## Success Metric

None of the five field-report failures reproduce after the fix: (a) the picker prose no longer promises an approval bypass the binary refuses; (b) a wave dispatch in a consumer project succeeds with the namespaced agent type on the first try; (c) a test-shaped Acceptance Criteria (AC) command that runs zero tests FAILS the QA gate when evidence is declared; (d) a plan whose parent spec holds an AC that no wave satisfies is blocked at scaffold time; (e) a dispatched prompt for a submodule subproject states the git boundary. This spec is its own proof: its ACs use the new `Expect:` evidence field, and its wave plan must pass the new coverage gate.

## Non-Goals

- Fixing the sialia spec's own fragile AC-2 (false positive on a legitimate REST path) — that is authoring guidance in a consumer artifact, not tool behavior; the new `analyze-validation` warning nudges future authoring.
- A hard PreToolUse guard that blocks a superproject gitlink bump at commit time — the boundary text in prompts plus the existing `/git` per-repo prose cover the field case; a deterministic gitlink guard is a follow-up.
- Extending `work_branch_gate` to fully protect submodule edits the way the parent repo is protected (branch-on-first-edit inside the submodule) — this package only fixes the WRONG BASE it would use; full protection stays with `/git`'s create-branch-on-commit flow.
- Dashboard display strings that hardcode agent names (telemetry/i18n) — display-only, no dispatch behavior.
- Rewriting legacy specs' ACs to carry `Expect:` — the field is opt-in; old ACs keep exit-code semantics.

## Acceptance Criteria

- **AC-1** — when `approve-spec` runs in strict mode on a Full spec that is missing BOTH the `.clarified` and the `.approved-by-user` markers, then a SINGLE refusal message names both missing requirements and how each is minted (no more one-gate-at-a-time refusals); and when no session binding resolves, the approval observers fall back to the UNIQUE full/Plan/unapproved spec (fail-closed on zero or many)
  Command: `cargo test -p mustard-rt -- combined_refusal unique_pending`
  Expect: `test result: ok\. [1-9]\d* passed`
- **AC-2** — when `dispatch-plan` or `wave-advance` emits an item for a plugin-owned role (review/qa/guards/patterns), then its `subagent_type` carries the plugin namespace (`mustard:mustard-review`), while builtin types (Explore, Plan, general-purpose) stay bare; the read-only denylist in `subagent_inject` matches both spellings
  Command: `cargo test -p mustard-rt namespac`
  Expect: `test result: ok\. [1-9]\d* passed`
- **AC-3** — when an Acceptance Criteria item declares an `Expect:` regex and its command exits 0 WITHOUT the output matching, then `qa-run` marks that AC `fail` (vacuous green is dead); with a match it passes; without an `Expect:` line legacy exit-code semantics hold
  Command: `cargo test -p mustard-rt expect_regex`
  Expect: `test result: ok\. [1-9]\d* passed`
- **AC-4** — when the parent spec declares an AC id that no wave `satisfies` (nor covers via its `acceptance` lines), then `wave-scaffold` reports the gap and, in strict mode, refuses the scaffold (env `MUSTARD_TRACE_GATE_MODE=strict|warn|off`)
  Command: `cargo test -p mustard-rt parent_spec_ac`
  Expect: `test result: ok\. [1-9]\d* passed`
- **AC-5** — when the census marks a subproject as its own git root (a `.git` directory OR file at its dir), then the dispatched item carries the flag and the rendered implementer prompt states the boundary (separate commit history; never bump the superproject gitlink pointer), and `work_branch_gate` resolves the work-branch base from the nested repo's own default branch
  Command: `cargo test --workspace own_git_root`
  Expect: `test result: ok\. [1-9]\d* passed`
- **AC-6** — the whole workspace builds and tests green with the new gates active
  Command: `cargo test --workspace --quiet`
  Expect: `test result: ok`
- **AC-7** — when the plugin prose tables that map roles to agent types are read, then they carry the namespaced spelling
  Command: `rg -c "mustard:mustard-review" plugin/pipeline-config.md`
  Expect: `^[1-9]`

<!-- PLAN -->

## Files

- apps/rt/src/commands/spec/approve_spec.rs — aggregate the F6+T5 refusals into one message
- apps/rt/src/commands/agent/render/role.rs — the single subagent_type resolver gains the plugin namespace
- apps/rt/src/hooks/task/subagent_inject.rs — prefix-normalized read-only denylist (+ missing mustard-patterns)
- apps/rt/src/commands/pipeline/dispatch_plan.rs — tests/docs for namespaced emit; own_git_root on DispatchItem
- apps/rt/src/commands/pipeline/wave_advance.rs — same, on AdvanceItem
- apps/rt/src/commands/review/qa_run/mod.rs — AcItem.expect + `Expect:` parsing
- apps/rt/src/commands/review/qa_run/runner.rs — evidence match gates the pass branch
- apps/rt/src/commands/review/analyze_validation.rs — WARN test-shaped ACs lacking Expect:
- apps/rt/src/commands/wave/wave_scaffold.rs — traceability includes parent-spec ACs; gated outcome
- packages/core/src/domain/scan.rs — Project.own_git_root (serde default)
- packages/core/src/io/workspace.rs — expose the git-root helper for reuse
- apps/rt/src/commands/scan.rs — census writes the flag
- apps/rt/src/commands/agent/render/mod.rs — boundary block in rendered prompts
- apps/rt/src/hooks/write/work_branch_gate.rs — nested-git-root base resolution
- plugin/commands/spec.md — honest `r` shortcut prose
- plugin/refs/spec/resume-loop.md — aligned approval wording
- plugin/refs/agent-prompt/agent-prompt.md, plugin/pipeline-config.md, plugin/commands/task.md, plugin/commands/scan.md, plugin/commands/review.md, plugin/commands/qa.md, plugin/commands/feature.md, plugin/commands/bugfix.md — namespaced agent tables
- plugin/refs/feature/full-plan.md — AC schema documents `Expect:`; trace gate mode

## Boundaries

IN: the five field-report fronts above — refusal aggregation, agent-type namespace at the single resolver, opt-in AC evidence (`Expect:`), parent-spec AC coverage gate, git-boundary flag from census to dispatch/prompt/branch-gate — plus the prose sync and tests for each.
OUT: everything under Non-Goals (sialia's own AC wording, hard gitlink commit guard, full submodule edit protection, dashboard display strings, legacy AC backfill).

## Concerns

- [analyze-validation WARN, accepted] weak-ac on AC-6 (`cargo test --workspace --quiet` passes whether or not the feature exists). Kept deliberately as the whole-workspace regression ceiling: ACs 1-5 already assert each new behaviour with `Expect:` evidence, and AC-6 is what proves the package does not break the other 3,5k tests. Not the last AC, so the linter flagged it honestly — this note is the accepted-risk record.
- [FOLLOW-UP, out of scope] emitter-side session-bind race: `shared/events/route.rs` binds session→spec using the env-less newest-session-by-mtime fallback, which picked a DEAD session dir during this very pipeline (2026-07-18, bind landed on e467c08d… instead of the live session). Wave 1 removes the approval-path dependency on that bind (state-window fallback in the observers), but the emitter race still mis-attributes telemetry/events; fixing `route.rs` session resolution is a separate unit.
- [FINDING #7, folded into wave 2] live DAG-flattening bug: `wave_number_from_link` (dispatch_plan.rs) does not resolve the dotted wikilink `[[wave.<slug>.<N>-<role>]]` that `wave-scaffold` actually emits (it only knows `[[wave-N-role]]`), so on THIS pipeline every dependency edge dropped and `wave-advance` returned all 6 waves as one flat round. Proven live 2026-07-18. Because the running binary predates the fix, this run dispatches the real DAG by hand (waves 1-4, then 5, then 6); the fix in wave 2 repairs it for future runs and for sialia. Same family as the earlier role-form drop, different link shape.

<!-- wikilinks-footer-start -->
- [wave.<slug>.<N>-<role>](?) ⚠ unresolved
- [wave-N-role](?) ⚠ unresolved
<!-- wikilinks-footer-end -->