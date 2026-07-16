# Git Flow Reference

> Detail for `/git`: branch flow, base derivation, the worktree contract, and commit scope. Command: `${CLAUDE_PLUGIN_ROOT}/commands/git.md`. Submodule / ephemeral / auto-stash detail: `${CLAUDE_PLUGIN_ROOT}/refs/git/submodule-rules.md`.

## Contents
- Configuration & flow resolution
- Work branches & the gate
- Worktree contract
- PRs as the integration path (+ base→base promotion / backport)
- Step 0 / 0b — resolve base, branch protection
- Commit scope policy (the `add -A` law)
- Commit message format

## Configuration (mustard.json)

Read `mustard.json` from the **project root** via the `Read` tool (not `cat`); missing → defaults.

```json
{ "git": { "flow": { "*": "dev", "dev": "main" }, "submodules": true } }
```

**Integration bases** = every non-`*` key ∪ every value of `git.flow` (`{"*":"dev","dev":"main"}` → `dev`, `main`). Agnostic — no hardcoded `dev`/`main`; an empty flow falls back to `main`/`master`.

**Flow resolution** — match the current branch against `flow` keys, exact before glob; `*` is the fallback for anything unlisted. `dev` → `main` (promotion via `/git pr`); `main` is terminal (no ops).

## Work branches & the gate

Every work unit runs on its own `{base}_{slug}` branch (e.g. `dev_aba-atividade`). The `{base}_` prefix **records the integration branch the work was cut from**; `/git` recovers a branch's base / PR-target from it (longest match against the integration bases). The branch is **auto-created off `<base>` on the first file edit**: the router picks the base (asking "de qual base?" when the project has more than one), pre-computes the name (`emit-pipeline --base <base>`), and `work_branch_gate` cuts + checks it out on the first `Write`/`Edit`. Read-only requests never branch.

**The gate** (`work_branch_gate`, PreToolUse Write/Edit) judges the LOCAL tree hosting the edit, so a nested worktree on a work branch is never blocked by the main checkout's branch. With a pending-unit marker it cuts `{base}_{slug}` off the freshly fetched base and allows (fail-open — a git failure warns, never blocks); with no marker, a direct edit on a bare integration base is **denied**, while any `{base}_*` work branch edits freely.

**Monorepo:** the gate cuts the branch in the PARENT only. Each dirty submodule gets its OWN `{base}_{slug}` branch (its own base prefix), cut by `/git` at commit time — see `submodule-rules.md`.

## Worktree contract — one unit, one worktree

Every unit runs in its OWN worktree at `.claude/worktrees/{base}_{slug}`, so concurrent sessions never share a tree. The `{base}_` prefix is load-bearing — `/git` reads it to target the PR — and the branch is cut FROM `{base}`, so the right base in yields the right PR target out.

- **Desktop / background CLI** — isolated automatically. A Desktop branch has no `{base}_` prefix, so `/git` falls back to the primary base (`git.flow["*"]`); pass an explicit `<target>` for any other base.
- **Foreground CLI** — isolate before the first edit: `mustard-rt run work-unit-open --spec {slug} --base {base}` creates the worktree idempotently (fresh `origin/{base}`, offline degrades to the local ref, existing branch is attached, never re-cut) and copies the main checkout's `.claude/settings.local.json` into it, then switch the session into the returned `path` with `EnterWorktree path={path}`. NEVER `EnterWorktree name=…` for a work unit — it cuts from the repo DEFAULT branch, wrong for any other `{base}`.

## PRs are the integration path

A work branch reaches its base ONLY through a PR — never a local push to the base, and there is no `merge` action. Both `push` and `pr` **sync-first** (rebase onto `origin/<its base>`), so the branch never drifts from the latest base.

**Base→base PRs (promotion & backport).** `/git pr` run while ON a bare base `B` is the sole write-op allowed on a base — it opens a PR, never pushes to `B`:

- **Promotion** (up the flow): PR `B → flow[B]` (e.g. `dev → main`).
- **Backport** (against the flow): `/git pr <target>` → PR `B → <target>` (e.g. `main → dev` after a hotfix).

Directions come from `git.flow` — no hardcoded pair. A terminal base (no `flow[B]`) needs an explicit `<target>`.

## Step 0 — resolve the base

```bash
rtk git rev-parse --abbrev-ref HEAD
```

Derive the integration bases from `git.flow`, then recover the branch's base from its leading `{base}_` prefix (longest match) → `$BASE`. No prefix (or no `mustard.json`) → `$BASE` = the primary base (`git.flow["*"]`, else `rtk git symbolic-ref refs/remotes/origin/HEAD` || `main`).

## Step 0b — branch protection

Before any write op (commit, push, sync): if the current branch **is** a bare integration base → **REFUSE** (`Cannot operate directly on protected branch '<branch>'. Create a work branch first.`). A `{base}_*` work branch proceeds. **Exception:** `/git pr` on a base opens a base→base PR (above) and is allowed.

## Commit scope policy — the `add -A` law

**Default `all`: ALWAYS `rtk git add -A` in every dirty repo.** `commit`/`push` sweep the *entire* working tree unless the user *explicitly* passes a narrower `--scope`. NEVER infer a partial scope from the diff, NEVER memoize one — a silent partial commit that leaves files behind is the exact failure this law prevents.

| `--scope` | Behavior |
|-----------|----------|
| _(omitted)_ / `all` | `rtk git add -A` in every dirty repo — **the default** |
| `staged` | Commit only what is staged (`rtk git commit`, no add) — explicit only |
| `<path-pattern>` | `rtk git add <pattern>` then commit — explicit only |

The only paths ever skipped are genuine ephemerals (single home: `submodule-rules.md`).

## Commit message format

```
<type>: <short description>

<body if needed>

Co-Authored-By: Claude <noreply@anthropic.com>
```

Types: feat, fix, refactor, docs, chore, test.
