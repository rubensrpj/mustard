# Git Flow Reference

> Detail for `/git` action routing, configuration, and branch flow.

## Configuration (mustard.json)

Reads `mustard.json` from the **project root**. If not found, falls back to defaults.

```json
{
  "git": {
    "flow": {
      "*": "dev",
      "dev": "main"
    },
    "submodules": true
  }
}
```

### Flow Resolution

Match current branch against `flow` keys. Exact match first, then glob. `*` is the default fallback for any branch not explicitly listed.

| Current branch | Pattern matched | Parent resolved |
|---------------|----------------|-----------------|
| `feature/login` | `*` | `dev` |
| `fix/bug-123` | `*` | `dev` |
| `dev` | `dev` | `main` (only via `/git merge main`) |
| `main` | no match | **error**: terminal branch, no operations allowed |

**Rule**: Exact keys (`dev`, `main`) are matched first. `*` catches everything else. `main` and `dev` are never matched by `*`.

## Work branches

Every work unit runs on its own `{kind}/{slug}` branch (e.g. `feature/aba-atividade`, `bugfix/close-gate-windows`). The branch is **auto-created off `dev` on the first file edit** of the request: the router pre-computes the name (`emit-pipeline --kind pipeline.kind`) and the harness's `work_branch_gate` checks it out on the first `Write`/`Edit`. Read-only requests never branch. `/git merge` then fuses the work branch back to `dev` (its `*` parent).

**Direct edits on a protected branch are BLOCKED.** `dev` and `main`/`master` (the `git.flow` parents) are never developed on directly. If a `Write`/`Edit` fires while on a protected branch with no work branch to switch to, `work_branch_gate` returns **`Deny`** — describe the work so the router seeds a branch, or branch by hand first. If the auto-checkout itself fails while on a protected branch, the gate also **`Deny`s** (it never falls back to editing `dev`); on a normal work branch a failed checkout only warns and lets the edit proceed.

## Actions Table

| Action | Description |
|--------|-------------|
| `sync` | Pull parent branch into current branch |
| `commit` | Create commit (no push). Accepts `--scope=all\|staged\|<path-pattern>` |
| `push` | Commit + push to remote |
| `merge` | Sync + fast-forward merge to parent (single hop, always to dev) |
| `merge main` | Fast-forward merge dev → main (explicit promotion, must be on dev) |

## Step 0 — Resolve Parent (all actions except commit)

```bash
rtk git rev-parse --abbrev-ref HEAD
```

Read `mustard.json` via the `Read` tool (do not `cat` it). Match the current branch against `git.flow` patterns. Store as `$PARENT`.
If no match and no `mustard.json`: `$PARENT` = default branch (detect via `rtk git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null || echo main`).

## Step 0b — Branch Protection Check

Before any operation (commit, push, merge, sync) check the current branch:

- If current branch is `main` → **REFUSE** with error: `Cannot operate directly on protected branch 'main'. Create a feature branch first.`
- If current branch is `dev` AND action is `commit`, `push`, or `sync` → **REFUSE** with error: `Cannot operate directly on protected branch 'dev'. Create a feature branch first.`
- If current branch is `dev` AND action is `merge main` → **ALLOW** (this is the only permitted operation on dev).

**Exception**: `/git merge main` is the sole operation allowed on the dev branch — it is the explicit promotion path.

## Step 0c — Submodule HEAD state check (monorepo only)

Before any merge or sync that traverses submodules, emit a readable state line per submodule:

```bash
for sm in $(rtk git config --file .gitmodules --get-regexp path | awk '{print $2}'); do
  ( cd "$sm" && echo "$sm: $(rtk git rev-parse --abbrev-ref HEAD) ($(rtk git rev-parse --short HEAD))" )
done
```

If any submodule is in **detached HEAD** (`HEAD` as branch name), report clearly BEFORE attempting any checkout on that submodule. The user must decide (manual fix or proceed via `/git` stash protocol).

## Commit Scope Policy

**Default: `all` — ALWAYS `rtk git add -A` in every dirty repo.** `commit`/`push` sweep the *entire* working tree unless the user *explicitly* passes a narrower `--scope`. NEVER infer a partial scope from the diff, NEVER memoize one — a silent partial commit that leaves files behind is the exact failure this policy exists to prevent.

| `--scope` value | Behavior |
|-----------------|----------|
| _(omitted)_ or `all` | `rtk git add -A` in every dirty repo — **the default** |
| `staged` | Commit only what is already staged (`rtk git commit` with no add) — **only when explicitly passed** |
| `<path-pattern>` | `rtk git add <pattern>` then commit (glob or directory) — **only when explicitly passed** |

### When `--scope` is NOT passed

Use `all`. No prompt, no inference, no memoization. Run `rtk git add -A` in the parent and every dirty submodule, then commit. The only paths skipped are genuine ephemerals (see **Ephemeral Paths** in the skill) — everything else goes up, every time.

## Performance Budget

- **Max Task agents**: 1 per dirty submodule
- **Max Bash calls per agent**: 1 (all commands chained)
- **Max Bash calls for merge**: 1 per submodule + 1 for parent
- **Max checkout retries**: 3 per repo, then abort with descriptive error

## Message Format

```
<type>: <short description>

<detailed description if needed>

Co-Authored-By: Claude <noreply@anthropic.com>
```

Types: feat, fix, refactor, docs, chore, test
