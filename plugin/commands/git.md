---
name: git
description: Use when the user runs /git or asks to commit, push, sync, or open a PR. Reads mustard.json for branch flow. Reversible operations only — never destructive filesystem or history rewrites.
source: manual
---
<!-- mustard:generated -->
# /git - Git Operations

**Iron law: everything goes up (`add -A`) — never a silent partial scope.**

`/git <action> [--scope=all|staged|<path-pattern>]`

## Rationalizations that don't fly

| Excuse | Answer |
|--------|--------|
| "I'll commit just these two files for now" | scope=all is the default; a partial scope applies ONLY when the user explicitly passes `--scope` |
| "pushing straight to the base is faster than a PR" | a work branch reaches its base via `pr` only — never a local push to an integration branch |
| "the submodule change can ride the parent commit" | submodules first, always — each dirty repo gets its own `{base}_{slug}` branch and its own PR |
| "conflict — a quick hard reset and I redo it" | abort on ANY conflict; only reversible operations, never destructive fallbacks |
| "`git add .` from this subdir covers what matters" | never `git add .` — `add -A` (or the user's explicit pattern) from the correct directory |

**Red flags** — catch yourself thinking any of these and stop: *"I'm cherry-picking files into the commit without being asked."* · *"I'm committing while sitting on a bare integration base."* · *"Stash pop without the sentinel index."* · *"I'll skip the submodule PR, it's tiny."*

| Action | Description |
|--------|-------------|
| `sync` | Rebase current branch onto `origin/<its base>` (base from its `{base}_` prefix) |
| `commit` | Create commit (no push). Accepts `--scope=all\|staged\|<pattern>` |
| `push` | Sync first, then commit + push ONLY the current branch |
| `pr [<target>]` | Open/update a PR (idempotent), **one per repo, submodules before parent**. NO return to base — the work stays live on the branch; each `push`/`pr` updates the SAME PR until `pr close`. Work branch → its prefix base; bare base `B` → `<target>` or `flow[B]` (promote `dev→main` / backport `main→dev`) |
| `pr close [<worktree>]` | EXIT RITUAL — run from the WORK BRANCH after its PR merges into its base `dev` (on `dev`/`main` it refuses). Merged? → return to `dev`, pull `dev`, delete the worktree + local & remote branch. NOT merged? → **only warns**, nothing touched. Delegates to `mustard-rt run git-settle` (verify + prune) with `ExitWorktree` wedged in the middle |

→ `${CLAUDE_PLUGIN_ROOT}/refs/git/git-flow.md` (mustard.json, integration-base derivation, work-branch naming, scope policy, backport reminder).

## Behavior

- **ZERO confirmations** — `commit`/`push` default to `--scope=all` (**always `git add -A`, sweep the whole tree**). NEVER infer or memoize a partial scope. `--scope=staged|<pattern>` applies ONLY when the user explicitly passes it.
- **Prefix `git` with `rtk`** — every invocation, including inside `&&`/`;` chains and `$(…)` substitutions.
- Minimize Bash calls — chain with `&&`/`;`, one Bash per repo max.
- Submodules BEFORE parent (always). Single repo: skip submodule steps. Each touched repo (parent + every dirty submodule) carries the work unit on its OWN `{base}_{slug}` branch, cut from THAT repo's base — a submodule never commits straight onto its base, and each repo opens its own PR. → `${CLAUDE_PLUGIN_ROOT}/refs/git/submodule-rules.md` (work branch per repo, PR per repo, ephemeral paths).
- **PRs are the integration path** — a work branch reaches its base via `pr` (`gh pr create --base <prefix-base>`), NEVER a local push to the base. `commit`/`push`/`sync` only ever touch the current work branch.
- **Only reversible operations** — see Forbidden Operations in `${CLAUDE_PLUGIN_ROOT}/refs/git/merge-protocol.md`.

## Procedure

Step 0: resolve `$BASE` from the current branch's `{base}_` prefix (bases derived from `mustard.json#git.flow`). Step 0b: branch protection (refuse write ops while ON a bare integration base — EXCEPT `pr`, which opens a base→base PR). Step 0c: submodule HEAD check (monorepo only).

- **sync** — ensure-excluded → auto-stash → `git fetch && git rebase "origin/$BASE"` → safe stash pop. Abort on conflict. → `merge-protocol.md`.
- **commit** — analyze → ensure-excluded + detect ephemerals → resolve scope → commit submodules (parallel) → commit parent → Final Status Report.
- **push** — sync first (stop on conflict) → commit + push ONLY the current branch (set upstream), in each repo → Final Status Report. In a submodule sitting on its OWN base with changes, cut its `{base}_{slug}` work branch FIRST (checkout `-b` carries the edits over) and push THAT — never an integration branch, in any repo. → `${CLAUDE_PLUGIN_ROOT}/refs/git/submodule-rules.md`.
- **pr** — **work branch** (`{base}_…`): `push` first (this creates each dirty submodule's own `{base}_{slug}` work branch and pushes it — a submodule never commits onto its base; see submodule-rules.md). Then open **one PR per repo, submodules before parent**: for each submodule whose work branch is ahead of its base, `gh pr create --base <sub-base> --head <sub-work-branch> --fill` run INSIDE the submodule; then the parent → `gh pr create --base "$BASE" --head <current> --fill`. **Do NOT return to base** — the work stays live on the branch/worktree; a later `push`/`pr` re-targets the SAME PR until `pr close` prunes it. **Bare base** `B` (the sole op allowed on a base): NO push, NO submodule/base-return steps → `gh pr create --base <target|flow[B]> --head "$B" --fill` — promotion `dev → main`, or backport `main → dev` via `/git pr dev`. Existing PR in any repo → print its URL instead of re-creating.
- **pr close** `[<worktree>]` — the unit's exit ritual, AFTER its PR merges into `dev`. From the WORK BRANCH (never a base — on `dev`/`main` it refuses): (1) `mustard-rt run git-settle` — confirms the branch is merged into its base `dev` (ancestry + `gh` for squash; NOT merged → hard stop, **only a warning**, nothing touched) and advances local `dev`; the answer is `exit-and-rerun` when run inside the unit's own worktree. (2) `ExitWorktree` — the session returns to `dev` on the main checkout. (3) `mustard-rt run git-settle --unit <branch>` — pulls `dev`, removes the worktree, deletes the local branch, deletes the remote branch. Print each JSON verbatim; `alsoMergeable` lists other delivered units awaiting their own `pr close`.

## Ephemeral Paths

Never tracked: `.claude/.agent-state/`, `.claude/.metrics/`, `.claude/.pipeline-states/`, `.claude/.detect-cache.json`, `.claude/.knowledge-seen.json`. At every write action, silently ensure these are in `$(rtk git rev-parse --git-path info/exclude)` (idempotent).

## INVIOLABLE RULES

- Aborts on ANY merge/rebase conflict — **NEVER** fall back to destructive ops.
- NEVER `git add .` — use `git add -A` / `git add <pattern>` from the correct directory.
- NEVER `git stash pop` without the sentinel index. NEVER touch `.git/info/exclude` directly.
- NEVER commit/push/sync directly on a bare integration base (the `git.flow` set). The ONLY op allowed on a base is `/git pr` (base→base promotion/backport) — it opens a PR without pushing. Integration is via `pr` only — reversible, never destructive.
