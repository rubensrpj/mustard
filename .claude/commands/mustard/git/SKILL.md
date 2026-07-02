---
name: mustard-git
description: Use when the user runs /git or asks to commit, push, sync, or open a PR. Reads mustard.json for branch flow. Reversible operations only — never destructive filesystem or history rewrites.
source: manual
---
<!-- mustard:generated -->
# /git - Git Operations

`/git <action> [--scope=all|staged|<path-pattern>]`

| Action | Description |
|--------|-------------|
| `sync` | Rebase current branch onto `origin/<its base>` (base from its `{base}_` prefix) |
| `commit` | Create commit (no push). Accepts `--scope=all\|staged\|<pattern>` |
| `push` | Sync first, then commit + push ONLY the current branch |
| `pr` | Commit + push, then open a PR into the branch's prefix base (idempotent) |

→ `../../../refs/git/git-flow.md` (mustard.json, integration-base derivation, work-branch naming, scope policy, backport reminder).

## Behavior

- **ZERO confirmations** — `commit`/`push` default to `--scope=all` (**always `git add -A`, sweep the whole tree**). NEVER infer or memoize a partial scope. `--scope=staged|<pattern>` applies ONLY when the user explicitly passes it.
- **Prefix `git` with `rtk`** — every invocation, including inside `&&`/`;` chains and `$(…)` substitutions.
- Minimize Bash calls — chain with `&&`/`;`, one Bash per repo max.
- Submodules BEFORE parent (always). Single repo: skip submodule steps. → `../../../refs/git/submodule-rules.md` (monorepo handling + ephemeral paths).
- **PRs are the integration path** — a work branch reaches its base via `pr` (`gh pr create --base <prefix-base>`), NEVER a local push to the base. `commit`/`push`/`sync` only ever touch the current work branch.
- **Only reversible operations** — see Forbidden Operations in `../../../refs/git/merge-protocol.md`.

## Procedure

Step 0: resolve `$BASE` from the current branch's `{base}_` prefix (bases derived from `mustard.json#git.flow`). Step 0b: branch protection (refuse any write op while ON a bare integration base). Step 0c: submodule HEAD check (monorepo only).

- **sync** — ensure-excluded → auto-stash → `git fetch && git rebase "origin/$BASE"` → safe stash pop. Abort on conflict. → `merge-protocol.md`.
- **commit** — analyze → ensure-excluded + detect ephemerals → resolve scope → commit submodules (parallel) → commit parent → Final Status Report.
- **push** — sync first (stop on conflict) → commit + push ONLY the current branch (set upstream) → Final Status Report. Never pushes an integration branch.
- **pr** — `push` first → `gh pr create --base "$BASE" --head <current> --fill` (an existing PR → just push + print its URL) → if any base `X` has `flow[X] == $BASE`, remind the user to also backport the change down to `X` so it does not regress.

## Ephemeral Paths

Never tracked: `.claude/.agent-state/`, `.claude/.metrics/`, `.claude/.pipeline-states/`, `.claude/.detect-cache.json`, `.claude/.knowledge-seen.json`. At every write action, silently ensure these are in `$(rtk git rev-parse --git-path info/exclude)` (idempotent).

## INVIOLABLE RULES

- Aborts on ANY merge/rebase conflict — **NEVER** fall back to destructive ops.
- NEVER `git add .` — use `git add -A` / `git add <pattern>` from the correct directory.
- NEVER `git stash pop` without the sentinel index. NEVER touch `.git/info/exclude` directly.
- NEVER commit/push/sync directly on a bare integration base (the `git.flow` set). Integration into a base branch is via `pr` only — reversible ops, never destructive, abort on conflict.
