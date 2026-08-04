---
id: spec.work-unit-has-one-name
---

# Work unit has one name and the ok signals stop lying: insideWorkBranch compares the recorded branch instead of a rebuilt slug, the picker approval shortcut works from the bare letter the table asks for, the status column reads neverDispatched, and a declined precheck stops reusing the ok field

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

Work unit has one name and the ok signals stop lying: insideWorkBranch compares the recorded branch instead of a rebuilt slug, the picker approval shortcut works from the bare letter the table asks for, the status column reads neverDispatched, and a declined precheck stops reusing the ok field.

Why now: a field report over one full `/feature` run measured thirteen frictions. Four of them are defects in this repository's own code, and one of those four is the cause of the other three's worst symptom — a unit can carry TWO names at once. The base gate needs a name before the name exists (the slug is born one step later, in `spec-draft`), so the caller invents one; the draft then derives its own from its own intent. Nothing reconciles them.

That is not cosmetic. `resume-bootstrap` answers "are you inside your own unit?" by rebuilding `{base}_{slug}` from the SPEC's slug and comparing it to the checkout. With two names the answer is permanently no — measured live in the run that produced this spec, standing on the unit's own branch. The no-ceremony resume the loop documentation promises in capitals ("inside the unit's own branch the resume costs NOTHING") therefore never fires, and nothing says so. A feature switched off in silence is worse than one that errors.

The same divergence surfaced by a second route in the same run: `mustard-rt run notebook` resolved the slug from the BRANCH and pointed at a spec directory that does not exist.

Alongside it, three signals report success without having looked, or report a state nobody reached. Each is small; together they are the reason a reader has to open the source to know whether a green means anything.

## Users/Stakeholders

Whoever runs a Full-scope unit — they pay the ceremony the harness promised to skip, and they read a table that says work is running before anything was dispatched. Also the next reader of `mode_decision.rs`, whose docstring currently guarantees an invariant the code does not hold, and who would therefore stop checking.

## Success Metric

A Full unit opened through the base gate carries ONE name from the gate to the close: the branch, the spec directory, the events and the notebook all agree. `resume-bootstrap` reports `insideWorkBranch: true` from inside that branch, so the resume costs nothing exactly as documented. And no signal in the pipeline says `ok`/`em exec` for a state nobody reached.

## Non-Goals

- The worktree/spec-root dead end (report item 1), the `.NET`/`pnpm` acceptance-command shapes (item 4) and the submodule branch check (item 12). Each needs an environment this repository cannot stand up, and a fix nobody can demonstrate is a fix nobody should ship.
- Moving the wave-size audit earlier (item 13). The audit is correct and nothing ships wrong; acting on it sooner is an optimisation, and it would change what `plan-prepare` is answerable for.
- Loosening the approval observer so a bare letter mints the marker. The exact-form rule is what keeps the gesture unforgeable, and that property is worth more than the keystrokes it costs.
- Changing what `ok` means on an existing report. Consumers read it today; the distinction arrives as a new field beside it.
- The spec title being the whole `--intent` (report item 6). Real and cheap, but it changes the same command wave 1 is already reshaping, and stacking a second reason to touch `spec-draft` into one unit is how a focused change grows a second agenda. It is the first candidate for the next unit.
- The shared `.claude/.cache/spec-material.json` path (report item 7). Confirmed live — the file arrived carrying a previous spec's material. Left out for the same reason as item 6, and because the safe fix (a path per slug) only becomes obvious AFTER wave 1 settles what a unit's canonical name is.
- `spec-hygiene` assuming a modal question (report item 8). It is a preference conflict, not a defect: the step still has a correct answer, it just prescribes the wrong instrument for a user who has declined modals.

## Concerns

- **Report item 2 — the flow's order versus the protected-branch guard — did NOT reproduce here.** The report describes the first write of `.claude/.cache/spec-material.json` being refused because the checkout was still an integration base. In this run the same write triggered the auto-branch hook, which cut `dev_work-unit-has-one-name` in place and let the write through. Two runs, two behaviours. The wording fix in wave 3 (say that the base gate, and therefore the branch, precedes the material step) makes the order explicit either way, and costs nothing if the guard already handles it. What is NOT being claimed is that the dead end the report hit was reproduced.

## Acceptance Criteria

- **AC-1** — when the base gate opens a unit, then it mints the canonical slug itself and reports it, so the name the branch carries is the name the spec will carry
  Command: `cargo test -p mustard-rt the_base_gate_mints_the_canonical_slug 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt --test run_command_surface 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-2** — when `spec-draft` is given an explicit slug, then it uses that one instead of deriving a second name from its own intent
  Command: `cargo test -p mustard-rt spec_draft_consumes_the_slug_it_is_given 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt --lib commands::spec::spec_draft 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-3** — when the checkout IS the unit's branch and the slug was decided at the gate, then `insideWorkBranch` reports true, so the no-ceremony resume actually fires
  Command: `cargo test -p mustard-rt inside_work_branch_holds_when_the_gate_named_the_unit 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt --lib resume_bootstrap::mode_decision 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-4** — when a wave plan was scaffolded but nothing was dispatched, then the picker table does NOT read `em exec`, because that word asks the reader to resume work that never started
  Command: `cargo test -p mustard-rt a_scaffolded_plan_is_not_reported_as_running 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt --lib commands::spec::active_specs 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-5** — when the dependency precheck declines to judge, then its report says so in its own verdict field, instead of only in the presence of a second key
  Command: `cargo test -p mustard-rt a_declined_precheck_is_not_a_pass 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt --lib commands::review::dependency_precheck 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-6** — when the picker table is read, then it no longer claims a bare letter mints the approval marker, and it names the full form that does
  Command: `! grep -q 'the text you typed mints' plugin/commands/spec.md && grep -q 'typed in full' plugin/commands/spec.md`
  Control: `grep -q 'approved-by-user' plugin/commands/spec.md`
- **AC-7** — when the Full path is followed from `feature.md`, then the text sends the reader to the full-plan machinery BEFORE the census-dependent step, so the first `plan-prepare` is not guaranteed to abstain
  Command: `cargo test -p mustard-rt the_full_path_reaches_full_plan_before_the_census_step 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt --test spec_flow_prose 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-8** — the project build and tests pass green
  Command: `cargo build --workspace`

<!-- PLAN -->

## Files

- `apps/rt/src/commands/event/emit_pipeline.rs` — the gate mints the canonical slug from `--intent` and reports it, so the branch and the future spec share one derivation
- `apps/rt/src/commands/spec/spec_slug.rs` — the ONE derivation both callers share (cascade: "one derivation, two callers" cannot be honoured without it living somewhere neutral)
- `apps/rt/src/commands/event/work_branch.rs` — calls the shared derivation, and gains the `{base}_{slug}` inverse so a name can be recovered FROM a branch (cascade)
- `apps/rt/src/commands/spec/spec_draft.rs` — accepts an explicit slug and uses it instead of deriving a second one
- `apps/rt/src/commands/spec/cli.rs` — the new flag on the draft
- `apps/rt/tests/spec_draft_events_only_dir.rs`, `apps/rt/tests/spec_draft_context_prose.rs` — the draft's options struct gained a field (cascade)
- `apps/rt/src/commands/pipeline/resume_bootstrap/mode_decision.rs` — the false docstring corrected; the comparison proven against a gate-named unit
- `apps/rt/src/commands/spec/active_specs.rs` — the status column stops reading a scaffolded wave as a running one
- `apps/rt/src/commands/review/dependency_precheck.rs` — a declined judgement carries its own verdict beside `ok`
- `apps/rt/src/commands/pipeline/resume_bootstrap/wave_progress.rs`, `.../resume_bootstrap/mod.rs` — the dispatch witness widened so the picker asks the SAME list `neverDispatched` folds, rather than a second reading (cascade)
- `apps/rt/src/commands/pipeline/wave_advance.rs` — carries the new verdict through both branches that TRIM the precheck report, so a declined judgement is not flattened back into a bare `ok:true` on the way out (cascade)
- `plugin/commands/spec.md` — the picker stops promising that a bare letter mints the approval marker
- `plugin/commands/feature.md` — the Full path is sent to the full-plan machinery before the census-dependent step, and the `--plan` form is named
- `plugin/refs/spec/resume-loop.md` — the no-ceremony promise now holds, so its wording is checked against what the code does

## Boundaries

IN: one name per unit, minted at the gate and consumed by the draft; the corrected docstring; the status column; the declined-precheck verdict; the picker's approval prose; the Full-path ordering in the flow documents.
OUT: the worktree/spec-root dead end; the `.NET` and `pnpm` acceptance-command shapes; the submodule branch check; moving the wave-size audit earlier; loosening the approval observer's exact-form rule; changing the meaning of `ok` on any existing report.

## Definitions

- **unit name** — The single string that must name a work unit everywhere: the `{base}_{slug}` branch, the `.claude/spec/<slug>/` directory, the events, and the notebook. Today it is derived TWICE from two different inputs, so a unit can carry two names at once.
- **pending-work-branch marker** — `.claude/.session/<session>/pending-work-branch` — where `emit-pipeline --kind pipeline.kind` drops the branch name it computed, so the first Write/Edit can check that branch out. It is CONSUMED and deleted on that first edit, so it is not a durable record of the unit's branch.
- **declined check** — A gate that returned without judging — `dependency_precheck` on an unsupported stack. It ships `ok: true` with a `skipped` reason, i.e. the SAME success field a real pass uses.
- **picker two-step** — `/mustard:spec` with no argument renders the table and waits for the user to type a bare letter. The one-step form is `/mustard:spec ar` typed in full. Only the one-step form reaches the approval observer.
- **first active wave** — How `active_specs::derive_status` picks the number for the `W{N} em exec` column: the first `wave-N-*` directory whose meta says `Outcome=Active`. A wave directory is born Active at scaffold time, so this reads `em exec` before anything was dispatched.

## Decisions

- The unit's name is minted ONCE, at the base gate, and the draft consumes it — rather than teaching resume-bootstrap to compare differently
  Reason: The field report offered two fixes; only this one addresses the cause. Comparing against `a recorded branch` fails because the recorded branch lives in the pending-work-branch marker, which is deleted the moment the first edit consumes it (context.rs:475-498) — and because the emit's events are filed under the invented slug, so a spec under the real slug cannot find them. Two derivations from two inputs is the defect; one derivation is the fix.
- The docstring at mode_decision.rs:138 is FALSE and is corrected as part of the fix
  Reason: It claims `the name is not re-derived: compute_work_branch is the same function that minted the pending marker, so the two spellings cannot drift`. The FUNCTION is shared; the ARGUMENT is not. One call site passes the slug the orchestrator invented at the gate, the other passes the slug spec-draft derived from its own intent. A comment that guarantees an invariant the code does not hold is worse than no comment — it stops the next reader from checking.
- The picker's bare-letter promise is corrected in the PROSE, not by loosening the observer
  Reason: The observer requires the whole prompt to be `/mustard:spec ar` on purpose: a substring rule lets a sentence that merely QUOTES the form mint an approval, and that forgery already happened once on the AskUserQuestion door. The marker's entire value is being unforgeable. So the table stops promising that a bare letter approves, and names the form that does. Cheaper, and it does not trade the one property the approval gate rests on.
- The status column derives from whether anything was DISPATCHED, not from a wave directory being Active
  Reason: A wave directory is born `Outcome=Active` at scaffold time, so `first_active_wave` answers `W1 em exec` for a plan nobody has run. resume-loop.md already warns these two readings ask for OPPOSITE actions — start it versus resume it — and names `neverDispatched` as the signal to trust. The table is the surface where the user CHOOSES, so it is the one place that must not carry the misleading reading.
- The declined precheck gains a `verdict` field; `ok` keeps its current meaning
  Reason: dependency_precheck.rs:129-132 already documents that `checked and found nothing wrong` and `did not look` both ship as `ok: true` — it is a known trade-off, not an oversight. Changing `ok` itself would break every consumer that reads it. An additive `verdict: pass|declined` lets a new reader tell them apart without a flag day, and the existing `skipped` key keeps working.
- Items 1 (worktree), 4 (dotnet/pnpm AC shapes) and 12 (submodule branch) are OUT of this unit
  Reason: Each needs an environment this repository cannot reproduce: a worktree flow that was not exercised here, a .NET/pnpm workspace, and an active submodule. Fixing them from a repo that cannot demonstrate the failure is how a fix ships that nobody proved. They stay in the report for a unit that can stand them up.
- Item 13 (wave-size audit runs late) is OUT — it is an optimisation, not a defect
  Reason: The audit runs and its verdict is correct; the complaint is that acting on it at the approval gate costs more than acting at decomposition. True, but nothing ships wrong today, and moving an audit earlier changes what plan-prepare is responsible for. Not worth bundling with four real defects.

## Evidence

- inside_own_work_branch recomputes the branch name from the SPEC's slug and compares it to the checkout, so a unit whose branch was cut from a different string can never report itself inside its own branch
  Evidence: `apps/rt/src/commands/pipeline/resume_bootstrap/mode_decision.rs:144`
- The docstring right above it asserts the two spellings cannot drift because compute_work_branch is shared — true of the function, false of its argument, which is exactly what differs between the gate and the draft
  Evidence: `apps/rt/src/commands/pipeline/resume_bootstrap/mode_decision.rs:138`
- The branch name computed at the gate is stored in a SESSION marker that the first Write/Edit consumes and deletes, so there is no durable record of the unit's branch to compare against later
  Evidence: `apps/rt/src/shared/context.rs:481`
- Measured live this session: the checkout was dev_scan-stops-forcing-scripts and the spec was give-scan-flow-commands-it, so resume-bootstrap answered insideWorkBranch:false while standing inside the unit — the no-ceremony resume the docs promise never fired
  Evidence: `plugin/refs/spec/resume-loop.md:57`
- Measured live this session: `mustard-rt run notebook` resolved slug scan-stops-forcing-scripts from the BRANCH and pointed at .claude/spec/scan-stops-forcing-scripts/notebook.md, a directory that does not exist — the same divergence, reached by a second path
  Evidence: `apps/rt/src/commands/scan_patterns/../notebook.rs:1`
- The picker approval observer requires the WHOLE prompt to be `/mustard:spec <letter>r`, so the bare letter the two-step picker asks the user to type never reaches it
  Evidence: `apps/rt/src/hooks/observe/picker_approval_observer.rs:127`
- The picker's own selection block tells the user that typing `ar` mints the approval marker — false in the two-step flow, which is the flow the table itself opens
  Evidence: `plugin/commands/spec.md:30`
- derive_status returns `W{N} em exec` for the first wave directory marked Outcome=Active, and a scaffolded wave is Active before any dispatch — measured live: the table showed `0/2` and `W1 em exec` for a plan resume-bootstrap reported as neverDispatched:true
  Evidence: `apps/rt/src/commands/spec/active_specs.rs:1007`
- dependency_precheck documents that a declined judgement and a real pass both ship as ok:true, with only the presence of the `skipped` key telling them apart
  Evidence: `apps/rt/src/commands/review/dependency_precheck.rs:129`
- feature.md orders plan-prepare immediately after the draft, but the ## Files census it reads is only authored later in full-plan.md step 2 — so the first call returns scope:abstain with filesSectionEmpty:true every time on the full path
  Evidence: `plugin/commands/feature.md:1`
- feature.md describes spec-draft WITHOUT --plan while full-plan.md step 2 states the correct materialisation is spec-draft --plan in ONE call, which routes anyone following the first document into plan-materialize — the door the same file classifies as the EDIT door
  Evidence: `plugin/refs/feature/full-plan.md:33`