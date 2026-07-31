---
id: spec.ceremony-costs-more-than-gates
---

# ceremony costs more than the gates it guards

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

Getting one spec from "written" to "first agent dispatched" cost 14 harness calls in the session that produced this one. That number was counted, not estimated, and it splits three ways: five were operator error, two were gates that refused something real, and the remainder were pure ceremony — steps that cost a call without changing what the pipeline is allowed to do.

Two of them are worth removing, and they are removable for the same reason: in both cases the system already holds the information it then demands a second time.

The first is materialisation. Asked for two waves, the draft command records that it is a wave plan and how many waves there are — it has decided, and written the decision down — and then creates no wave directory at all. The layout appears only after the operator hand-writes a plan file and makes a second call. Everything that second call runs is already in-process: the wave renderer, the criteria-format validation, the negative proof, the phase emit. Nothing is missing except the wiring between the two.

The second is the approval gesture. The picker's `r` suffix means "approve and implement now", and the user types it themselves — yet the flow states explicitly that `r` pre-answers only the implement-now continuation and never the approval. So a user who has already typed their consent is routed through a plan-mode round trip to type it again. The approval marker exists to be unforgeable by the model, and it is minted by observers that read exactly that kind of act: the user's own answer to a question, or their acceptance of a plan. The text a user types is the same class of evidence — the model writes neither the prompt nor its content.

What is NOT ceremony, and stays untouched: the negative proof and the clarify marker. Both refused something real in the session that motivated this spec. The proof caught two criteria shaped as word searches that would have passed with the word written anywhere in the file; the clarify marker is what forces the glossary to be settled before execution. A refusal that catches defects is not a saving.

## Users/Stakeholders

The operator of this harness, on every full-scope spec — the cost is paid once per spec and it is paid at the worst moment, between deciding to work and starting to work.

## Success Metric

The path from intent to first dispatch loses the second materialisation call and the second approval gesture, and loses no refusal. A full-scope spec that today needs `spec-draft` + a hand-written plan + `plan-materialize` + a plan-mode acceptance needs `spec-draft --plan` + the `r` the user already typed.

## Non-Goals

- Removing, relaxing or making optional the negative proof (`ac-negative-check`) or the clarify marker (F6) — both are kept exactly as they are.
- Deleting `plan-materialize`: it stays as the RE-materialisation door, which is load-bearing when a plan is edited before approval.
- Any new configuration knob. The shorter path is the only path, not a mode.
- Changing how the two existing approval observers (AskUserQuestion, ExitPlanMode) behave.

## Acceptance Criteria

- **AC-1** — when `spec-draft` is given `--plan <file>`, then `spec.md`, `meta.json`, `wave-plan.md` and every wave directory are produced by that ONE call, with the negative proof taken in the same pass.
  Command: `cargo test -p mustard-rt spec_draft_materialises_the_whole_layout_in_one_call 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt --lib spec_draft 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
- **AC-2** — when the plan handed to `spec-draft --plan` carries a criterion that already passes against the current tree, then the call REFUSES and writes no layout — the negative proof keeps its blocking power on the fused path, exactly as it has on `plan-materialize`.
  Command: `cargo test -p mustard-rt spec_draft_plan_refuses_an_unproven_criterion 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt --lib plan_materialize 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
- **AC-3** — when the USER's own prompt is the picker's approve-and-implement form, then `<spec>/.approved-by-user` is minted with `via` naming the picker; and when the identical text is not the user's prompt, nothing is minted — both halves asserted, so the test can fail.
  Command: `cargo test -p mustard-rt picker_approval 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt approval_marker 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
- **AC-4** — when the flows are read by a test, the picker states that the typed `r` IS the approval and the materialisation is one call — asserted structurally, both halves (the new instruction present AND the superseded "r never approves" sentence gone).
  Command: `cargo test -p mustard-rt --test spec_flow_prose 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
  Control: `grep -q "pre-answers" plugin/commands/spec.md`
- **AC-5** — the project build and tests pass green
  Command: `cargo build --workspace`

<!-- PLAN -->

## Files

Wave 1 — rt (Rust):

- `apps/rt/src/commands/spec/spec_draft.rs` — gains `--plan <file>`; on that path it calls the same in-process composite `plan-materialize` runs (wave-scaffold renderer + `analyze-validation` + the negative proof + the `pipeline.scope`/PLAN emits) after writing `spec.md`/`meta.json`. A refusal from the proof leaves NO layout behind.
- `apps/rt/src/commands/pipeline/plan_materialize.rs` — the composite is extracted to a shared entry both commands call; its own published behaviour (re-materialisation, reconciling onto an edited plan) is unchanged.
- `apps/rt/src/hooks/observe/picker_approval_observer.rs` (create) — UserPromptSubmit observer minting `<spec>/.approved-by-user` with `via` naming the picker, when the user's own prompt is the approve-and-implement form and the active spec is a Full plan still awaiting approval. Mirrors `approval_marker_observer` and reuses `marker_body` / `approval_marker_path`.
- `apps/rt/src/hooks/observe/mod.rs` — register the observer.
- `apps/rt/src/commands/spec/active_specs.rs` — gains `spec_for_letter`, resolving a picker ROW LETTER through the SAME enumeration that rendered the table. Cascade, not scope creep: the letter is the only part of the gesture that says WHICH spec, so an observer that read only its shape would mint a genuine approval against whatever spec the session was bound to.

Wave 2 — plugin (prose) + its test:

- `plugin/commands/spec.md` — the picker's `r` becomes the approval, not a pre-answer to the continuation; the sentence stating it never approves goes.
- `plugin/refs/spec/resume-loop.md` — §A stops asking for a second gesture when the marker is already minted by the typed form.
- `plugin/refs/feature/full-plan.md` — step 2/3 become one call (`spec-draft --plan`), with `plan-materialize` named as the re-materialisation door.
- `apps/rt/tests/spec_flow_prose.rs` (create) — the structural test AC-4 names, both-halves style like `git_prose_rules.rs`.

## Boundaries

IN: the two removable ceremonies — one-call materialisation and the typed approval — plus the prose that instructs them.
OUT: the negative proof and the clarify marker (kept verbatim); deleting `plan-materialize`; any configuration knob; the AskUserQuestion and ExitPlanMode observers; the picker's letter-mode table rendering.

<!-- signals: layers,files -->

## Definitions

- **ceremony** — a step that costs a call without changing what the pipeline is allowed to do — as opposed to a gate, which can refuse
- **gate that paid** — a refusal that has demonstrably caught a real defect; the negative proof and the clarify marker both qualify and are kept
- **unforgeable gesture** — an act recorded in the transcript that the model cannot author — the user's own AskUserQuestion answer, the ExitPlanMode acceptance, or the literal text the user typed
- **one-call materialisation** — spec.md, meta.json, wave-plan.md and every wave dir produced by a single command invocation, with the negative proof taken in the same pass

## Decisions

- spec-draft gains --plan <file> and materialises the wave layout in the SAME call, running the wave-scaffold renderer, analyze-validation and the negative proof in-process — exactly the composite plan-materialize already performs
  Reason: measured this session: spec-draft --waves 2 wrote meta.json isWavePlan:true totalWaves:2 and created ZERO wave dirs; the layout only appeared after a hand-written plan.json and a second command. The command already knows the wave count — requiring a second call to act on what it recorded is the ceremony
- plan-materialize survives unchanged as the RE-materialisation door (reconciling a layout onto an edited plan before approval); spec-draft --plan is the first-materialisation door
  Reason: the composite is re-runnable by design and that behaviour is load-bearing for plan edits; folding it away would trade one ceremony for a regression
- A new UserPromptSubmit observer mints <spec>/.approved-by-user with via="picker" when the user's own prompt text is the picker's approve-and-implement form; the two existing observers (AskUserQuestion, ExitPlanMode) are untouched
  Reason: the marker's whole property is that it is born from an act the model cannot author. The user typing `/mustard:spec ar` is exactly such an act — the model writes neither the prompt nor its text — so the property is preserved, not weakened. What disappears is the SECOND gesture: today `r` explicitly pre-answers only the implement-now continuation, so a user who already typed their approval still gets a plan-mode round trip
- The negative proof (ac-negative-check) and the clarify marker (F6) stay exactly as they are
  Reason: both are gates that paid, in this very session: the proof caught two grep-shaped criteria that would have passed with the word written anywhere, and the clarify marker is what forces the glossary to be settled before EXEC. Cutting a refusal that catches defects is not a token saving
- No new configuration knob anywhere — the shorter path is the only path, not a mode
  Reason: a knob would double the surface the flows must describe and re-introduce the branch the operator has to think about, which is the cost being removed

## Evidence

- spec-draft records the wave decision and materialises none of it: `--waves 2` wrote meta.json `isWavePlan:true, totalWaves:2` while `created_files` held only spec.md and meta.json; the wave dirs appeared only after a separate plan-materialize
  Evidence: `apps/rt/src/commands/spec/spec_draft.rs:1`
- plan-materialize already composes everything the first materialisation needs — wave-scaffold renderer, analyze-validation, the negative proof and the PLAN emit — in-process; spec-draft calling the same composite is a wiring change, not a new engine
  Evidence: `apps/rt/src/commands/pipeline/plan_materialize.rs:1`
- the approval marker is minted by observers reading an act the model cannot author, and marker_body already records the PROVENANCE of the gesture (via) — a third provenance is the shape the design anticipates
  Evidence: `apps/rt/src/hooks/observe/approval_marker_observer.rs:386`
- the picker states that `r` pre-answers only the EXECUTE continuation and never the approval, so a user who typed `ar` is still routed through a plan-mode or AskUserQuestion round trip before anything runs
  Evidence: `plugin/commands/spec.md:1`
- measured end to end this session: 14 harness calls between the first spec write and the first dispatch; 5 were operator error, 2 were gates that paid, and the rest were the two-call materialisation and the second approval gesture
  Evidence: `plugin/refs/spec/resume-loop.md:1`