---
id: spec.retire-last-holdout-advisory-skip
---

# Retire the last holdout of the advisory skip: the close gate must refuse a spec that declares no criteria, like every other door already does

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

Retire the last holdout of the advisory skip: the close gate must refuse a spec that declares no criteria, like every other door already does.

fill in why now.

## Users/Stakeholders

fill in who benefits.

## Success Metric

fill in the success metric.

## Non-Goals

fill in what stays out.

## Acceptance Criteria

- **AC-1** — when the recorded verdict is a skip and the run recorded no criteria at all, then the strict gate refuses instead of falling through, so the retired advisory rule cannot come back unnoticed.
  Command: `cargo test -p mustard-rt --lib close_gate::tests::an_empty_criteria_skip_no_longer_falls_through`
  Expect: `[1-9][0-9]* passed`
- **AC-2** — when either skip shape is refused, then each is told apart in the reason and pointed at its own remedy, so the refusal is not a blunt merge of two different situations.
  Command: `cargo test -p mustard-rt --lib close_gates::tests::the_two_skip_shapes_are_refused_with_their_own_remedy`
  Expect: `[1-9][0-9]* passed`
- **AC-3** — when the operator has deliberately set the warn mode, then both skip shapes still fall through, so the existing override keeps its meaning.
  Command: `cargo test -p mustard-rt --lib close_gate::tests::warn_mode_still_lets_both_skip_shapes_through`
  Expect: `[1-9][0-9]* passed`
- **AC-4** — when the whole workspace is built, then it compiles green.
  Command: `cargo build --workspace`

## Files

- `apps/rt/src/commands/pipeline/close_gates.rs`
- `apps/rt/src/hooks/write/close_gate.rs`

## Root cause

One condition, `criteria_count > 0`, guards the whole strict branch for a skip
verdict. With no criteria recorded the branch is skipped and the gate falls
through to allow. Its own comment states the retired rule as if it were current
— "the historical advisory contract holds" — and a passing test pins that shape
as the legitimate one, which is why nothing noticed when every other door
stopped honouring it.

## Plan

- drop the count guard so BOTH skip shapes reach the strict refusal, keeping a
  distinct reason and remedy for each: a spec with nothing declared needs a
  criterion authored; criteria that exist but were never attempted need their
  commands fixed, or a verdict recorded by an external run that can attempt them;
- invert the test that pins the carve-out rather than deleting it — it is the
  only coverage the empty-criteria shape has, and inverted it makes the retired
  rule impossible to reintroduce silently;
- leave the warn mode exactly as it is: it is the operator's deliberate
  override, not a knob invented here.

## Limits

This adapter can only ALLOW — it never writes the completion event, so no door
was open while it disagreed. What is being closed is a rule disagreement, not an
active hole. The shipped prose already describes the rule this brings the code
in line with, so nothing here changes what the operator was told.

## Definitions

- **the two skip shapes** — a verification run reports `skip` for two different reasons, told apart by whether it recorded any criteria at all. EMPTY criteria: the spec declares nothing to verify. NON-EMPTY criteria: criteria exist but none could be attempted (timeout, spawn failure, or a run inside the binary its criteria target). The remedies differ — author a criterion versus fix or externally re-run the ones that exist — but neither is a verification.

## Decisions

- Remove the empty-criteria carve-out so both skip shapes deny in strict mode, while keeping a DISTINCT reason for each.
  Reason: Every other door already refuses both shapes; this adapter is the last holdout, and one rule enforced in four places minus one is not a rule. Keeping the reasons distinct is what stops the fix from being a blunt merge: the two shapes ask the operator for opposite actions, and a gate that refuses without naming the remedy is the theatre that teaches callers to route around it.
- The existing test that asserts the carve-out is INVERTED, not deleted.
  Reason: It is the only place that pins the empty-criteria shape at all. Deleting it would leave that shape uncovered; inverting it keeps the coverage and makes the retired rule impossible to reintroduce silently.
- No shipped prose changes.
  Reason: The rituals were already corrected in the previous unit to say every skip blocks. This adapter was the reason that sentence was not yet universally true; the fix makes the shipped documentation true rather than adding new claims.
- The warn mode keeps its meaning untouched.
  Reason: The change is about what STRICT refuses. `MUSTARD_QA_GATE_MODE=warn` already exists as the operator's deliberate override and is not a knob invented here.

## Evidence

- The carve-out is a single condition: with `criteria` empty the strict branch is skipped entirely and the gate falls through to allow.
  Evidence: `apps/rt/src/commands/pipeline/close_gates.rs:1042`
- Its own comment states the retired rule as current: 'criteria empty -> the spec carries nothing testable; the historical advisory contract holds - fall through'.
  Evidence: `apps/rt/src/commands/pipeline/close_gates.rs:1036`
- A test asserts the carve-out as the legitimate shape, calling it 'the historical advisory contract' — so the retired rule is currently pinned by a passing test.
  Evidence: `apps/rt/src/hooks/write/close_gate.rs:410`
- The sibling shape (criteria exist but all skipped) already denies in strict and names the count in its reason, so the deny path and its message shape already exist and are reused rather than invented.
  Evidence: `apps/rt/src/commands/pipeline/close_gates.rs:1046`
- The shipped QA ritual already tells the reader that every close door reads the recorded verdict and only a pass opens it, so a skip always blocks — a sentence this adapter currently contradicts.
  Evidence: `plugin/commands/qa.md:32`
- Reported by the review that approved the previous unit, which verified the adapter can only ALLOW and never writes the completion event — so no door is open today; it is the last rule-disagreement, not an active hole.
  Evidence: `apps/rt/src/hooks/write/close_gate.rs:410`