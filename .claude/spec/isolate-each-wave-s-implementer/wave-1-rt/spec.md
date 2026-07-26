---
id: wave.isolate-each-wave-s-implementer.1-rt
---

# wave-1-rt

## Summary

The way IN: an agent worktree is cut from the work unit's HEAD; without a unit, from the flow's primary base — never the remote's default; and never at all while the tree holds uncommitted code.

## Network

- Parent: [[spec.isolate-each-wave-s-implementer]]

## Tasks

- [ ] In `hook_create`, the non-unit branch (a name with no `_`) resolves the base to `origin/HEAD`. Replace that single resolution with an explicit three-step cascade: (1) the CURRENT work unit's HEAD when the invoking tree sits on one; (2) `origin/{primary_base}` from `mustard.json#git.flow`; (3) the local `HEAD` as the last resort. Each step is tried only when the one before it does not resolve.
- [ ] Step 2 is the point of this wave as much as step 1. `origin/HEAD` is the REMOTE's opinion of a default; in this project it resolves to `origin/main`, while `git.flow` declares `"*": "dev"`. The project's declared flow is the authority — the same file already reads `integration_bases()` and `primary_base()` on the unit branch, so this consults configuration that is already loaded, not a new source of truth. A project that declares no flow keeps today's `origin/HEAD` behaviour.
- [ ] Recover the unit for step 1 from the tree the hook was invoked in — the same `{base}_{slug}` shape `super::event::work_branch` produces and `base_for` parses. Never infer a branch from the agent worktree name; it carries none.
- [ ] Keep every existing behaviour intact: a `{base}_…` name with a declared base still fetches and cuts from a fresh `origin/{base}`; an undeclared prefix still returns the didactic Err; an already-registered branch still returns its registered path.
- [ ] Fail-open in this hook's own terms: when a step cannot be resolved, fall through to the next rather than abort. A non-zero exit ABORTS worktree creation, so a resolution failure must never become an abort.
- [ ] Test `agent_worktree_cuts_from_unit_head`: a repo on a `{base}_{slug}` work branch carrying a commit the base lacks; call `hook_create` with an `agent-*` name; assert the created worktree's HEAD contains that commit.
- [ ] Test `agent_worktree_falls_back_to_primary_base_not_remote_head`: a repo whose `origin/HEAD` points at `main` and whose `mustard.json#git.flow` declares `dev` as primary, sitting on no work unit; assert the agent worktree is cut from `dev` and NOT from `main`. Companion: a repo with no declared flow still lands on the historical `origin/HEAD` base.
- [ ] Refresh before cutting, best-effort, mirroring `work_branch_gate::refresh_integration_bases`: fetch so the base is current, and fall back to the LOCAL base when the fetch fails (offline, no remote, diverged). Never block on the refresh — a stale-but-local base is a worse cut, not a failure.
- [ ] Add the clean-tree precondition, scoped to AGENT worktrees only. A fresh checkout carries only COMMITTED code, so uncommitted work in the main checkout would not travel and the wave would run against the older version — most visibly after `/scan`, which rewrites each subproject's `CLAUDE.md` `## Guards` and its `{role}-pattern` skills next to the code. Refuse creation and name the offending paths. Unit worktrees and background-session worktrees keep today's behaviour untouched: they have no wave depending on the tree's state.
- [ ] Exclude `.claude/` from the dirty count — it is redirected state, not code (packages/core/src/io/workspace.rs:41), and the harness already uses that same carve-out in other gates. Untracked files count as dirty: a new source file that never travels is the same defect as a modified one.
- [ ] This refusal is deliberate and is the ONE place in this wave that blocks. `WorktreeCreate` aborts creation on a non-zero exit with stderr shown to the user — that IS the event's protocol, the same way a Deny is a gate's. It mirrors `scan-clean-gate`, which already refuses `/scan` on a dirty tree for the `add -A` reason. Proceeding would mean the wave works on stale code and nobody notices; that is the failure worth stopping the line for.
- [ ] Test `agent_worktree_refuses_dirty_tree`: a repo with an uncommitted change to a source file; assert `hook_create` with an `agent-*` name returns Err naming that path and creates no worktree. Companions: an uncommitted change confined to `.claude/` still creates the worktree; a dirty tree asking for a UNIT worktree (`{base}_…`) still creates it.

## Files

- `apps/rt/src/commands/work_unit_open.rs`
