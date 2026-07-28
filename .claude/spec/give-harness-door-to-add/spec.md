---
id: spec.give-harness-door-to-add
---

# Give the harness a door to ADD an acceptance criterion, taking the same negative proof a planned one takes, and stop the statement rewrite from orphaning continuation lines when a criterion carries no Command line

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

Give the harness a door to ADD an acceptance criterion, taking the same negative proof a planned one takes, and stop the statement rewrite from orphaning continuation lines when a criterion carries no Command line.

Why now. The flow already carries the rule: a request that is implemented but
named by no criterion makes the gate report green without ever verifying it. The
rule is written down and nothing implements it — the only criterion-editing
operation replaces an id that already exists, and refuses one it does not know.

So when a review demands a criterion for a finding, there is no door. In the run
that just shipped, four criteria had to be typed into the spec by an agent's own
hand, against the role contract that forbids touching the spec. The result was
correct only because that agent then submitted each one to the proof — a
discipline nobody enforced and nothing would have caught.

## Users/Stakeholders

The reviewer who finds a defect mid-pipeline and needs it named by a criterion
rather than by a paragraph; the operator who should not have to choose between
editing a frozen artefact and leaving a fix unverified; and the reader of a
closed spec, for whom "no criterion covers this" must be impossible rather than
merely discouraged.

## Success Metric

A criterion can be added by an operation, and it enters the ledger through the
same proof a planned criterion takes — red before its work exists. Concretely:
the four criteria added by hand in the previous run could have been added by this
door, with the same recorded verdicts, and nobody would have edited the spec.

## Non-Goals

Adding a criterion that skips the proof stays out: a door that admitted an
unproven criterion would import the vacuous-criterion defect the proof exists to
stop. Folding the operation into the amend command stays out too — amend's whole
contract is that a replacement supersedes a predecessor, and an added id has no
predecessor. Rewriting how criteria are numbered stays out: this door appends an
id, it does not renumber what is already there.

## Acceptance Criteria

Each criterion names the test that proves it and demands a non-zero pass count:
a filter matching nothing exits 0 and prints "0 passed", and `[1-9][0-9]*` is
what refuses to read that as success.

- **AC-1** — when a criterion id the spec does not carry is added through the
  door, then it lands in the spec and in the ledger only after taking the same
  negative proof a planned criterion takes
  Command: `cargo test -p mustard-rt ac_add_lands_only_after_taking_the_proof`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-2** — when the added criterion's command already passes against the tree
  as it is, then the addition is refused and nothing is written anywhere
  Command: `cargo test -p mustard-rt ac_add_refuses_a_criterion_that_cannot_fail`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-3** — when the work a criterion describes is taken away again, then a criterion still green there is reported as verifying nothing, and one whose own evidence the strip took away is DECLINED by name instead of being booked as a proven red
  Command: `cargo test -p mustard-rt removal_refuses_a_survivor_and_declines_what_it_cannot_judge`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-4** — the project build passes green
  Command: `cargo build --workspace`

## Checklist

- [ ] T1 — Add the door: a criterion id the spec does not carry can be introduced
      with its statement, command and expectation, and it is written to every
      artefact that carries criteria — never only the root spec.
- [ ] T2 — Route the addition through the same negative proof, refusing an id that
      already exists and refusing a command that comes back green.
- [ ] T3 — Reach the orphan the multi-line rewrite cannot: a criterion carrying no
      Command line must have its whole statement block replaced, not just its
      header.
- [ ] T4 — Take the third transition: run the criterion against the tree with the
      work it describes taken away, and require it to come back red. The wave's
      own cached diff is what says which change to take away, so no guessing is
      needed about what "the work" was.

## Definitions

- **amend** — replace the command, expectation or statement of a criterion that already exists, proving the replacement can still fail
- **add** — introduce a criterion id the spec does not carry yet — the operation that does not exist today
- **statement block** — the criterion's prose: the header line plus every continuation line up to its Command: line

## Decisions

- adding a criterion is a door of its own, not a flag on amend
  Reason: amend's whole contract is that a replacement must prove it can fail against a tree where the old one already lived; an added id has no predecessor to supersede, so folding it into amend would blur the one rule that makes amend trustworthy
- an added criterion takes the same negative proof as a planned one
  Reason: the door exists so a mid-pipeline finding can be named by a criterion — a door that admitted a criterion nobody proved would import exactly the vacuous-criterion defect the proof was built to stop
- the orphan on a criterion without a Command: line is fixed in the same pass
  Reason: it is the same rewrite surgery, one file away, and leaving it is how a defect fixed in one shape survives in another

## Evidence

- ac-amend can only REPLACE an existing criterion id and refuses an unknown one, so when a review demands a criterion for a finding, the harness offers no operation at all. In this session four criteria had to be written into spec.md by an agent's own hand, against the role contract that forbids touching the spec.
  Evidence: `apps/rt/src/commands/spec/ac_amend.rs:1`
- The flow states the rule the missing door is supposed to serve — a request implemented but unnamed by any criterion makes the gate report green without ever verifying it — so the rule is written while nothing implements it.
  Evidence: `plugin/refs/spec/resume-loop.md:92`
- The statement rewrite gives up when a criterion carries no Command: line, so the header is replaced and the superseded continuation lines survive underneath it — the same orphan the multi-line fix removed, in a shape it does not reach.
  Evidence: `apps/rt/src/commands/spec/ac_amend.rs:378`
- The rewrite must reach every artefact carrying the id, not just the root spec: the wave plan and each wave spec repeat the criterion lines, and the scaffold is frozen after approval.
  Evidence: `apps/rt/src/commands/spec/ac_amend.rs:17`