---
id: spec.make-harness-stop-asserting-what
---

# Make the harness stop asserting what it has not verified: prove every acceptance criterion both red-before and green-after, give a criterion found inexecutable a sanctioned repair path, let a plan declare duties to verify the world outside the repository, surface the dependency-precheck skip on unsupported stacks, discover in-flight specs across work branches, distinguish a never-dispatched plan from wave 1, and record a deliberately dropped checklist item as a decision instead of a pending one

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

Make the harness stop asserting what it has not verified: prove every acceptance criterion both red-before and green-after, give a criterion found inexecutable a sanctioned repair path, let a plan declare duties to verify the world outside the repository, surface the dependency-precheck skip on unsupported stacks, discover in-flight specs across work branches, distinguish a never-dispatched plan from wave 1, and record a deliberately dropped checklist item as a decision instead of a pending one.

Why now. Three field reviews of the same harness landed in one week, and they
converge on a single habit rather than eight unrelated bugs: when information is
missing, this harness prefers to complete the sentence rather than admit the gap.
The proof of a criterion completes with half the evidence. The dependency
precheck writes down that it declined to judge, and nobody reads it back. The
active-spec listing answers "nothing in progress" when it means "I only looked at
one branch". Wave progress reports "wave 1 of 5" for a plan nobody ever
dispatched. A checklist item dropped on purpose looks exactly like one forgotten.

Each of those is cheap on its own. Together they are the reason an operator — or
an agent — states something with confidence and is wrong, which costs more than
the error itself: it discredits the answers that were right.

## Users/Stakeholders

The operator running a pipeline across sessions, who reads these answers and acts
on them; the implementer agent dispatched into a wave, whose prompt is built from
the same state; and the reviewer reading the record months later, for whom a
dropped decision and a forgotten task must not look alike.

## Success Metric

Every mechanism touched here answers one of three ways — the fact, "I did not
look", or "I cannot judge this" — and never a fourth way that reads like the
fact. Concretely: no gate accepts a criterion it never saw pass; no listing
reports absence it did not verify; no progress number is derived from directories
alone.

## Non-Goals

Mold-gate precision stays out: correcting it needs a measured hit rate, and two
misses in one session is an observation, not a measurement. Flipping the boundary
gate from advisory to blocking stays out: its noise comes from scope lists nobody
updates, so blocking today would turn twenty advisories into twenty blocks.
Detecting correctness — code that compiles, passes, and is still wrong — stays
out by nature; no gate reaches it, and pretending otherwise is the very habit
this spec exists to remove.

## Acceptance Criteria

Each criterion below names the test that proves it and demands a non-zero pass
count, because a filter that matches nothing exits 0 and prints "0 passed" — a
green that proves nothing. `[1-9][0-9]*` is what refuses that reading.

- **AC-1** — when the criterion proof is taken again after a wave's work has
  landed, then a criterion that still comes back red is reported as unproven
  instead of clearing on its earlier failure alone
  Command: `cargo test -p mustard-rt ac_proof_requires_green_after`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-2** — when the criterion being replaced is recorded as inexecutable, then
  ac-amend accepts a substitute that passes, instead of refusing everything that
  is not red
  Command: `cargo test -p mustard-rt ac_amend_accepts_inexecutable_predecessor`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-3** — when active-spec discovery runs on a branch that does not carry the
  spec directory, then a spec living on an unmerged work branch is listed as
  in-flight with the branch that holds it
  Command: `cargo test -p mustard-rt active_specs_lists_in_flight_from_other_branches`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-4** — when a plan declares reality obligations for a wave, then those
  duties reach the dispatched agent's prompt as their own section
  Command: `cargo test -p mustard-rt plan_reality_obligations_reach_wave_prompt`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-5** — when a wave closes without reporting the reality obligations it was
  given, then wave-done reports the unmet duty by name
  Command: `cargo test -p mustard-rt wave_done_flags_unreported_reality_obligation`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-6** — when a spec has wave directories but no dispatch event, then resume
  bootstrap reports the plan as never dispatched instead of as wave 1
  Command: `cargo test -p mustard-rt wave_progress_distinguishes_never_dispatched`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-7** — when the dependency precheck declines to judge an unsupported stack,
  then the caller surfaces the skip instead of reading the empty result as a
  clean pass
  Command: `cargo test -p mustard-rt dependency_precheck_skip_is_surfaced`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-8** — when a checklist item is dropped on purpose with a stated reason,
  then it is recorded as a decision and stays distinct from an unchecked item
  Command: `cargo test -p mustard-rt checklist_records_dropped_with_reason`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-9** — the project build passes green
  Command: `cargo build --workspace`

<!-- PLAN -->

## Files

Wave 1 — the criterion proof gains its second half:

- `apps/rt/src/commands/review/ac_negative_check.rs`
- `apps/rt/src/commands/spec/ac_amend.rs`
- `apps/rt/src/commands/spec/approve_spec.rs`

Wave 2 — in-flight specs become visible across branches:

- `apps/rt/src/commands/spec/active_specs.rs`

Wave 3 — a plan can oblige a wave to verify the world:

- `plugin/refs/feature/full-plan.md`
- `apps/rt/src/commands/pipeline/plan_materialize.rs`
- `apps/rt/src/commands/agent/agent_prompt_render.rs`
- `apps/rt/src/commands/pipeline/wave_done.rs`

Wave 4 — bootstrap and precheck stop implying what they did not check:

- `apps/rt/src/commands/pipeline/resume_bootstrap/wave_progress.rs`
- `apps/rt/src/commands/pipeline/resume_bootstrap/mod.rs`
- `apps/rt/src/commands/review/dependency_precheck.rs`

Wave 5 — a dropped item is recorded as a decision:

- `apps/rt/src/commands/checklist/mark_checklist_item.rs`
- `packages/core/src/domain/model/view/wave.rs`

## Boundaries

IN: the eight fixes above, each landing in the mechanism named by its own
evidence line; the tests that name each criterion; the module doc contradiction
in `wave_progress.rs` (0-based prose over 1-based code), fixed in the wave that
already opens that file.

OUT: mold-gate precision, the boundary gate's advisory default, and any attempt
to detect correctness rather than conformance — all three argued in Non-Goals.
Also out: changing where a spec directory lives. Wave 2 teaches discovery to look
across branches; it does not move the spec back to the base branch, because the
spec belongs with the work it describes.

## Definitions

- **conformance** — the code does what the spec said it would — the class of defect a gate can check
- **correctness** — the code actually works — the class of defect only reading or running finds
- **red proof** — ac-negative-check's rule: a criterion clears only by FAILING against the tree before its work exists
- **in-flight spec** — a spec whose work branch has not merged yet — its directory exists only on that branch
- **reality obligation** — a plan-declared duty to verify something outside the repository (official docs, a live endpoint, a stored row) before writing code

## Decisions

- all eight fixes ship as ONE spec of five waves
  Reason: the operator's explicit call after a three-spec split was proposed; ageing risk is mitigated by a per-wave premise re-check from round 2 on
- the criterion-proof wave runs first
  Reason: every other wave writes new acceptance criteria, which would otherwise pass through the same broken gate
- cross-branch spec discovery runs in round 1 next to the gate wave
  Reason: it is the only fix the operator hits by hand on every branch switch, and it shares no file with the gate wave
- mold-gate precision stays OUT of scope
  Reason: fixing precision needs a measured hit rate; two misses in one session is an observation, not a measurement
- flipping the boundary gate to blocking stays OUT of scope
  Reason: the cause is the never-updated scope list; blocking today would turn twenty advisories into twenty blocks
- base is dev
  Reason: mustard.json#git.flow declares '*': 'dev', and every recent spec cut from it

## Evidence

- The negative proof has only one half: a criterion clears ONLY by failing before its work exists. Nothing ever requires it to pass afterwards, so a command that is broken and a command whose behaviour is absent are indistinguishable to the gate.
  Evidence: `apps/rt/src/commands/review/ac_negative_check.rs:6`
- ac-amend refuses any replacement that is not red. A criterion discovered to be inexecutable only AFTER its work landed cannot be repaired through the sanctioned door, because the corrected command passes.
  Evidence: `apps/rt/src/commands/spec/ac_amend.rs:14`
- On a non-JS/TS target the dependency precheck declines to judge and returns ok:true with an empty checks_performed plus skipped:"stack-unsupported". The honest marker is already in the payload; no caller surfaces it, so half the dispatches in a polyglot repo look verified.
  Evidence: `apps/rt/src/commands/review/dependency_precheck.rs:1100`
- Active-spec discovery globs .claude/spec/*/spec.md in the current working tree only. The file contains no git, branch or worktree query at all, so a spec living on an unmerged work branch is reported as absent.
  Evidence: `apps/rt/src/commands/spec/active_specs.rs:1`
- FS wave progress counts wave-* directories as the total and reads each wave header (stage close + outcome completed) for done, then reports current = done + 1. A plan that was scaffolded and never dispatched therefore reads as 'wave 1 of 5', indistinguishable from a plan ready to start.
  Evidence: `apps/rt/src/commands/pipeline/resume_bootstrap/wave_progress.rs:33`
- The module doc states wave directories are 0-based; the code twenty lines below states and implements 1-based. One of the two is read by whoever edits next.
  Evidence: `apps/rt/src/commands/pipeline/resume_bootstrap/wave_progress.rs:5`
- The checklist marker knows two positions only and dies when no `- [ ]` matches. An item dropped on purpose has nowhere to be recorded, so it stays indistinguishable from one that was forgotten.
  Evidence: `apps/rt/src/commands/checklist/mark_checklist_item.rs:404`
- WaveStatus declares exactly four states — Queued, InProgress, Completed, Failed. Deliberately abandoned work has no variant, which is how a dropped decision reads as a pending one months later.
  Evidence: `packages/core/src/domain/model/view/wave.rs:16`
- The only research the flow knows is the deterministic digest over the codebase. Nothing in the plan machinery obliges a wave to verify anything outside the repository, which is why the one duty that caught a provider semantics inversion in the field was hand-written prose.
  Evidence: `plugin/refs/feature/full-plan.md:79`