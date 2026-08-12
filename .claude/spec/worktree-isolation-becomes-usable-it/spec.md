---
id: spec.worktree-isolation-becomes-usable-it
---

# worktree isolation becomes usable: it carries the environment the project declares, links what is heavy, reports what did not travel, cleans up after itself, and the gate isolates a second unit instead of taking over the checkout

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

worktree isolation becomes usable: it carries the environment the project declares, links what is heavy, reports what did not travel, cleans up after itself, and the gate isolates a second unit instead of taking over the checkout.

Working on two things at once is routine here, and the harness has no honest answer for it.

Today a work unit lives on a branch in the main checkout. That is cheap and it is right — for ONE unit. When a second session in the same folder opens another unit, the gate checks that unit's branch out in the same directory, and a checkout carries uncommitted work along. The first session keeps editing, believing it is on its own branch, while its edits land on the second one. Nothing warns, because from each session's point of view everything is correct: each knows which branch it wants, and neither knows the other exists.

The mechanism that solves this already exists — a worktree gives each unit its own directory over one shared repository — and it is precisely the mechanism people stop using. The reason is narrow and fixable: a worktree is cut from git, so it receives only what git tracks. The project's `.env` is not there. Its dependencies are not there. What should be a second workspace is a directory that cannot run, and the operator goes back to whatever they were doing before.

Everything else that looks wrong about worktrees follows from that one fact, or from a second one just as narrow: nothing reaps them. The collector that would clean up runs at every session start, in simulation, and prints a warning above a threshold — it has never removed anything. Its age rule waits seven days. And the one worktree the harness itself creates for a proof lives outside the only directory the collector walks, so an interrupted run leaks a directory that nothing can ever collect. This session produced exactly that.

The alternative considered and rejected was separate clones. It works, and for occasional parallel work it is the simpler answer — but it splits the harness's own memory: metrics, knowledge, the census and the list of units in flight would live half in each clone. For someone who works in parallel routinely, that split is not an episode, it is the permanent state of the tool.

After this unit: the project declares what a worktree needs, so a second workspace is usable the moment it exists; heavy directories are linked rather than copied, so it exists in seconds; whatever could not travel is named before you hit it; an orphaned worktree is collected because its owner is gone, not after a week; and a second unit is isolated instead of taking over the checkout someone else is working in.

## Users/Stakeholders

Anyone who keeps one unit in review while starting the next — which, for this project's operator, is the normal working day rather than the exception. The silent-loss defect is felt by whoever runs two sessions on one folder; the unusable-worktree defect is felt by anyone who tried the isolation once and went back to working in series.

## Success Metric

Two sessions on the same folder can hold two units at once without either losing work, and the isolated one is usable without manual setup: the declared environment is present, or what is missing is named. Complementarily, a worktree whose owner no longer exists does not survive the next session start.

## Non-Goals

- Removing worktree support. It was considered seriously and rejected on evidence: Claude Code cuts worktrees on its own for isolated tasks, so they appear in the repository whether or not the harness ever creates one. Dropping the layer that handles them would not remove the directories, only the handling.
- Guessing which ignored paths a project needs. The harness never infers this — a project declares it once, or nothing travels. Copying everything ignored would drag `node_modules` and `target` along; a hand-picked default list is the curated-list habit this project rejects elsewhere.
- Changing the `.claude/` redirect. A worktree resolving harness state to the main checkout is what keeps one acervo instead of two, and it is the reason worktree beats clone for parallel work at all.
- Changing what the collector refuses. It already declines a unit's worktree (that is the exit ritual's job) and any worktree holding uncommitted or untracked work. Those two guards are what make acting safe; this unit makes it act, it does not widen what it may touch.
- Making the first unit move. The checkout belongs to whoever is already in it; isolation applies to the unit arriving second, never to the one already working.

## Acceptance Criteria

- AC-1 — when a project declares a `carry` path and a worktree is cut, then that path exists inside the worktree as a real copy. Command: `cargo test -p mustard-rt a_declared_carry_path_lands_in_a_fresh_worktree`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt work_unit_open`
- AC-2 — when a project declares a `link` path, then the worktree reaches the main checkout's copy instead of duplicating it. Command: `cargo test -p mustard-rt a_declared_link_path_reaches_the_main_checkout`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt work_unit_open`
- AC-3 — when a declared path cannot travel, then the creation still succeeds and the report names that path. Command: `cargo test -p mustard-rt what_did_not_travel_is_named_and_never_aborts`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt work_unit_open`
- AC-4 — when a worktree's owning process no longer exists and it holds no work, then the session-start collector removes it without waiting for any age threshold. Command: `cargo test -p mustard-rt an_orphan_worktree_is_collected_without_waiting_for_age`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt worktree_gc`
- AC-5 — when a worktree holds uncommitted or untracked work, then the acting collector still refuses to remove it, whatever its owner or age. Command: `cargo test -p mustard-rt the_acting_collector_still_refuses_a_worktree_holding_work`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt worktree_gc`
- AC-6 — when a removal-proof worktree is left behind by an interrupted run, then it is within the collector's reach and is collected. Command: `cargo test -p mustard-rt an_abandoned_removal_worktree_is_within_reach_and_collected`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt worktree_gc`
- AC-7 — when the checkout already holds a different unit's branch, then the new unit is cut into its own worktree and the checkout is left untouched. Command: `cargo test -p mustard-rt a_second_unit_is_isolated_instead_of_taking_the_checkout`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt work_branch_gate`
- AC-8 — when a second unit is isolated, then the first unit's uncommitted work stays on the first unit's branch. Command: `cargo test -p mustard-rt the_first_units_uncommitted_work_stays_where_it_was`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt work_branch_gate`
- AC-9 — when the operator prose describes isolation, then it teaches the declared environment and the second-unit rule. Command: `cargo test -p mustard-rt worktree_prose_teaches_the_declared_environment`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt prose_teaches`
- AC-10 — the workspace still builds. Command: `cargo build --workspace`

<!-- PLAN -->

## Files

| File | What changes | Wave |
|---|---|---|
| `packages/core/src/domain/config.rs` | New `worktree` section beside `git`: `carry` (paths copied) and `link` (heavy directories pointed at the main checkout). Both default empty — a project that declares nothing behaves exactly as today. | 1 |
| `apps/rt/src/commands/work_unit_open.rs` | A freshly cut worktree is populated from that declaration, next to the submodule step and with the same forgiving posture: a failure warns loudly and never aborts the creation. Reports what did not travel. | 1 |
| `apps/rt/src/commands/maint/worktree_gc.rs` | The session-start probe stops running in simulation and collects what is orphaned; ownership is read from the creator's process id instead of waiting seven days; the search widens to the temp-dir prefix the removal proof uses. Both refusals stay untouched. | 2 |
| `apps/rt/src/commands/review/work_removed.rs` | The removal-proof worktree becomes reachable by the collector, so an interrupted run no longer leaks a directory nothing can reap. | 2 |
| `apps/rt/src/hooks/write/work_branch_gate.rs` | Before checking a unit's branch out, the gate asks what the checkout is on: an integration base cuts in place as today; another unit's branch cuts a worktree instead of taking the checkout. The standing "EnterWorktree" nudge is retired. | 3 |
| `plugin/refs/git/git-flow.md` | Operator prose: a worktree is what a SECOND unit gets, the project declares what it carries, the collector reaps what is orphaned. | 3 |
| `apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs` | One ratchet holding that prose to the shipped behaviour, in the file's existing shape. | 3 |

## Boundaries

IN: the project's declaration of what a worktree receives; populating a fresh worktree from it; naming what did not travel; making the existing collector act and keying it on ownership; bringing the removal-proof worktree within its reach; the gate's decision between cutting in place and isolating; the operator prose for all of it.

OUT: removing worktree support (considered and rejected — Claude Code cuts them regardless); inferring the environment instead of declaring it; the `.claude/` redirect that keeps harness state single; what the collector refuses to touch (unit worktrees, and any worktree holding work); moving a unit that is already established in the checkout; the separate-clone workflow, which stays available and unmanaged.

## Definitions

- **carry** — copy a git-ignored path from the main checkout into a freshly cut worktree. For small, environment-specific files the project cannot run without — `.env` and its siblings. The project DECLARES them; the harness never guesses.
- **link** — point a heavy regenerable directory in the worktree at the main checkout's copy (a directory junction on Windows, a symlink elsewhere) instead of duplicating it — `node_modules`, `target`. Copying these is what makes a worktree take ten minutes to become usable; linking makes it seconds.
- **orphan worktree** — a worktree whose creator is gone — a killed process, a crashed task. Distinct from an IDLE one, which still has an owner. The distinction matters because only an orphan is safe to collect without asking.
- **the second unit** — a work unit opened while the main checkout already holds another one. Today it TAKES the checkout by checking its own branch out, carrying the first unit's uncommitted work along; this is the case worktree isolation exists to serve.

## Decisions

- the environment is DECLARED in mustard.json, never inferred
  Reason: the harness cannot know which ignored paths a project needs to run. Copying all of them drags `node_modules` and `target` along; copying a hand-picked subset is the curated list this project rejects on principle. A declaration is neither: the project states it once, and the harness obeys.
- two verbs, `carry` and `link`, because the costs are opposite
  Reason: a `.env` is bytes and must be a real copy (a link would leak edits back into the main checkout). `node_modules` and `target` are gigabytes and regenerable, so a link is both cheaper and harmless. One verb for both would be wrong for one of them.
- the collector starts ACTING, and only where it is already proven safe
  Reason: it already refuses to remove a worktree that holds uncommitted or untracked work — that guard is what makes acting safe. What it does today is run in dry-run at every session start and print a warning above a threshold, which is the report-instead-of-resolve shape this project keeps finding in itself.
- ownership is read from the PID the removal worktree already carries in its name
  Reason: `mustard-removal-{slug}-{pid}` (work_removed.rs:321) names its creator. Asking whether that process still exists answers 'orphan or busy' exactly, so the collector never has to wait days to be sure — the 7-day threshold exists only because nothing else told it.
- a second unit gets a worktree instead of taking the checkout
  Reason: this only became reasonable once the environment travels. It was rejected earlier in the same conversation precisely because a worktree that lacks `.env` and dependencies sends the operator into a directory that cannot run — trading a silent defect for a different one.
- worktree stays, against the reviewer-of-record's own first recommendation
  Reason: the alternative (separate clones) splits the harness's own memory: metrics, knowledge, census and the in-flight spec list live per clone, so a project that works in parallel routinely would keep half its history in each. The user confirmed parallel work is routine, which is the fact that decides it.

## Evidence

- a freshly cut worktree receives only versioned files plus submodules — `init_submodules` is the ONLY population step, so every git-ignored path the project needs is absent
  Evidence: `apps/rt/src/commands/work_unit_open.rs:513`
- the project config carries only `flow` and `provider`, so there is nowhere to declare what a worktree must receive
  Evidence: `packages/core/src/domain/config.rs:61`
- the session-start collector runs in DRY-RUN and only prints a warning above a threshold — it never removes anything, at any age
  Evidence: `apps/rt/src/commands/maint/worktree_gc.rs:371`
- the collector's age threshold is 7 days, so a worktree orphaned minutes ago is invisible to it for a week
  Evidence: `apps/rt/src/commands/maint/worktree_gc.rs:362`
- the removal-proof worktree is cut under the OS temp dir, outside `.claude/worktrees/`, which is the only tree the collector walks — so no collector has ever been able to see it
  Evidence: `apps/rt/src/commands/review/work_removed.rs:321`
- that same path embeds the creating process id (`mustard-removal-{slug}-{pid}`), which is exactly the signal needed to tell an orphan from a live owner without waiting for age
  Evidence: `apps/rt/src/commands/review/work_removed.rs:321`
- the write gate checks out the unit's branch whenever the session marker names one, without asking whether the checkout already holds a DIFFERENT unit — and the checkout carries uncommitted work along, so a second session silently moves the first session's edits onto its own branch
  Evidence: `apps/rt/src/hooks/write/work_branch_gate.rs:328`
- FIELD EVIDENCE — `git worktree list` in this very session returned three entries and NONE came from unit isolation: the main checkout, another session's scratch build, and an orphaned removal-proof worktree left behind when a close was interrupted
  Evidence: `apps/rt/src/commands/maint/worktree_gc.rs:5`
- the collector already refuses to remove a worktree holding uncommitted or untracked work, and refuses unit worktrees outright — the two guards that make acting safe are already written and tested
  Evidence: `apps/rt/src/commands/maint/worktree_gc.rs:25`
- Claude Code itself cuts a worktree for every task dispatched with worktree isolation, so worktrees appear in the repository whether or not the harness ever creates one
  Evidence: `apps/rt/src/commands/maint/worktree_gc.rs:5`