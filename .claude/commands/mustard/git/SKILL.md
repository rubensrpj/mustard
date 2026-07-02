---
name: mustard-git
description: Use when the user runs /git or asks to commit, push, sync, or merge. Reads mustard.json for branch flow. Reversible operations only — never destructive filesystem or history rewrites.
source: manual
---
<!-- mustard:generated -->
# /git - Git Operations

`/git <action> [--scope=all|staged|<path-pattern>]`

| Action | Description |
|--------|-------------|
| `sync` | Pull parent branch into current branch |
| `commit` | Create commit (no push). Accepts `--scope=all\|staged\|<pattern>` |
| `push` | Sync first, then commit + push |
| `merge` | Sync + fast-forward merge to parent (single hop, always to dev) |
| `merge main` | Cascade: branch → dev → main → back to branch |

→ `../../../refs/git/git-flow.md` (mustard.json, flow resolution, scope policy, performance budget).

## Behavior

- **ZERO confirmations** — `commit`/`push` default to `--scope=all` (**always `git add -A`, sweep the whole tree**). NEVER infer or memoize a partial scope. `--scope=staged|<pattern>` applies ONLY when the user explicitly passes it.
- **Prefix `git` with `rtk`** — every invocation, including inside `&&`/`;` chains and `$(…)` substitutions.
- Minimize Bash calls — chain with `&&`/`;`, one Bash per repo max.
- Submodules BEFORE parent (always). Single repo: skip submodule steps. → `../../../refs/git/submodule-rules.md` (monorepo handling + ephemeral paths).
- **Local fast-forward merge** — no PRs, no merge commits, 100% linear history.
- **Only reversible operations** — see Forbidden Operations in `../../../refs/git/merge-protocol.md`.

## Procedure

Step 0: resolve `$PARENT` from `mustard.json`. Step 0b: branch protection (refuse on `main`; refuse `commit`/`push`/`sync` on `dev`; allow `merge main` on `dev`). Step 0c: submodule HEAD check (monorepo only).

- **sync** — ensure-excluded → auto-stash → `git fetch && git rebase "origin/$PARENT"` → safe stash pop. → `merge-protocol.md`.
- **commit** — analyze → ensure-excluded + detect ephemerals → resolve scope → commit submodules (parallel) → commit parent → Final Status Report.
- **push** — sync first (stop on conflict) → commit + push submodules (parallel) → push parent → Final Status Report.
- **merge** — sync → ensure pushed → auto-stash checkout loop → `git merge --ff-only` → push → return to source.
- **merge main** — if not on dev: run `merge` first. Then dev → main via same ff-only → return to `$ORIGIN`. Print summary table + Final Status Report.

## Ephemeral Paths

Never tracked: `.claude/.agent-state/`, `.claude/.metrics/`, `.claude/.pipeline-states/`, `.claude/.detect-cache.json`, `.claude/.knowledge-seen.json`. At every write action, silently ensure these are in `$(rtk git rev-parse --git-path info/exclude)` (idempotent).

## INVIOLABLE RULES

- Aborts on ANY merge conflict. Aborts if `--ff-only` fails — **NEVER** fall back to destructive ops.
- NEVER `git add .` — use `git add -A` / `git add <pattern>` from the correct directory.
- NEVER `git stash pop` without the sentinel index. NEVER touch `.git/info/exclude` directly.
- After merge, return to the original branch. NEVER commit/push/sync directly on `main` or `dev`.
