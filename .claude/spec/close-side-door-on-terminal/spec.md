---
id: spec.close-side-door-on-terminal
---

# Close the side door on the terminal close: complete-spec must consult the recorded QA verdict the shipped ritual already promises

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

The harness has one gate it treats as final: nothing may be declared finished
without a green verification. In practice that means the event which marks a
spec complete is refused unless a recorded verification says it passed.

That refusal exists, and it works — but only on one of the two doors that write
that event. The command an operator would reach for to close a spec by hand
writes the event straight to the log, consulting nothing. The gate is upheld not
by the code but by the ORDER in which a larger command happens to call its
steps: it runs the verification first, and only calls the closer when the
verification passed. Anyone who calls the closer directly walks past it.

What turns this from an oversight into the more serious kind of defect is the
shipped instruction. The close ritual tells its reader never to hand-call the
closer to get past a red verification, and explains why: the other command's
gate rejects it anyway. It does not. The closer never goes through that command.
So the documentation asserts a protection that does not exist, which is worse
than a missing gate — a missing gate invites caution, while a promised one
invites the opposite.

This was reproduced with the installed binary, not inferred: a spec whose only
attempted criterion was the trailing build-green net closed with a verdict of
"skip", and the completion event was written anyway, with no verdict recorded at
all. Both closures performed in the session that found this used exactly that
door.

The intended contract is therefore already written down. What is missing is the
code that honours it.

## Users/Stakeholders

The operator, who is told a protection is in place and would reasonably rely on
it. Anyone reading a spec's history later, for whom a completion event is the
claim that the work was verified. And the closing gate itself, whose strictness
on one door means little while the other stands open.

## Success Metric

The completion event cannot be written by any door without a recorded passing
verification, and the shipped ritual describes the protection that actually
enforces it rather than one that does not.

## Non-Goals

Changing what the verification itself checks, or how it decides its verdict —
that shipped in the previous unit and is untouched here. Gating the tail the
composite close commands reuse: those already ran the verification and gated on
it, so a second gate there would only re-execute every criterion. Turning an
already-finished close into a failure: it writes no event, so there is nothing
to gate, and the documented hygiene sweep depends on it staying a silent no-op.
Adding any switch that relaxes the new refusal.

## Acceptance Criteria

- **AC-1** — when a terminal close is asked for and no recorded verdict says the verification passed, then the close is refused and no completion event is written.
  Command: `cargo test -p mustard-rt --lib complete_spec::tests::a_close_with_no_recorded_pass_is_refused`
  Expect: `[1-9][0-9]* passed`
- **AC-2** — when the spec is already finished, or when a recorded verdict does say it passed, then the close is admitted — so the idempotent no-op and the documented hygiene sweep keep working.
  Command: `cargo test -p mustard-rt --lib complete_spec::tests::an_already_finished_close_and_a_proven_one_are_both_admitted`
  Expect: `[1-9][0-9]* passed`
- **AC-3** — when the shipped close ritual is read, then it names the protection that actually enforces the refusal instead of promising another command's gate.
  Command: `cargo test -p mustard-rt --lib complete_spec::tests::the_shipped_ritual_names_the_protection_that_exists`
  Expect: `[1-9][0-9]* passed`
- **AC-5** — when the composite close asks its verification gate, then a run that verified nothing does not open the close, and a recorded pass does.
  Command: `cargo test -p mustard-rt --lib close_orchestrate::tests::a_skip_verdict_does_not_open_the_composite_close`
  Expect: `[1-9][0-9]* passed`
- **AC-4** — when the whole workspace is built, then it compiles green.
  Command: `cargo build --workspace`

## Files

- `apps/rt/src/commands/spec/complete_spec.rs`
- `apps/rt/src/commands/pipeline/close_orchestrate.rs`
- `plugin/commands/close.md`
- `plugin/commands/qa.md`
- `plugin/refs/feature/spec-hygiene.md`

## Root cause

`mark_complete` writes the completion event straight through the NDJSON writer
and consults no verdict. The gate that does work lives in `emit-pipeline`, is
coupled to that command's own options struct and to `process::exit`, and is
therefore unreachable from a library caller — so nothing enforces the
precondition on the closer's own path. `run_qa_fail_open` RUNS the verification
but only prints its outcome, which is what makes the omission invisible: the
operator sees a verdict line scroll past and the close happens regardless.

## Plan

Reuse the policy, add the admission, enforce it once:

- keep `emit_pipeline::qa_result_passed` as the single reader of "the recorded
  verdict is a pass" — no second predicate;
- add a pure admission that answers whether a terminal close may proceed:
  admitted when the spec is already finished (its close writes nothing) or when
  the recorded verdict passed; refused otherwise, and refused on an unreadable
  store;
- fold the duplicated "run the verification, then close" shape shared by the
  default branch and `--archive` into ONE step that consults the admission, so a
  future third branch cannot forget it;
- leave `finalize` exactly as it is — it is the tail the composite closes reuse
  after they already gated;
- correct the shipped ritual to name the protection that now exists.

## Limits

The refusal reads what was RECORDED; it does not verify anything itself. A spec
whose verification cannot run in the harness's own process still needs one
external run to record a pass before it may close — which is the existing
contract, now enforced instead of described.

## Definitions

- **close admission** — the precondition a TERMINAL close must satisfy before `pipeline.complete` may be written: a recorded `qa.result` whose `overall` is `pass` for this spec. Distinct from running QA — the admission reads what was RECORDED, it does not verify anything itself.
- **already-terminal close** — a close issued against a spec whose projection is already `completed` or `cancelled`. `mark_complete` short-circuits it and writes no event, so it is a documented no-op and the admission must not turn it into a refusal.

## Decisions

- Reuse `emit_pipeline::qa_result_passed` as the policy; do not write a second predicate.
  Reason: It is already `pub(crate)` and already carries four unit tests (no events dir, requires overall=pass, false on fail, false on skip). A second reader of the same event log is exactly how two notions of 'QA passed' drift apart.
- Enforce inside the QA-then-complete pairing, not inside `finalize`.
  Reason: `finalize` is documented as the QA-less tail reused by `close-pipeline` and `close-orchestrate`, which have ALREADY run QA and gated on it. Gating there would double-gate the correct path; gating the pairing closes the only door a caller can actually walk through.
- Both CLI branches — the default complete and `--archive` — go through ONE shared admission step.
  Reason: They duplicate the same 'run QA fail-open, then complete' shape today. Extracting the pairing removes the duplication and makes it impossible to add a third branch that forgets the gate.
- An already-terminal spec is admitted without consulting the verdict.
  Reason: Its close writes no event, so there is nothing to gate; refusing it would break the documented hygiene sweep (`complete-spec {name} --archive` on a spec confirmed done) and turn an idempotent no-op into a failure.
- Fail CLOSED on an unreadable event store, and refuse with exit 2.
  Reason: Mirrors the stance `enforce_qa_gate_or_exit` already documents for the same event: allowing a complete on a missing store would erase the gate entirely. Same exit code so a caller that already handles the emit-pipeline refusal handles this one identically.
- No environment switch relaxes it.
  Reason: Project law: a gate blocks unconditionally, with no knob fork. The `--allow-no-qa` escape on `emit-pipeline` exists for trusted callers like `qa-run` itself; nothing calls `complete-spec` in that role.

## Evidence

- The shipped close ritual PROMISES this protection and it does not exist: it tells the reader never to hand-call complete-spec to bypass a red gate because the emit-pipeline QA-gate rejects it anyway. complete-spec never goes through emit-pipeline.
  Evidence: `plugin/commands/close.md:23`
- mark_complete writes pipeline.complete straight through the NDJSON writer, consulting no verdict — the event the close gate exists to protect.
  Evidence: `apps/rt/src/commands/spec/complete_spec.rs:207`
- The gate that DOES work lives in emit-pipeline and refuses with exit 2 unless a recorded qa.result overall=pass exists, fail-closed on an unreachable store.
  Evidence: `apps/rt/src/commands/event/emit_pipeline.rs:292`
- qa_result_passed is the single policy for 'the recorded verdict is a pass'; it is pub(crate) with four unit tests and today has exactly ONE caller.
  Evidence: `apps/rt/src/commands/event/emit_pipeline.rs:498`
- run_qa_fail_open runs QA self-invoked and only PRINTS the outcome; it gates nothing, so the CLI face completes whatever the verdict was.
  Evidence: `apps/rt/src/commands/spec/complete_spec.rs:622`
- run_complete is the QA-then-complete pairing and has exactly one caller: the CLI face at line 613.
  Evidence: `apps/rt/src/commands/spec/complete_spec.rs:554`
- finalize is the deliberate QA-less tail, reused by close-pipeline and close-orchestrate precisely because those already ran QA and gated on overall == pass.
  Evidence: `apps/rt/src/commands/spec/complete_spec.rs:564`
- The --archive branch duplicates the pairing: it calls run_qa_fail_open and then archive, which calls mark_complete — a second ungated route to the same event.
  Evidence: `apps/rt/src/commands/spec/complete_spec.rs:602`
- mark_complete already short-circuits an already-terminal spec without writing any event, which is why the admission must admit that case instead of refusing it.
  Evidence: `apps/rt/src/commands/spec/complete_spec.rs:183`
- The documented hygiene sweep tells the operator to close a spec confirmed done with complete-spec --archive, so that path must keep working for an already-terminal spec.
  Evidence: `plugin/refs/feature/spec-hygiene.md:10`
- Reproduced live with the installed binary: a spec whose only attempted criterion was the trailing safety net closed with overall=skip and pipeline.complete was emitted anyway, with no qa.result recorded at all.
  Evidence: `apps/rt/src/commands/spec/complete_spec.rs:613`