# /git - Git Operations

> Commit, push, sync, and merge. Reads `mustard.json` for branch flow. Handles monorepo (submodules) and single repo automatically. Uses **only reversible operations** — never destructive filesystem or history rewrites.

## Trigger

`/git <action> [--scope=all|staged|<path-pattern>]`

## Actions

| Action | Description |
|--------|-------------|
| `sync` | Pull parent branch into current branch |
| `commit` | Create commit (no push). Accepts `--scope=all\|staged\|<path-pattern>` |
| `push` | Sync first, then commit + push to remote |
| `merge` | Sync + fast-forward merge to parent (single hop, always to dev) |
| `merge main` | Cascade: branch → dev → main → back to branch |

→ See `../../../refs/git/git-flow.md` for `mustard.json` config, flow resolution table, commit scope policy, and performance budget.

## Behavior

- **ZERO confirmations by default** — analyze, execute, done. Only exception: `commit` without `--scope` asks once per session and memoizes the choice.
- **Always prefix `git` with `rtk`** — every invocation (`rtk git status`, `rtk git fetch`, …) including inside `&&`/`;` chains and `$(...)` substitutions. RTK passes through when no filter applies, so prefixing is always safe.
- **Minimize Bash calls** — chain everything with `&&` / `;`. One Bash call per repo max whenever possible.
- **No investigation** — if a submodule is dirty, commit it (scoped per Commit Scope Policy).
- Submodules BEFORE parent (always).
- **Single repo**: skip all submodule steps — just operate on the root.
- **Local fast-forward merge** — no PRs, no merge commits, 100% linear history.
- **Only reversible operations** — see Forbidden Operations in `../../../refs/git/merge-protocol.md`.

## Procedure (per action)

**Step 0** — Resolve parent branch from `mustard.json` → `$PARENT`. → See `../../../refs/git/git-flow.md`.

**Step 0b** — Branch protection check: refuse on `main`; refuse `commit`/`push`/`sync` on `dev`; allow `merge main` on `dev`. → See `../../../refs/git/git-flow.md`.

**Step 0c** — Submodule HEAD state check (monorepo only). → See `../../../refs/git/git-flow.md`.

**sync** — Ensure-excluded → auto-stash → `git fetch origin "$PARENT" && git rebase "origin/$PARENT"` → safe stash pop. → See `../../../refs/git/merge-protocol.md`.

**commit** — Analyze changes → ensure-excluded + detect tracked ephemerals → resolve scope → commit submodules (parallel Task agents) → commit parent → Final Status Report. → See `../../../refs/git/submodule-rules.md` and `../../../refs/git/git-flow.md`.

**push** — sync first (stop on conflict) → commit + push submodules (parallel) → push parent → Final Status Report. → See `../../../refs/git/merge-protocol.md`.

**merge** — sync → ensure pushed → auto-stash checkout loop → `git merge --ff-only` → push → return to source → Final Status Report. → See `../../../refs/git/merge-protocol.md`.

**merge main** — If not on dev: run `merge` first. Then promote dev → main via same ff-only protocol → return to `$ORIGIN`. Print summary table + Final Status Report. → See `../../../refs/git/merge-protocol.md`.

## Ephemeral Paths

Claude/RTK runtime paths that must never be tracked:

```
.claude/.agent-state/   .claude/.metrics/   .claude/.pipeline-states/
.claude/.detect-cache.json   .claude/.knowledge-seen.json
```

At every write-touching action, silently ensure these are in `$(rtk git rev-parse --git-path info/exclude)` (idempotent). → See `../../../refs/git/submodule-rules.md` for full protocol.

## Cautions

- Aborts if ANY repo has merge conflicts (sync or push)
- Aborts if `--ff-only` fails — **NEVER** fall back to destructive ops
- NEVER use `git add .` — use `git add -A` / `git add <pattern>` from the correct directory
- NEVER `git stash pop` without locating sentinel index — preserves user's pre-existing stashes
- NEVER touch `.git/info/exclude` directly — always resolve via `git rev-parse --git-path info/exclude`
- After merge, return to the original branch (`$SOURCE` / `$ORIGIN`)
- NEVER commit, push, or sync directly on `main` or `dev`
