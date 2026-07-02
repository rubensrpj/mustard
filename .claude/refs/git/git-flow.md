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

Every work unit runs on its own `{base}_{slug}` branch (e.g. `dev_aba-atividade`, `main_close-gate-windows`) — the `{base}_` prefix **records the integration branch the work was cut from**. The branch is **auto-created off `<base>` on the first file edit** of the request: the router chooses the base (asking **"de qual base?"** when the project has more than one integration base), pre-computes the name (`emit-pipeline --kind pipeline.kind --base <base>`), and the harness's `work_branch_gate` checks it out on the first `Write`/`Edit`. A work branch's **base / PR-target is recovered from its prefix** — the leading `{base}_` segment, matched longest-first against the project's integration bases. Read-only requests never branch.

**Integration bases** = every non-`*` key ∪ every value of `git.flow` (`{"*":"dev","dev":"main"}` → `dev`, `main`; `{"*":"develop","develop":"master"}` → `develop`, `master`). Agnostic — no fixed `dev`/`main`; the base set is whatever `git.flow` declares (an empty flow falls back to `main`/`master`).

**Direct edits on a protected branch are BLOCKED.** The BARE integration branches (every base in `git.flow`) are never developed on directly; the `{base}_*` work branches are NOT protected. If a `Write`/`Edit` fires while on a bare integration branch with no work branch to switch to, `work_branch_gate` returns **`Deny`** — describe the work so the router seeds a branch, or branch by hand first. If the auto-checkout itself fails while on a protected branch, the gate also **`Deny`s** (it never falls back to editing the integration branch); on a work branch a failed checkout only warns and lets the edit proceed.

## Actions Table

| Action | Description |
|--------|-------------|
| `sync` | Rebase the current branch onto `origin/<its base>` (base from its `{base}_` prefix). Abort on conflict. |
| `commit` | Create commit (no push). Accepts `--scope=all\|staged\|<path-pattern>` |
| `push` | Sync-first (onto its base), then commit + push ONLY the current branch (set upstream). Never touches an integration branch. |
| `pr` | Commit (scope=all) + push the current branch, then open a PR into its prefix base (`gh pr create --base <prefix-base> --head <current> --fill`). Idempotent — an existing PR just gets the push + its URL. |

**PRs are the integration path.** A work branch reaches its base branch through a PR, never a local push to the base. The base is the branch's own `{base}_` prefix, matched against the project's integration bases (`git.flow`).

### Backport reminder (promotion chains)

After a `pr` into base `B`, check `git.flow` for any base `X` with `flow[X] == B` — those bases **promote into** `B` (e.g. with `{"dev":"main"}`, `dev` promotes into `main`). If the PR targets `B`, remind the user to also bring the change **down** to each such `X` (a follow-up PR `X_… → X`, or a cherry-pick) so `X` does not regress relative to `B`. Fully generic — derived from the flow, no hardcoded dev/main.

## Step 0 — Resolve the base (all actions except commit)

```bash
rtk git rev-parse --abbrev-ref HEAD
```

Read `mustard.json` via the `Read` tool (do not `cat` it). Derive the integration bases from `git.flow` (non-`*` keys ∪ values), then recover the current branch's base from its leading `{base}_` prefix (longest match). Store as `$BASE`.
If the branch has no `{base}_` prefix (or there is no `mustard.json`): `$BASE` = the primary base (`git.flow["*"]`, else the repo default via `rtk git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null || echo main`).

## Step 0b — Branch Protection Check

Before any write op (commit, push, pr, sync) check the current branch against the project's integration bases (`git.flow`):

- If the current branch **is** a bare integration base (an exact member of the derived set, e.g. `dev`, `main`, `master`, `develop`) → **REFUSE** with error: `Cannot operate directly on protected branch '<branch>'. Create a work branch first.`
- Otherwise (a `{base}_*` work branch) → proceed.

Integration into a base branch happens via `pr`, not by operating on the base directly.

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
