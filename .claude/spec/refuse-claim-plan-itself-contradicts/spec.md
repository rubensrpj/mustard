---
id: spec.refuse-claim-plan-itself-contradicts
---

# Refuse a claim the plan itself contradicts: a wave cannot cover a criterion while declaring nowhere to do the work

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

Every acceptance criterion must be claimed by some wave of the plan, and that
requirement is already enforced — an unclaimed criterion refuses the plan
outright. What nothing checks is whether the wave making the claim declared
anywhere to do the work.

A field run made the difference visible. A wave was dispatched with a green
pre-gate and came back blocked: two of the criteria it had claimed required
changes in a shared package and a backend that no wave of the eleven had in
scope. The plan looked complete because every wave, read alone, was coherent.
The gap was in the whole.

That failure has been described here more than once as "the plan does not prove
the claiming wave is adequate to satisfy the criterion", and left open on the
grounds that adequacy cannot be decided. That reasoning is sound and the
conclusion drawn from it was wrong. Adequacy — will these files be ENOUGH —
really is undecidable before the work is attempted; not even the implementer
knows until they try. But what the field run actually showed was not an
inadequate declaration. It was a self-contradicting one: a claim the plan's own
contents refute.

That narrower question is decidable from the plan alone, and two shapes of it
are worth acting on. A wave that claims a criterion while declaring no files has
said it will do work and, in the same breath, that it has nowhere to do it — no
reading of the plan makes that hold. And a criterion whose own command names a
repository path that none of its claimants declares is pointing at something
nobody in that group will touch.

The distinction is the whole point, and it is why this is worth building where
the stronger check was not: refusing a contradiction is a fact about the
document; asserting adequacy would be a guess wearing the clothes of a gate,
which is the failure this whole line of work exists to remove.

## Users/Stakeholders

The operator approving a plan, who today can be handed one whose parts agree
individually and contradict as a set. The waves themselves, which discover the
gap by hitting it — spending a full dispatch to learn what the document already
said. And the coverage requirement, whose strictness about WHO claims a
criterion means less while nothing looks at what the claimant brought.

## Success Metric

A plan whose own contents refute one of its claims does not reach approval, and
a plan the check cannot judge is never refused on that basis.

## Non-Goals

Deciding adequacy — whether the declared files suffice to satisfy the criterion.
That is undecidable before the attempt, and any answer would be a heuristic
presented as a gate. Judging a criterion whose command names no path: most
criteria here run a named test, so the path lives inside the test, and those are
simply not covered rather than guessed at. Changing what the existing coverage
requirement means, or adding any switch that softens either signal.

## Acceptance Criteria

- **AC-1** — when a wave claims a criterion while declaring no files at all, then the plan is refused, because the claim contradicts the same plan that makes it.
  Command: `cargo test -p mustard-rt --lib wave_scaffold::tests::a_wave_claiming_a_criterion_with_nowhere_to_work_is_refused`
  Expect: `[1-9][0-9]* passed`
- **AC-2** — when a criterion's command names a repository path that no wave claiming it declares, then the plan is flagged without being refused, because a command may name a path it only reads.
  Command: `cargo test -p mustard-rt --lib wave_scaffold::tests::a_criterion_pointing_outside_its_claimants_is_flagged_not_refused`
  Expect: `[1-9][0-9]* passed`
- **AC-3** — when a plan is internally consistent, then neither signal fires — including the cases this check deliberately cannot judge, so it never refuses what it cannot decide.
  Command: `cargo test -p mustard-rt --lib wave_scaffold::tests::a_consistent_plan_and_an_unjudgeable_one_both_stay_silent`
  Expect: `[1-9][0-9]* passed`
- **AC-4** — when the whole workspace is built, then it compiles green.
  Command: `cargo build --workspace`

## Files

- `apps/rt/src/commands/wave/wave_scaffold.rs`
- `apps/rt/src/commands/review/analyze_validation.rs`
- `apps/rt/src/commands/pipeline/plan_materialize.rs`

## Root cause

The traceability pass answers "which wave claims which criterion" and stops
there. It already holds everything the further question needs — each wave's
declared files sit beside its claims, and the criterion parser hands over the
command alongside the id — but nothing crosses the two. So a claim is checked
for existence and never for support.

## Plan

- extend the existing traceability pass with the two decidable gaps, reusing the
  claim resolution already computed there rather than re-deriving it;
- reuse the existing path recogniser for what reads as a repository path — it is
  already tuned to keep prose out — instead of writing a second one;
- route the unsupportable claim through the SAME channel the coverage gate
  already blocks on, so one kind of fact keeps one severity and one message
  shape;
- keep the path mismatch advisory, surfaced beside the existing WARN signal.

## Limits

This decides consistency, never adequacy, and the distinction is load-bearing
rather than a caveat: a plan that passes has not been shown sufficient, only
shown not to contradict itself. Two known blind spots, stated so the check is
not trusted past what it earns: a criterion whose command names no path is not
judged at all, and a wave that declares files unrelated to its claim still
passes — the check sees that something was declared, not that it was the right
something. Deciding either would require knowing the work before it is done.

## Definitions

- **adequacy** — whether the files a wave declared are ENOUGH to satisfy the criterion it claims. Undecidable before implementation: it would require knowing which files the work will touch, which not even the implementer knows until they try. Nothing here attempts it.
- **an unsupportable claim** — a claim whose own declaration contradicts it — a wave saying it covers a criterion while declaring no place to do the work, or a criterion whose command names a repository path that no wave claiming it declares. Decidable from the plan alone, and the shape the field report actually observed.

## Decisions

- Check consistency, never adequacy — and say so where a reader would otherwise assume the stronger claim.
  Reason: Adequacy cannot be decided from a declared file list, and a heuristic dressed as a gate is precisely the failure this chain of work removed: rigour applied to a criterion that does not discriminate produces unearned confidence, not a guarantee. What CAN be decided is whether the declaration contradicts itself, and the failure actually seen in the field was a contradiction, not an inadequacy.
- A wave claiming a criterion while declaring NO files BLOCKS; a criterion whose command names a path no claiming wave declares WARNS.
  Reason: The first is unambiguous — a claim with no declared place to do the work cannot hold under any reading, so it joins the existing coverage gate rather than inventing a second severity for the same kind of fact. The second is a strong signal but not a proof: a command may name a path it only reads (a fixture, a doc it greps), so it earns attention rather than a refusal.
- Reuse the existing traceability pass and the existing path recogniser; add no second reader of either fact.
  Reason: `traceability_gaps` already resolves which wave claims which criterion, through the same qa-run parser QA executes. And `looks_like_file_path` already decides what reads as a repository path, tuned to keep prose out. A second copy of either is how two notions of the same fact drift apart — the defect this codebase has paid for more than once.
- A criterion whose command names no path at all is not checked, and that is stated rather than hidden.
  Reason: Most criteria here run a named test, so the path lives inside the test rather than in the command. The check therefore covers a real subset, not everything, and a reader who assumes otherwise would trust it further than it earns.

## Evidence

- The traceability pass already computes, per wave, the set of criteria it claims — through the same qa-run parser QA executes — so the claim side of the question is already resolved and needs no new reader.
  Evidence: `apps/rt/src/commands/wave/wave_scaffold.rs:458`
- It already separates a WARN gap from the escalatable one, and the escalatable one is what plan-materialize blocks on — so a new blocking gap joins an existing channel instead of inventing a second.
  Evidence: `apps/rt/src/commands/wave/wave_scaffold.rs:425`
- The coverage gate refuses the PLAN transition with exit 2 and no env knob, which is the precedent a second unconditional gap follows.
  Evidence: `apps/rt/src/commands/pipeline/plan_materialize.rs:84`
- Each wave carries its own declared files in the plan, so the file side of the question is already available where the claim is resolved.
  Evidence: `apps/rt/src/commands/wave/wave_scaffold.rs:475`
- looks_like_file_path already decides what reads as a repository path and is deliberately tuned to keep prose out by requiring a separator or a known extension.
  Evidence: `apps/rt/src/commands/review/analyze_validation.rs:274`
- The criterion parser exposes the command alongside the id, so the command text is available in the same pass that resolves the claim.
  Evidence: `apps/rt/src/commands/review/qa_run/mod.rs:39`
- Field evidence: a wave was dispatched with a green pre-gate and returned BLOCKED because two of its criteria required changes in a shared package and a backend that no wave of the eleven had in scope — the plan looked complete because each wave was coherent alone.
  Evidence: `docs/2026-07-25-revisao-portoes-pipeline-ondas.md:1`