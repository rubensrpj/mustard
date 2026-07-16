---
description: Use when the user runs /git or asks to commit, push, sync, or open a PR. Reads mustard.json for branch flow. Reversible operations only — never destructive filesystem or history rewrites.
source: manual
disable-model-invocation: true
---
<!-- mustard:generated -->
# /git — Git Operations

**Iron law: everything goes up (`add -A`) — never a silent partial scope.** Scope policy, base derivation, work-branch naming → `${CLAUDE_PLUGIN_ROOT}/refs/git/git-flow.md`.

`/git <action> [--scope=all|staged|<path-pattern>]`

## Actions

| Action | Description |
|--------|-------------|
| `sync` | Rebase the current branch onto `origin/<its base>` (base from its `{base}_` prefix). Abort on conflict. |
| `commit` | Create a commit, no push. `--scope` defaults to `all`. |
| `push` | Sync first, then commit + push ONLY the current branch (set upstream). |
| `pr [<target>]` | Open/update a PR (idempotent) — **one per repo, submodules before parent**. Work stays live on the branch; each `push`/`pr` updates the SAME PR until `pr close`. Work branch → its prefix base; bare base `B` → `<target>` or `flow[B]` (promote `dev→main` / backport `main→dev`). |
| `pr close [<worktree>]` | Exit ritual — run from the WORK BRANCH after its PR merges (on a bare base it refuses). Merged → return to base, pull, delete the worktree + local & remote branch. NOT merged → only warns, nothing touched. Delegates to `mustard-rt run git-settle` (verify + prune), with `ExitWorktree` between its two calls. |

## Iron rules

- **`rtk` prefixes every `git`** — inside `&&`/`;` chains and `$(…)` substitutions too.
- **`git add -A`, never `git add .`** — from the correct directory. `--scope=staged|<pattern>` applies ONLY when the user explicitly passes it; never infer or memoize a partial scope.
- **PRs are the only integration path** — a work branch reaches its base via `pr`, NEVER a local push to the base. `commit`/`push`/`sync` touch only the current work branch. There is no `merge` action.
- **Submodules before parent, always.** Each dirty repo carries the unit on its own `{base}_{slug}` branch and opens its own PR — a submodule never commits onto its base. Single repo → skip submodule steps. → `${CLAUDE_PLUGIN_ROOT}/refs/git/submodule-rules.md` (work/PR per repo, ephemeral paths, auto-stash).
- **Only reversible operations** — abort on ANY merge/rebase conflict; never a destructive fallback. Banned commands live in `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Destructive-ops Law`.
- **Never operate on a bare integration base** (the `git.flow` set). The one op allowed there is `/git pr` (base→base promotion/backport — opens a PR without pushing).
- Minimize Bash calls — chain with `&&`/`;`, one Bash per repo.

## Procedure

Step 0 resolve `$BASE` from the branch's `{base}_` prefix · Step 0b refuse write ops on a bare base (except `pr`) · Step 0c submodule HEAD check (monorepo). Per-step commands, auto-stash, and the Final Status Report live in the refs above.

- **sync** — ensure-excluded → auto-stash → `rtk git fetch && rtk git rebase "origin/$BASE"` → safe stash pop. Abort on conflict.
- **commit** — analyze → ensure-excluded + detect ephemerals → resolve scope → commit submodules (parallel) → commit parent → Final Status Report.
- **push** — sync (stop on conflict) → commit + push the current branch in each repo. A submodule sitting on its base cuts its `{base}_{slug}` work branch FIRST (checkout `-b` carries the edits over), then pushes THAT — never an integration branch.
- **pr** — work branch: `push` first, then one PR per repo (submodules first) into each prefix base; do NOT return to base. Bare base `B`: no push → `rtk gh pr create --base <target|flow[B]> --head "$B" --fill`. Existing PR in any repo → print its URL.
- **pr close** — from the work branch after merge: `mustard-rt run git-settle` (confirm merged, advance the base) → `ExitWorktree` → `mustard-rt run git-settle --unit <branch>` (pull, remove the worktree, delete local + remote branch). Print each JSON verbatim; `alsoMergeable` lists units awaiting their own `pr close`.
