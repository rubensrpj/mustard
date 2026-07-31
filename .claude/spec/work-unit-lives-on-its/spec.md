---
id: spec.work-unit-lives-on-its
---

# The work unit lives on its branch. A base gate refuses to start an analysis outside an integration base or on a base behind its remote, and runs the census refresh there. Once the analysis is approved the branch is cut and the spec, its waves and the whole ceremony are materialized inside it. /mustard:spec called from within that branch resumes with no ceremony. Anything that surfaces during the work and does not belong to the spec goes into a per-branch notebook that becomes the next cycle's prompt. A new /mustard:pr door carries list, review and merge; /mustard:git gains delete. The exposed surface drops from fifteen doors to four: git, pr, spec, upsert.

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

The work unit lives on its branch. A base gate refuses to start an analysis outside an integration base or on a base behind its remote, and runs the census refresh there. Once the analysis is approved the branch is cut and the spec, its waves and the whole ceremony are materialized inside it. /mustard:spec called from within that branch resumes with no ceremony. Anything that surfaces during the work and does not belong to the spec goes into a per-branch notebook that becomes the next cycle's prompt. A new /mustard:pr door carries list, review and merge; /mustard:git gains delete. The exposed surface drops from fifteen doors to four: git, pr, spec, upsert..

Why now: the flow works but its pieces sit in the wrong places. A spec is authored on the
base while the code it governs lives on a branch, so the base accumulates the directory of
every spec ever drafted. Nothing checks that an analysis starts from an up-to-date base, so
a spec can be written against a tree that no longer exists. The PR cycle — list, review,
merge, clean up — is spread across four commands in two files, and the last step is manual.
And fifteen doors are exposed for a flow that uses two of them.

## Users/Stakeholders

The single operator driving Mustard day to day. Every change here is about the gestures that
operator types and the state they have to hold in their head between those gestures.

## Success Metric

The whole cycle — from a prompt on the base to a merged PR with the branch pruned — is
typed with four doors and no manual step outside them. A unit's every artefact (spec, waves,
ceremony, code, notebook) lives on one branch, so deleting the branch deletes the unit whole.

## Non-Goals

- Replacing the wave dispatch engine with the workflow tool. The research settled the facts
  (20 concurrent subagents with the excess failing rather than queueing, no native dependency
  graph, background agents killed on session pause) but the swap is its own work unit.
- Changing how the analysis grills the request. The grill stays exactly as it is; only the
  gate in front of it and the home of its output move.
- Touching the internal flows (feature, bugfix, task, tactical-fix). They are already
  non-invocable and the router keeps choosing them the same way.

## Acceptance Criteria

- **AC-1** — when a pipeline is opened from a checkout that is not a `git.flow` base, then the base gate refuses and names the base to switch to.
  Command: `cargo test -p mustard-rt base_gate 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-2** — when a `spec.md` write is attempted on a protected base, then the work-branch gate refuses it instead of carving it out.
  Command: `cargo test -p mustard-rt spec_authoring_on_protected_base 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-3** — when a spec is resumed from inside its own `{base}_{slug}` branch, then it dispatches with no confirmation prompt and no table.
  Command: `cargo test -p mustard-rt resume_inside_own_branch 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-4** — when `pr list` runs from a work branch instead of an integration base, then it refuses and names the base.
  Command: `cargo test -p mustard-rt pr_list 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-5** — when a merge is requested with no recorded review verdict, then the command warns and asks rather than refusing or merging silently.
  Command: `cargo test -p mustard-rt pr_merge_without_verdict 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-6** — when `git delete` is invoked from a work branch instead of a base, then it refuses without touching anything.
  Command: `cargo test -p mustard-rt git_delete 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-7** — when an out-of-scope item is recorded during a work unit, then it lands in that unit's notebook and is readable back by unit.
  Command: `cargo test -p mustard-rt notebook 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-8** — when the exposed command surface is enumerated, then exactly four user-invocable doors remain: git, pr, spec and upsert.
  Command: `cargo test -p mustard-rt exposed_doors 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-9** — the project build and tests pass green
  Command: `cargo build --workspace`

<!-- PLAN -->

## Files

- `apps/rt/src/hooks/write/work_branch_gate.rs` — the base gate is added here and the
  `.claude/spec/` carve-out is removed from the same file.
- `apps/rt/src/commands/event/emit_pipeline.rs` — the pipeline-opening path that must
  cross the base gate.
- `apps/rt/src/commands/spec/spec_draft.rs` — reordered so the draft runs after the branch
  is cut, writing inside it.
- `apps/rt/src/commands/pipeline/resume_bootstrap/mode_decision.rs` — the no-ceremony resume
  when the current branch already belongs to the spec.
- `apps/rt/src/commands/git_settle.rs` — reused by both `pr merge` (pruning after merge) and
  the new `git delete`.
- `apps/rt/src/commands/event/work_branch.rs` — the per-branch notebook records live beside
  the work-unit state.
- `apps/rt/src/commands/review/cli.rs`, `apps/rt/src/commands/review/mod.rs` — the pr list /
  review / merge commands.
- `packages/core/src/domain/config.rs` — reading the `git.flow` bases the gate accepts.
- `plugin/commands/pr.md` (create) — the new door.
- `plugin/commands/git.md`, `plugin/refs/git/git-flow.md` — the `delete` action and the
  notebook rule.
- `plugin/commands/spec.md`, `plugin/refs/spec/resume-loop.md` — the no-ceremony resume.
- `plugin/commands/{mustard,status,stats,knowledge,maint,skills,qa,close,review,scan,unhook,rehook}.md`
  — the twelve doors that are removed, folded or turned into internal steps.
- `plugin/commands/upsert.md` — absorbs the doctor, off and on flags.
- `.claude/mustard/orchestrator.md` and its template under `apps/cli/templates/` — the router
  learns the base gate and the four-door surface.
- `apps/rt/tests/run_command_surface.rs`, `apps/rt/tests/template_parity.rs` — the locked
  command surface both tests guard.

## Boundaries

IN: the base gate before ANALYZE; the census refresh moved into that gate; cutting the branch
at approval and materializing the spec, its waves and ceremony inside it; the no-ceremony
resume from within the branch; the per-branch notebook and the loop back to the base gate; a
`pr` door carrying list, review and merge; a `git delete` action; reducing the exposed surface
to four doors.

OUT: replacing the wave dispatch engine with the workflow tool (its own work unit — the
research is recorded in `## Decisions`); how the analysis grills a request; the internal flows
(feature, bugfix, task, tactical-fix), which stay non-invocable and unchanged; the review
checklist itself, which `pr <id>` reuses as it is.

## Definitions

- **work unit** — the branch plus everything the work produced — spec, waves, ceremony, code and the notebook; deleting the branch deletes the unit whole
- **base gate** — the check that runs before ANALYZE and refuses to start when the checkout is not an integration base of git.flow, or is behind its remote
- **notebook** — a per-branch record of what surfaced during the work and does NOT belong to the current spec; after the PR opens it becomes the next cycle's prompt
- **door** — a user-invocable /mustard:* command — the surface the user types, as opposed to an internal flow the router dispatches
- **barrier** — a dispatch shape where the next level waits for every agent of the current level, so the level advances at the speed of its slowest agent

## Decisions

- the spec is materialized ON the work branch after the analysis is approved, not on the base
  Reason: the user wants the branch to be the container of the whole unit — one place to look, and deleting the branch deletes the unit; today .claude/spec/ is carved out of branch protection precisely so the spec can be authored on the base before the branch exists
- the approval gates move to run inside the branch, before wave 1
  Reason: they read artifacts from disk (ac-proof.json, .clarified) that will not exist before the draft; their premise — the code does not exist yet — still holds inside the branch before the first wave, so the proof keeps its meaning
- a merge with no recorded review verdict WARNS and asks for confirmation, it never refuses
  Reason: the user chose this explicitly over a hard block, keeping the call case by case
- the notebook is per-branch, never a single global list
  Reason: the origin of an item is information — knowing which work produced the pendency changes its priority; a global list loses that and becomes the loose to-do list that rots
- four doors survive: git, pr, spec and upsert
  Reason: the user asked for two (git, pr), but their own flow needs spec to resume work inside the branch, and nothing installs the harness without upsert
- the /mustard door is removed
  Reason: the natural-language routing does not depend on the command — the orchestrator is injected on every user prompt via mustard.json#inject, so typing /mustard before describing the work changes nothing
- scan stops being a door and runs automatically at the base gate
  Reason: everything scan writes is versioned and it refuses a dirty tree; the base gate is the only moment the tree is clean by definition — a freshly updated base before any edit
- qa, close and review stop being doors and become steps of pr
  Reason: none of them is ever what the user wants to do — they are what must happen along the way, exposed as commands by inheritance
- unhook, rehook and the maint doctor fold into upsert as flags
  Reason: they are the same subject as upsert — the state of the installation in this project; three doors to turn one thing on and off is division without a reason
- swapping the wave dispatch engine for the workflow tool is OUT of this spec
  Reason: the research settled the facts — 20 concurrent subagents with the excess FAILING rather than queueing, no native dependency graph, and background agents killed on session pause losing uncommitted work — but replacing the execution engine is its own work unit; this spec is about the flow
- the cancel path for an abandoned unit becomes git delete, not close
  Reason: close is the only door today that cancels a spec the user gave up on; git delete already removes the branch, the remote and the open PR, which is the same outcome expressed once

## Evidence

- the .claude/spec/ directory is carved out of the work-branch gate, so a spec is authored on the protected base with no worktree
  Evidence: `apps/rt/src/hooks/write/work_branch_gate.rs:424`
- git push already rebases onto origin/{base} before committing, so the user's ask to reintegrate the base before pushing is already the behaviour
  Evidence: `plugin/commands/git.md:42`
- there is no merge action — a work branch reaches its base only through a PR
  Evidence: `plugin/commands/git.md:28`
- /mustard:spec in focused mode still prints a header and asks 'Implementar agora?' before dispatching, even when the caller is already inside that spec's branch
  Evidence: `plugin/commands/spec.md:40`
- /review records a verdict but never merges — the merge is always a manual step afterwards
  Evidence: `plugin/commands/review.md:40`
- the natural-language router is injected on every userPromptSubmit, independently of the /mustard command
  Evidence: `mustard.json:11`
- scan refuses to run on a dirty tree because every artefact it writes is versioned
  Evidence: `plugin/commands/scan.md:12`
- the skills door is half inert: create, optimize and eval depend on a Python authoring tool that is not bundled and that this project decided not to use
  Evidence: `plugin/commands/skills.md:22`
- close is today the only door that cancels an abandoned pipeline, via stage Close plus outcome Cancelled
  Evidence: `plugin/commands/close.md:42`
- the QA gate already blocks CLOSE unless a recorded qa.result overall=pass exists, which is the mechanism a pr merge step can read instead of re-running
  Evidence: `plugin/commands/qa.md:40`
- git.flow declares dev as the default base and main as dev's promotion target, so the base gate has two valid bases to accept
  Evidence: `mustard.json:3`