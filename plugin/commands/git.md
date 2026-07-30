---
description: Use when the user runs /git or asks to commit, push, sync, or open a PR. Reads mustard.json for branch flow. Reversible operations only — never destructive filesystem or history rewrites.
argument-hint: <action> [--scope=all|staged|<path>]
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
| `pr [<target>]` | Open/update a PR (idempotent) — **one per repo, submodules before parent**. While ANY submodule PR is still open the parent opens `--draft` with a `Blocked by <sub PR url>` body line (GitHub refuses to merge a draft — that is the mechanical half of the order). Work stays live on the branch; each `push`/`pr` updates the SAME PR until `pr close`. Work branch → its prefix base; bare base `B` → `<target>` or `flow[B]` (promote `dev→main` / backport `main→dev`). |
| `pr close [<worktree>]` | Exit ritual — run from the WORK BRANCH after its PR merges (on a bare base it refuses). **One per repo, submodules before parent** (same order as `commit`/`push`/`pr`): the unit lives in every repo it touched. Merged → return to base, pull, delete the worktree + local & remote branch. NOT merged → only warns, nothing touched (giving up instead? `ExitWorktree action=remove` + `rtk git push origin --delete <branch>` if pushed). Delegates to `mustard-rt run git-settle` (verify + prune), with `ExitWorktree` between its two calls when the unit has a worktree; an IN-PLACE unit (no worktree) is exited by settle itself. |

## Iron rules

- **`rtk` prefixes every `git`** — inside `&&`/`;` chains and `$(…)` substitutions too.
- **`git add -A`, never `git add .`** — from the correct directory. `--scope=staged|<pattern>` applies ONLY when the user explicitly passes it; never infer or memoize a partial scope.
- **PRs are the only integration path** — a work branch reaches its base via `pr`, NEVER a local push to the base. `commit`/`push`/`sync` touch only the current work branch. There is no `merge` action.
- **QA precedes integration.** A unit carrying a spec should reach its base only after a passing `qa.result`. The canonical order runs QA while the unit is still **live on its work branch** (`close-pipeline` fires pre-merge) — merging first integrates unverified work. `pr-qa-gate` warns at `gh pr create`/`merge` time, and CLOSE hard-refuses the spec until QA passes. → `/mustard:qa`
- **Submodules before parent, always.** Each dirty repo carries the unit on its own `{base}_{slug}` branch and opens its own PR — a submodule never commits onto its base. Single repo → skip submodule steps. → `${CLAUDE_PLUGIN_ROOT}/refs/git/submodule-rules.md` (work/PR per repo, ephemeral paths, auto-stash).
- **Only reversible operations** — abort on ANY merge/rebase conflict; never a destructive fallback. Banned commands live in `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Destructive-ops Law`.
- **A decision that authorises deletion reads `rev-list`, NEVER `git log`.** `rtk` filters `git log` and DROPS merge commits: measured on this repo, a range whose only commit is a merge (`rtk git log --oneline dd095023 --not dd095023^1 dd095023^2`) comes back as a lone newline — byte-indistinguishable from "nothing here" — so "no commits left, safe to delete" reads TRUE over commits that exist. `rtk git log --oneline -1 <merge sha>` even answers a DIFFERENT commit than plain git. `rtk` passes `rev-list` through byte-identical (measured, 1014 = 1014 bytes), so read ranges with `rtk git rev-list --pretty=oneline --no-commit-header <range>`. The Golden Rule stands: `rtk` still prefixes it.
- **Never operate on a bare integration base** (the `git.flow` set). The one op allowed there is `/git pr` (base→base promotion/backport — opens a PR without pushing).
- Minimize Bash calls — chain with `&&`/`;`, one Bash per repo.

## Procedure

Step 0 resolve `$BASE` from the branch's `{base}_` prefix · Step 0b refuse write ops on a bare base (except `pr`) · Step 0c submodule HEAD check (monorepo). Per-step commands, auto-stash, and the Final Status Report live in the refs above.

- **sync** — ensure-excluded → auto-stash → `rtk git fetch && rtk git rebase "origin/$BASE"` → safe stash pop. Abort on conflict.
- **commit** — analyze → ensure-excluded + detect ephemerals → resolve scope → commit submodules (parallel) → **stage each submodule's moved gitlink ONLY when that SHA is already reachable from the submodule's base** (`rtk git -C <SUB_ABS> merge-base --is-ancestor "$SUB_SHA" "origin/$SUB_BASE"` → then `git add -- <SUB_PATH>`; the parent may owe ONLY this). NOT reachable → do NOT stage it: the parent commits what is its own and the lone ` M <sub>` is the named `[pending-bump]` state, cleared by the **bump** step in `pr close` once the submodule PR merges → commit parent → Final Status Report.
- **push** — sync (stop on conflict) → commit + push the current branch in each repo. A submodule sitting on its base cuts its `{base}_{slug}` work branch FIRST (checkout `-b` carries the edits over), then pushes THAT — never an integration branch.
- **pr** — work branch: `push` first, then one PR per repo (submodules first) into each prefix base; do NOT return to base. **While ANY submodule PR is still open the parent opens as a DRAFT**: `rtk gh pr create --base "$BASE" --head <parent-work-branch> --fill --draft --body "Blocked by <sub PR url>"`. GitHub refuses to merge a draft PR, which is what turns "submodules before parent" from a sentence into a block — the order governed only PR OPENING, and on GitHub the two PRs are siblings anyone can merge in either direction. A draft ALSO does not request review from code owners (CODEOWNERS); those requests fire at `gh pr ready`, which runs in `pr close` after the bump lands — so expect no reviewers until then. Every submodule PR already merged → open the parent normally (`--fill`, no draft). Bare base `B`: no push → `rtk gh pr create --base <target|flow[B]> --head "$B" --fill`. Existing PR in any repo → print its URL.
- **pr close** — one close per repo, **submodules first**: in each submodule carrying the unit, after ITS PR merged, `mustard-rt run git-settle --unit "$SUB_WORK"` from `<SUB_ABS>`. **Then the bump, in the parent, BEFORE the parent PR merges**: the submodule commit now sits on its base, so re-sample the pointer and commit it ALONE (`rtk git submodule status; rtk git add -- <SUB_PATH> && rtk git commit -m "chore(submodule): bump pointer" && rtk git push`) — this is what clears the `[pending-bump]` line the commit step left. Then `rtk gh pr ready` on the parent PR (it opened as a draft while the submodule PR was open; `ready` is also what requests the code owners). Only after the parent PR merges, from the work branch: `mustard-rt run git-settle` (confirm merged, advance the base) → `ExitWorktree` (only when the unit has a worktree — skip for an in-place unit) → `mustard-rt run git-settle --unit <branch>` (pull, remove the worktree, delete local + remote branch; an in-place unit reports `inPlace:true` — settle itself checks out the base and deletes the branch). Print each JSON verbatim; `repos` carries one entry per repo of the unit and `complete:false` means one is still unsettled; `alsoMergeable` lists units awaiting their own `pr close`. → `${CLAUDE_PLUGIN_ROOT}/refs/git/submodule-rules.md`
