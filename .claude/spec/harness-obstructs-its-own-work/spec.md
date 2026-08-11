---
id: spec.harness-obstructs-its-own-work
---

# the harness obstructs its own work at both ends of the unit: the write gate blocks diagnosis evidence before a unit exists, and git-settle prunes the branch before the base advances

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

A work unit has two ends, and the harness currently obstructs itself at both of them.

At the OPENING end, the diagnosis that decides whether there is any work at all happens before a unit exists — so it happens on a protected integration branch, where every write is refused. The cheapest way to choose between two hypotheses is usually to RUN them, and that is exactly what the refusal forbids: the agent falls back to deduction, reaches the same conclusion, and spends several rounds where one execution would have done. The gate is right in principle and expensive at the wrong moment. Worse, once the diagnosis IS finished, everything it established — the terms it settled, the decisions it took, the facts it checked — has to be retyped by hand into the spec, because the flow that opens a bugfix never uses the channel that would carry it. And the hygiene step asks, every single time, whether to continue some other spec first, including when the user just asked for this work explicitly in the same message. A protocol with steps that are routinely skipped in practice teaches the reader to judge every step case by case.

At the CLOSING end, the exit ritual acts before it verifies. It checks out the base, tries to bring it up to date, and then deletes the unit's branch — locally and on the server — whether or not that update succeeded. Only afterwards does it notice the update failed and report it. The verdict is honest and it arrives after the irreversible act: a certificate, not a gate. What it certifies is the worst available state — the local tree does not hold the merged work, and the branch that held it no longer exists anywhere locally. Nothing is truly lost, because the merge is on the server, but "recoverable by whoever knows where to look" is a far weaker promise than a ritual should make, and the fright is real.

The reason the update fails is itself avoidable: the ritual guards the update with a stricter rule than the operation it protects. Git refuses to bring a branch up to date only when doing so would touch a locally modified path; the harness refuses whenever ANYTHING is modified anywhere. Measured in the field, the dirty paths and the advanced paths did not intersect at all, and the same update ran by hand without complaint. Worse still, the most frequent source of that dirt is the harness itself — the files it writes while working are not covered by the ignore list it seeds, so the more it works, the more likely it is to block its own exit.

After this unit: the diagnosis can produce runnable evidence without leaving the protected branch or dirtying the repository; what it established rides into the spec instead of being retyped; the hygiene question fires on a real collision instead of always; the exit ritual verifies before it prunes, so its worst outcome is "I did not advance, your branch is still here"; git decides whether an update is safe; and the harness stops competing with itself for a clean tree.

## Users/Stakeholders

Anyone operating a unit through the harness end to end. The closing-end defect is felt hardest by whoever runs a multi-repository unit, because the exit ritual runs once per repository and each run is another chance to prune on a stale base. The opening-end defects are felt on every bugfix, which is the most common kind of unit there is.

## Success Metric

No irreversible step of the exit ritual runs on an unverified precondition — the prune happens only after the unit's base demonstrably holds the merged work, and the failing path leaves both branches alive with a named next action. Complementarily: a diagnosis can produce executable evidence without violating a gate or dirtying the repository, and a bugfix whose root cause is already proven opens its spec without manual re-entry of that diagnosis.

## Non-Goals

- Changing the merge verification itself. It stays the hard gate that fails closed — this unit extends the same principle to the step after it, it does not relax it.
- Changing how the outcome is reported. The reason names and the distinction between a base that is behind, one that is ahead, and one another checkout holds are correct as they stand; the problem was never the quality of the verdict, only its timing.
- Ignoring untracked files as a special case in the advance guard. That repair targets a guard this unit removes outright, so shipping it would mean writing code to delete in the same breath.
- Modelling a diagnosis phase as a formal pipeline stage. The diagnosis stays what it is — conversation before a unit — and this unit only stops punishing it.
- Making the write gate accept throwaway evidence that must compile inside a crate. No carve-out under the harness directory can achieve that, and pretending otherwise would be a promise the mechanism cannot keep.
- Re-litigating how a unit is named. The field report asked that an explicitly passed name win over the derived one. The derivation is deliberate, it already reports the name it superseded rather than swapping silently, and reverting it would reopen the two-names-for-one-unit defect it was introduced to close.
- Re-implementing three things that already ship, each verified in the code before this spec was drafted: the isolation topology (a unit's branch IS the isolation, cut in place, with a submodule resolving its own base and a fresh worktree populating its submodules), the multi-repository close order (one branch per repository, submodule pull requests first, the parent held as a draft, the pointer bumped before it merges), and the mutation-sensitivity check for acceptance criteria (red before the work, green after, and a third pass that removes the work again to catch a criterion that verifies nothing).

## Acceptance Criteria

- AC-1 — when the unit's base did NOT advance, then the settle pass prunes nothing and answers `ok:false`, leaving the local branch and the remote branch alive. Command: `cargo test -p mustard-rt prune_waits_for_the_base_to_advance`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt git_settle`
- AC-2 — when the base advanced, then the prune is authorised regardless of whether the unit's own commit is reachable from the base, so a squash-merged unit still settles. Command: `cargo test -p mustard-rt prune_authorisation_reads_the_base_advance_not_unit_ancestry`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt git_settle`
- AC-3 — when the in-place exit could not free the floor, then the remote branch survives together with the local one. Command: `cargo test -p mustard-rt a_blocked_exit_leaves_the_remote_branch_alone`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt git_settle`
- AC-4 — when the working tree is dirty ONLY in paths the advance does not touch, then the base still fast-forwards and the report says `updated:true`. Command: `cargo test -p mustard-rt a_dirty_tree_the_advance_does_not_touch_still_fast_forwards`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt git_settle`
- AC-5 — when git refuses the fast-forward, then the report separates a genuine divergence from a merely dirty tree, so the operator is pointed at the real obstacle Command: `cargo test -p mustard-rt a_refused_advance_separates_divergence_from_dirt`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt git_settle`
- AC-6 — when a write on a bare integration base targets `.claude/scratch/`, then the gate allows it, cuts no branch, and keeps the pending marker for the first in-repo edit. Command: `cargo test -p mustard-rt scratch_evidence_is_writable_on_a_protected_base`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt work_branch_gate`
- AC-7 — when a fresh project is seeded with the shipped ignore template and every path the runtime actually writes is created, then git reports nothing dirty — the paths come from the writers in the code, not from a list the test chose for itself Command: `cargo test -p mustard-core the_seeded_ignore_hides_every_path_the_runtime_writes`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-core claude_paths`
- AC-8 — when the bugfix flow reaches its spec step, then its prose instructs assembling the material file and passing `--material` to `spec-draft`. Command: `cargo test -p mustard-rt bugfix_prose_teaches_the_material_channel`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt prose_teaches`
- AC-9 — when the hygiene ref describes step 3, then it conditions the question on overlap with the active spec instead of asking unconditionally. Command: `cargo test -p mustard-rt hygiene_prose_teaches_the_collision_condition`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt prose_teaches`
- **AC-11** — when the unit's base already HOLDS origin's tip (it is ahead of origin, so the fetch refuses), then the prune is authorised — the gate reads the fact, not the fetch exit status
  Command: `cargo test -p mustard-rt a_base_ahead_of_origin_authorises_the_prune`
  Expect: `[1-9][0-9]* passed`
- **AC-12** — when the write gate allows a scratch path in this repository, then git also ignores it, so an add -A cannot sweep throwaway evidence into the unit
  Command: `git check-ignore --no-index .claude/scratch/probe.sh`
- **AC-13** — when the harness seeds its ignore file over one that already exists, then the lines missing from it are appended instead of the whole file being skipped, so an already-initialised project receives new entries
  Command: `cargo test -p mustard-core seeding_over_an_existing_ignore_adds_the_missing_lines`
  Expect: `[1-9][0-9]* passed`
- **AC-14** — when the prune is refused on an in-place unit, then the checkout is restored to the unit branch and the report says so, so the refusal leaves the operator where the work is
  Command: `cargo test -p mustard-rt a_refused_prune_restores_the_unit_branch`
  Expect: `[1-9][0-9]* passed`
- AC-10 — the workspace still builds. Command: `cargo build --workspace`

<!-- PLAN -->

## Files

| File | What changes | Wave |
|---|---|---|
| `apps/rt/src/commands/git_settle.rs` | The advance verdict is computed before the prune and gates it; the remote delete moves inside the floor guard; the pre-check and its two exemptions are deleted so `merge --ff-only` decides. Second review round: the refused-advance diagnosis asks git whether a fast-forward was possible before it blames the tree, and `restoredToUnit` answers from where the checkout IS rather than from the restore's exit status. | 1 |
| `apps/rt/src/hooks/write/work_branch_gate.rs` | `.claude/scratch/` joins `.claude/plans/` as an exemption from branch protection — allowed, cuts no branch, keeps the pending marker. | 2 |
| `packages/core/templates/.gitignore` | Adds `scratch/`, `feature-digest.json`, `spec/*/qa-report.json` and `spec/*/qa-report.html` — the sanctioned scratch path plus the artefacts the runtime writes and nothing ignored. The `.html` entry was added by the implementer beyond the original four and is kept: the same renderer writes it, and this repository's own root ignore already carries it. `spec/*/qa/` was dropped in review (see Decisions). Second review round: the remaining runtime writes join them — `.compact-state/`, `.dispatch/`, `spec/*/.dispatch/`, `knowledge/`, `memory/`, `agent-memory/`, `.agent-memory/`, `.session/`, `.obsidian/`, `settings.local.json`, `.dashboard.pid`, `.dashboard.port`, `spec/*/.memory-approved`, `spec/*/economy-baselines.json` and the three per-wave renders. | 2 |
| `.claude/.gitignore` | This repository's own copy of the seed, which the merge-first seeder had left frozen at the pre-`scratch/` version — brought level with the template so the write gate's carve-out is actually ignored here (AC-12), and kept byte-identical to it through the second round. | 2 (cascade) |
| `plugin/commands/git.md` | Declared cascade, found in review by BOTH reviewers: the `pr close` procedure described only the happy path, so `base-behind`, `baseAdvance.reason`, `nextAction` and `restoredToUnit` — every mechanism this unit adds — reached the operator as raw JSON with no prose. The procedure now names the refusal, the three obstacles it distinguishes, and the standing ban on finishing a refused settle by hand. | 3 (cascade) |
| `plugin/refs/git/submodule-rules.md` | Declared cascade of the same finding: step 1 enumerates the refusals a per-repo settle can produce, so leaving the new one out would have described one mechanism two ways — the defect the root reviewer raised for `/bugfix` vs `/feature`. One clause, pointing at the shape `/git` now spells out. | 3 (cascade) |
| `packages/core/src/platform/project_seed.rs` | Declared cascade, found in review: `seed_gitignore` preserved an existing ignore file WHOLE, so a new pattern reached fresh projects only. It now merges by line — the patterns the file lacks are appended under an attributed header, the user's lines and comments untouched, and a file already carrying them all stays `Preserved`. | 2 (cascade) |
| `packages/core/tests/seeded_ignore.rs` | New: seeds the template into a temporary repository, writes each artefact, and asserts the repository reports nothing dirty. Second review round: the test no longer chooses its own list — the paths come from the WRITERS (each entry citing `file.rs:line`) and from every `.claude/`-scoped rule of this repository's own root `.gitignore`, turned into concrete sample paths, so a rule this project has to add tomorrow is asked of the template too. | 2 |
| `plugin/commands/bugfix.md` | DIAGNOSE names the scratch path for runnable evidence; its findings, decisions and definitions are assembled and passed to the draft through the existing material channel; a proven root cause drafts a minimal spec. Added in review: §3 now carries the same ordering warning `/feature` §2.2 spells out — `.claude/.cache/` is NOT the §1 carve-out. Second round: the warning also names the case that actually happens — a pending marker cuts the branch ON that write and it lands — so the two flows describe one mechanism identically. | 3 |
| `plugin/refs/feature/spec-hygiene.md` | Step 3 asks only on overlap with the active spec, or when the new work was not explicitly requested in the same message; otherwise one recorded line and proceed. | 3 |
| `apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs` | Two ratchets in the file's existing shape, holding both prose changes to the behaviour they describe. Second review round: a third ratchet holds `/git`'s refusal paragraph to the fields git-settle emits, and the bugfix ratchet gains the ordering-warning half. | 3 |
| `apps/rt/src/shared/branch_state.rs` | Declared cascade, not planned work: wave 1 moved the settle to `try_merged_refs`, which left `merged_refs` a wrapper with test-only callers and a `dead_code` warning. The wrapper is deleted rather than silenced, its documentation folded into the function that survives, and its two test callers read the pair directly. | 1 (cascade) |

## Boundaries

IN: the ordering and the authorisation criterion of the exit ritual's prune; the fast-forward guard inside the same command; the write gate's exemption list; the seeded ignore list; the bugfix flow's use of the existing material channel; the hygiene ref's question condition; tests for each of the above.

OUT: the merge verification (`is_merged`) and its provider fallback; the reason vocabulary and the `updated:false` distinction in the settle report; the multi-repository close order and the submodule RULES, which are already correct (their prose gained one clause in the second review round, naming the refusal a per-repo settle can now produce — the rules themselves are untouched); the `pr close` protocol text, verified as already exempting an in-place unit; `spec-draft`'s material channel itself, which needs no change to be used; any new command, flag or configuration knob.

## Definitions

- **base advanced** — the local integration base holds origin's tip after the ff-only advance — git-settle already computes it as `base_advanced` (git_settle.rs:702-706). It is the ONLY sound authorisation for the prune, because it is the fact that makes the merged work present in the local tree.
- **the prune** — the three irreversible steps of the exit ritual: `worktree remove`, `branch -D` and `push origin --delete` (git_settle.rs:617-641). What comes after it can only certify, never gate.
- **scratch evidence** — throwaway code written during DIAGNOSE for the single purpose of deciding between two hypotheses by running them — never committed, never part of the unit. Distinct from a test, which asserts and stays.
- **harness artefact** — a file the Mustard runtime itself writes into `.claude/` while working: the feature digest, a spec's QA renders, the rendered dispatch prompts, compaction memory, the knowledge and memory stores, the per-machine settings and the per-wave renders. It is neither code nor user state, and it is what makes the harness dirty its own tree between rituals. Narrowed in review: a spec's `qa/report.md` is NOT one of these — it is the unit's own record of which criteria passed, the peer of `review/verdict.md`, and the close commits it (see Decisions). Widened in the second round: the first list named three sidecars while the runtime writes a dozen paths, so the definition now points at the writers and at this repository's own ignore list rather than enumerating a subset.

## Decisions

- the prune is authorised by `base_advanced`, never by the unit's commit being an ancestor of the base
  Reason: the field report proposed local ancestry of the unit's commit. That criterion is false FOREVER for a squash merge — the portal rewrites the commit, so the unit's sha never becomes an ancestor of the base. `is_merged` (git_settle.rs:351-373) accepts provider evidence for exactly that case, so an ancestry gate would strand every squash-merged unit permanently. `base_advanced` is the fact the report actually wants, and the command already computes it — 65 lines too late.
- the `push origin --delete` moves inside the `floor_clear` condition
  Reason: at git_settle.rs:634 it sits outside it, so a settle that could NOT free the local floor still deletes the server branch — the worst half of the prune runs on the failing path.
- the pre-check inside `update_bases` is removed; `git merge --ff-only` becomes the authority
  Reason: git verifies before it writes and refuses without side effects, so the guard cannot protect anything the operation does not protect itself — it only refuses MORE cases. Measured in the field: a tree dirty in `documentacao/` and `scripts/` while the advance carried `partners.graphql.ts` fast-forwarded by hand, and the guard had refused it. The `dirty-tree` diagnosis survives, produced AFTER the refusal instead of instead of the attempt.
- the report's proposal 3 (ignore untracked `??` lines) is dropped
  Reason: it repairs the same guard proposal 2 deletes; shipping both means writing code to be removed in the next wave. Dropping it also loses nothing: proposal 2 covers every case proposal 3 covered, plus the tracked-dirt case it never could.
- the gitlink exemption in `blocks_fast_forward` goes with the guard
  Reason: it exists only to compensate the pre-check — git accepts a gitlink-only tree by itself (measured on git 2.53 per the function's own doc), so with no pre-check there is nothing left to exempt.
- the sanctioned scratch path is `.claude/scratch/`, not a new root
  Reason: the write gate already exempts `.claude/plans/` and the workspace resolver already redirects the whole `.claude/` tree to the main checkout. Inventing `.mustard/scratch/` would open a second harness root for one carve-out.
- evidence that must COMPILE inside the crate stays outside this carve-out, and the spec says so
  Reason: cargo does not compile a file under `.claude/`, so no carve-out can make a throwaway Rust integration test work there. The honest path for that case is opening the unit early — which wave 3 makes cheap by letting the diagnosis ride into the spec instead of being retyped.
- the seeded ignore merges by LINE, and this repository's own copy is brought level with it
  Reason: found in review. Wave 2 taught the write gate to ALLOW `.claude/scratch/` while the matching `scratch/` line landed only in the template, and `seed_gitignore` preserves an existing file whole — so no already-initialised project, Mustard included, would ever receive it. Under the standing `add -A` law of `/git`, the first diagnosis to follow the new instruction in `bugfix.md` would have committed its throwaway probe into the unit, and the prose promising otherwise would have been measurably false where it ships. Preserving the file whole is right for a document and wrong for a rule list; the fix is per-line, so the user's own rules, order and comments survive and the merge converges after one run.
- `spec/*/qa/` is NOT ignored — only the machine renders of the QA are
  Reason: found in review. The template hid `spec/*/qa/` while this repository versions 31 `qa/report.md` files, so new projects would lose exactly what this one keeps. `qa/report.md` is the transcript of WHICH criteria passed — the unit's record, peer of `review/verdict.md`, swept into the close commit — while `qa-report.json`/`.html` are regenerable renders of the same run. AC-7's own negative control already states the principle ("a spec belongs to its unit and stays versioned"), and this repository's root ignore had already drawn the line in the same place, enumerating the sidecars and leaving `qa/` out. The test now proves both directions: the sidecars are hidden, the transcript is seen.
- the prune gate reads the FACT that the base holds origin's tip, not the exit status of the advance
  Reason: `fetch origin <base>:<base>` rewinds nothing, so it refuses a base that is AHEAD of origin — the one shape where the local tree holds MORE of the merged work than the server. With the gate now stopping the prune, that refusal became a deadlock: `ok:false` + `base-behind` about a base that already had everything, a `nextAction` that reproduced itself byte for byte, and no sanctioned escape (the close protocol forbids pushing a base directly and forbids finishing a refused settle by hand). `update_bases` already asks git which of the three refusals happened and names one `ahead-of-origin`; the gate now reads that. The report is untouched — `updated` still means the advance ran.
- the refused advance is diagnosed by asking GIT whether a fast-forward was possible, not by looking at the tree
  Reason: found in the second review round. `dirty-tree` was claimed for ANY refusal over a non-empty `status --porcelain`, so a genuine divergence that happened to coincide with an unrelated modified file reported `dirty-tree` — and sent the operator to tidy files while the obstacle was the history. The question that separates the two is whether the base is an ancestor of origin's tip: if it is, the advance WAS a fast-forward and only a path in the tree can have stopped it; if it is not, no cleaning will ever help. The vocabulary is untouched (the Non-Goals keep it) — what changed is which of the two existing names is chosen, and the dirt must still be there for `dirty-tree` to be claimed, so a refusal neither name explains falls to the one that promises least. AC-5 was amended to state the separation instead of the old tree-only rule.
- `restoredToUnit` answers from WHERE THE CHECKOUT IS, not from the restore's exit status
  Reason: found in the second review round. Computed from "did our `checkout <unit>` succeed", the field read `false` in the one case where the operator needed it most: when the exit checkout was ITSELF refused, nobody ever left the unit branch, so a pass that changed nothing reported the same `false` as a pass that stranded the session on the base. Reading the fact is the same principle the prune gate already follows for `holds_origin_tip` — and the third leg of AC-14's test now stages exactly that refusal.
- the seeded ignore is measured against the runtime's writers AND this repository's own root ignore
  Reason: found in the second review round. The previous test picked its own four-item list and passed while SEVEN other runtime paths stayed untracked in a freshly seeded project — the same omission class as the round before, different entries, because a list the test chooses can only prove what the author already thought of. The seed now carries every `.claude/`-scoped rule this repository has had to add over its life, and the test derives its sample paths from that file: the field record is external to the test, and the day a rule is added there this test asks for it in the template too. AC-7 was amended to say the paths come from the writers rather than from the test.
- the refusal gets operator prose in `/git`, not only fields in the JSON
  Reason: found in the second review round by both reviewers independently. `git.md:47` described the second settle as "(pull, remove the worktree, delete local + remote branch)" — true of the happy path only. Every mechanism this unit adds is met by the operator through that one procedure, so a refusal with no prose is the inert half this repository keeps a ratchet file to prevent: the fields ship, nobody is told to read them, and the improvisation that follows is the manual `branch -D` the refusal exists to stop.
- the report's closing note on the `pr close` protocol needs no change
  Reason: verified against the shipped prose: `plugin/commands/git.md:23` already reads `an IN-PLACE unit (no worktree) is exited by settle itself`, and `git.md:47` already scopes `ExitWorktree` to a unit that has a worktree. The note describes a state of the doc that is already past.

## Evidence

- the prune runs before anything checks whether the base advanced: checkout at 593, the advance attempt at 604, the three irreversible prune steps at 617-641, and only at 707 does `pass_is_ok` read the advance — so `ok:false` describes a state already consumed
  Evidence: `apps/rt/src/commands/git_settle.rs:617`
- `push origin --delete` is evaluated outside the `floor_clear` guard that protects the local delete, so the remote branch dies even when the checkout that frees the floor was refused
  Evidence: `apps/rt/src/commands/git_settle.rs:634`
- `is_merged` accepts provider evidence when git alone cannot prove containment — the case of a portal that rewrites commits on merge (squash) — which is why a local-ancestry prune criterion would refuse every squash-merged unit
  Evidence: `apps/rt/src/commands/git_settle.rs:351`
- `blocks_fast_forward` treats every `status --porcelain` line as blocking dirt except `.claude/worktrees/` and a moved gitlink, so it refuses advances that `merge --ff-only` would perform safely
  Evidence: `apps/rt/src/commands/git_settle.rs:401`
- the seeded `.claude/.gitignore` covers `.cache/`, `.harness/`, `.metrics/`, `.agent-state/`, `.pipeline-states/`, `worktrees/`, `spec/*/.events/` and `spec/*/.blobs/` — and neither `feature-digest.json` nor any spec's `qa-report.json`/`qa/`, which are exactly the artefacts found blocking the exit ritual in the field
  Evidence: `packages/core/templates/.gitignore`
- `feature-digest.json` is written unconditionally by the digest command into `.claude/`, where nothing ignores it
  Evidence: `apps/rt/src/commands/feature.rs:842`
- the write gate exempts exactly two things from branch protection — a path outside the repo, and `.claude/plans/`; there is no exemption for gitignored paths or for throwaway evidence
  Evidence: `apps/rt/src/hooks/write/work_branch_gate.rs:286`
- `spec-draft` already accepts `--material` carrying definitions/decisions/findings and lands them in sections of their own, so the diagnosis channel exists — `/bugfix` simply never passes it
  Evidence: `apps/rt/src/commands/spec/cli.rs:164`
- spec hygiene asks whether to continue an in-progress spec unconditionally, including when the user just asked for the new work explicitly in the same message
  Evidence: `plugin/refs/feature/spec-hygiene.md:12`
- REFUTED — the `pr close` protocol does NOT tell an in-place unit to run ExitWorktree: the action table already exempts it, and so does the procedure line
  Evidence: `plugin/commands/git.md:23`