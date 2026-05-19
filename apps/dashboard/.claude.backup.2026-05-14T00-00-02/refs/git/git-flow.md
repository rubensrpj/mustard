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
cat mustard.json 2>/dev/null
git rev-parse --abbrev-ref HEAD
```

Match the current branch against `git.flow` patterns. Store as `$PARENT`.
If no match and no `mustard.json`: `$PARENT` = default branch (detect via `git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null || echo main`).

## Step 0b — Branch Protection Check

Before any operation (commit, push, merge, sync) check the current branch:

- If current branch is `main` → **REFUSE** with error: `Cannot operate directly on protected branch 'main'. Create a feature branch first.`
- If current branch is `dev` AND action is `commit`, `push`, or `sync` → **REFUSE** with error: `Cannot operate directly on protected branch 'dev'. Create a feature branch first.`
- If current branch is `dev` AND action is `merge main` → **ALLOW** (this is the only permitted operation on dev).

**Exception**: `/git merge main` is the sole operation allowed on the dev branch — it is the explicit promotion path.

## Step 0c — Submodule HEAD state check (monorepo only)

Before any merge or sync that traverses submodules, emit a readable state line per submodule:

```bash
for sm in $(git config --file .gitmodules --get-regexp path | awk '{print $2}'); do
  ( cd "$sm" && echo "$sm: $(git rev-parse --abbrev-ref HEAD) ($(git rev-parse --short HEAD))" )
done
```

If any submodule is in **detached HEAD** (`HEAD` as branch name), report clearly BEFORE attempting any checkout on that submodule. The user must decide (manual fix or proceed via `/git` stash protocol).

## Commit Scope Policy

The `commit` action accepts `--scope`:

| `--scope` value | Behavior |
|-----------------|----------|
| `all` (default when unambiguous) | `git add -A` in every dirty repo |
| `staged` | Commit only what is already staged (`git commit` with no add) |
| `<path-pattern>` | `git add <pattern>` then commit (glob or directory) |

### Decision flow when `--scope` is NOT passed

1. Run `git status --short` in parent + every dirty submodule.
2. Categorize output inline (see **Final Status Report** categorizer in merge-protocol.md).
3. If output has a **single obvious category** (e.g., only ephemerals → skip; only code changes in one dir → infer that dir): propose the inferred scope in a 5-line preview.
4. Use `AskUserQuestion` **EXACTLY ONCE** per session: _"Scope for this commit? [all / staged / <inferred path>]"_.
5. **Memoize** the answer on `pipeline-state`-style session cache (e.g., env `MUSTARD_GIT_SCOPE_DEFAULT` or a file-local marker) so subsequent `commit`/`push` actions in the same session skip the prompt. Only re-prompt if the user passes `--scope=ask` explicitly.

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
