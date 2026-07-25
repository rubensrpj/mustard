---
id: spec.isolate-each-wave-s-implementer
---

# Isolate each wave's implementer subagent in its own git worktree, per the official Claude Code docs: a dedicated plugin subagent carrying isolation: worktree, the writing-role subagent_type mapping pointing at it, and the WorktreeCreate hook cutting an agent worktree from the work unit's HEAD instead of origin/HEAD so a wave inherits the previous waves' work

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

**Today.** A round of waves (a wave = one batch of tasks dispatched to one implementer subagent) runs several implementers **in the same working tree at the same time**. Nothing separates them: the boundary gate warns after the fact, the branch gate reacts on the first edit, and `git add -A` at commit time sweeps whatever the other implementers happen to have written a second ago.

**Why that is a problem.** Two agents editing one tree is the failure the official documentation names outright. [Run agents in parallel](https://code.claude.com/docs/en/agents) puts it as a decision question — *"Do the tasks touch the same files? Isolate the work with worktrees"* — and [Agent teams](https://code.claude.com/docs/en/agent-teams) repeats it as a rule: *"Two teammates editing the same file leads to overwrites."* Every guard this harness added downstream (boundary warnings, scoped commits, branch protection) is a workaround for a separation that the platform offers directly.

**Why now.** The platform closed the last hole. Since v2.1.203 a subagent with `isolation: worktree` runs its shell commands inside its own checkout, and since v2.1.216 a command that redirects git back at the main checkout **fails with an error** — `git -C`, `--git-dir`, `GIT_DIR`, `GIT_WORK_TREE`, or a `cd` first. Isolation stopped being advice an implementer can talk itself out of and became something it cannot do.

**Three facts this repository adds, verified in code, that decide the shape of the work.**

1. **There is nowhere to declare the isolation.** Every writing role resolves to the built-in `general-purpose` agent (`recommended_subagent_type`, apps/rt/src/commands/agent/render/role.rs:309). The plugin owns three subagents and all three are read-only (`mustard-guards`, `mustard-patterns`, `mustard-review`). A built-in agent has no frontmatter this project can edit, so the isolation has nowhere to live until an implementer subagent exists.

2. **The `worktree.baseRef` and `worktree.sparsePaths` settings do not apply here.** This plugin registers a `WorktreeCreate` hook (plugin/hooks/hooks.json:88), and a configured hook **replaces** the native `git worktree add` entirely. Whatever those settings would have done, this hook decides instead.

3. **Turning isolation on today would break the pipeline on the way IN — and it already contradicts the project's declared flow.** The harness names a subagent worktree without an underscore (`agent-…`). For such a name the hook takes the non-unit branch and cuts from `origin/HEAD` (apps/rt/src/commands/work_unit_open.rs:273-278). In this repository `origin/HEAD` resolves to **`origin/main`**, while `mustard.json#git.flow` declares `"*": "dev"` — the project's own statement that every unit starts from `dev`. So the non-unit cut lands on a branch the configuration excludes, and the same file already reads `integration_bases()` and `primary_base()` on the unit branch: the information is at hand and simply not consulted. Two consequences, and both matter before isolation is switched on. First, a wave would start from `main`, with neither the unit's branch nor the previous waves' commits. Second, `worktree.baseRef` cannot fix it: its own default (`"fresh"`) means *the remote's default branch* too, so the native path would land on `main` as well. The base must come from the project's declared flow, never from the remote's idea of a default.

4. **A fresh checkout carries only COMMITTED code, and the scan writes in two places.** Harness state is safe: the workspace resolver redirects every state path from a linked worktree back to the main checkout, so a freshly regenerated code map is visible from an isolated wave even while it is still uncommitted. Code is not safe, because a worktree is a clean checkout — only what is committed travels. And the scan writes both the redirected state artifacts and, sitting next to the code, each subproject's own guard rules and its pattern skills. An isolated wave would therefore read the OLD guard rules, which the review role treats as blocking law. The same holds for any uncommitted edit, scan or not. This repository already carries the doctrine: the scan itself refuses to run on a dirty tree, for exactly the everything-goes-up reason. Isolation extends that one step further — a clean tree becomes a precondition for isolating a wave at all.

5. **And it would break it again on the way OUT — nothing brings the work back.** The documented lifecycle ends with the checkout still on disk: a subagent worktree that finishes *without* changes is removed automatically, one *with* changes is kept, and the periodic sweep never touches a checkout that still holds work. Nothing merges it anywhere. That is correct for the use case the docs describe — the bundled `/batch` skill splits a change into *"5 to 30 worktree-isolated subagents that each open a pull request"*, so the copies are never meant to meet. This pipeline is the opposite shape: its waves converge on ONE work-unit branch, and wave 3 must see what waves 1 and 2 produced. `git-settle` is the exit ritual for the UNIT against its remote base, not for a wave against its unit, so today no step folds a wave's commit back. Isolation without that step buys separation and loses the result.

## Users/Stakeholders

- **The operator** running the pipeline: gets one commit per wave with a real boundary, and stops arbitrating collisions between agents that were never separated.
- **The implementer subagents**: stop being judged by guards that exist only because they share a tree. Their edits cannot reach anyone else's work, and the harness refuses the escape hatches instead of asking them not to use them.

## Success Metric

| Metric | Target |
|---|---|
| Wave implementers dispatched into a shared working tree | 0 |
| Agent worktrees cut from a base that lacks the work unit's commits | 0 |
| Agent worktrees cut from the remote's default branch while the project declares a flow | 0 |
| Waves that ran against code the operator had changed but not committed | 0 |
| Completed waves whose commit never reached the work-unit branch | 0 |
| Merge conflicts on the way back that pass silently | 0 |
| Commits mixing files from two waves of the same round | 0 |

## Non-Goals

- **Not scoping `git add`.** The `add -A` law (plugin/refs/git/git-flow.md:67) stays exactly as written. Isolation makes `add -A` inside a worktree *equal* the wave's boundary — a narrower scope would reintroduce the silent partial commit that law exists to prevent.
- **Not changing wave decomposition, the dependency model, or the dispatch round.** Parallelising by file conflict instead of dependency level is a separate, later unit.
- **Not adopting agent teams.** They are experimental, disabled by default, and forbid nested teams; subagents already report back to the orchestrator, which is the channel this pipeline needs.
- **Not implementing `sparsePaths`.** Fact 2 makes it a change inside the hook, sized on its own evidence — not assumed here.

## Acceptance Criteria

Every criterion below is verified by a NAMED test and demands a non-zero pass count, so a filter that matches nothing cannot report success. All eight fail today, before any of the work exists.

- **AC-1** — when the `WorktreeCreate` hook is asked for an agent worktree (a name with no `_`) from inside a work unit, then the worktree is cut from that unit's HEAD, carrying the previous waves' commits.
  Command: `cargo test -p mustard-rt agent_worktree_cuts_from_unit_head`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-2** — when no work unit is in play and the project declares an integration flow, then the cut falls back to that flow's primary base — never to the remote's default branch. In this project `git.flow` says `dev` while `origin/HEAD` says `main`; the declared flow wins.
  Command: `cargo test -p mustard-rt agent_worktree_falls_back_to_primary_base_not_remote_head`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-3** — when an agent worktree is requested while the main checkout holds uncommitted CODE (anything outside the redirected `.claude/` state), then creation is refused and the message names the offending paths — because that code would not travel into the copy and the wave would silently work against the older version.
  Command: `cargo test -p mustard-rt agent_worktree_refuses_dirty_tree`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-4** — when the plugin's implementer subagent is loaded, then its frontmatter declares `isolation: worktree`, so every dispatch of a writing role lands in its own checkout.
  Command: `cargo test -p mustard-rt impl_agent_declares_worktree_isolation`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-5** — when a wave finishes in its own checkout, then its commit is folded back onto the work-unit branch, so the next wave starts from a tree that contains it.
  Command: `cargo test -p mustard-rt wave_reclaim_folds_commit_onto_unit_branch`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-6** — when the fold-back cannot complete (a conflict, an unmerged wave, an unreachable checkout), then the wave is NOT reported complete and the blocking reason names the files, so no wave is ever marked done while its work sits stranded.
  Command: `cargo test -p mustard-rt wave_reclaim_blocks_completion_on_conflict`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-7** — when a writing role asks for its subagent type, then it resolves to the plugin's isolated implementer instead of the shared built-in `general-purpose`.
  Command: `cargo test -p mustard-rt recommended_subagent_type_routes_writing_roles_to_impl`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-8** — when the documented role-to-subagent map is compared with the code, then the two agree, so a reader is never told that writing roles dispatch to `general-purpose` after they stopped doing so.
  Command: `cargo test -p mustard-rt agent_prompt_ref_matches_subagent_map`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-9** — the workspace builds green.
  Command: `cargo build --workspace`

<!-- PLAN -->

## Files

- `apps/rt/src/commands/work_unit_open.rs` — the `WorktreeCreate` engine: teach the non-unit cut to prefer the work unit's HEAD
- `plugin/agents/mustard-impl.md` (new) — the implementer subagent carrying `isolation: worktree`
- `apps/rt/tests/plugin_agents.rs` (new) — the drift guards pinning the plugin agents to the code
- `apps/rt/src/commands/wave/wave_reclaim.rs` (new) — the way back: fold a finished wave's commit onto the work-unit branch
- `apps/rt/src/commands/wave/cli.rs` — register the new subcommand's enum variant
- `apps/rt/src/commands/wave/mod.rs` — register its dispatch arm
- `apps/rt/tests/run_command_surface.rs` — the locked list of published `run` subcommands
- `apps/rt/src/commands/pipeline/wave_done.rs` — reclaim first, then emit the completion
- `apps/rt/src/commands/agent/render/role.rs` — `recommended_subagent_type`: writing roles resolve to the plugin implementer
- `plugin/refs/agent-prompt/agent-prompt.md` — the canonical role-to-subagent map the flows cite
- `plugin/commands/feature.md` — the inline copy of that map in the Inviolable list

## Boundaries

IN: the files above, plus their tests in the same crate.

OUT: `git add` scope and the `add -A` law; wave decomposition and the dependency model; `worktree.sparsePaths` and build-artifact reuse across checkouts (measure first, size separately); the boundary gate's path matcher; the acceptance-criteria negative test; agent teams; `git-settle`'s unit-level exit ritual, which stays exactly as it is.