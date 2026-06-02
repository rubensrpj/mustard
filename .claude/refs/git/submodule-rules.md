# Submodule Rules Reference

> Detail for monorepo / submodule handling in `/git`.

## Ephemeral Paths (Claude/RTK runtime)

Claude Code and RTK write continuously to these paths **during skill execution**. They are not code, must never be tracked, and must never block checkout.

Canonical list (`$EPHEMERAL_PATHS`):

```
.claude/.agent-state/
.claude/.metrics/
.claude/.pipeline-states/
.claude/.detect-cache.json
.claude/.knowledge-seen.json
```

### Submodule-safe exclude resolution

`.git` is a **file** in submodules (pointer `gitdir: ../../.git/modules/<name>`), so `.git/info/exclude` paths fail there. ALWAYS resolve the real exclude path first:

```bash
EXCLUDE=$(rtk git rev-parse --git-path info/exclude)
```

This works in parent repo, submodules, and worktrees uniformly.

### Silent ensure-excluded step

At the **start of every write-touching action** (`commit`, `push`, `merge`, `merge main`) and in **each repo operated** (parent + every submodule), run:

```bash
EXCLUDE=$(rtk git rev-parse --git-path info/exclude)
for p in ".claude/.agent-state/" ".claude/.metrics/" ".claude/.detect-cache.json" ".claude/.knowledge-seen.json"; do
  grep -qxF "$p" "$EXCLUDE" 2>/dev/null || echo "$p" >> "$EXCLUDE"
done
```

This is **idempotent** (grep guard before append). No commit, no worktree change — just ensures the paths are ignored by git going forward in that repo.

### Detection of already-tracked ephemerals

After ensure-excluded, check if any ephemeral is already tracked:

```bash
TRACKED_EPH=$(rtk git ls-files -- .claude/.agent-state/ .claude/.metrics/ .claude/.detect-cache.json .claude/.knowledge-seen.json 2>/dev/null)
```

If `$TRACKED_EPH` is non-empty → trigger **Ephemeral Tracked Sub-flow** (see merge-protocol.md) BEFORE the action's main commit.

## Ephemeral Tracked Sub-flow

Triggered automatically by `commit`/`push` when `$TRACKED_EPH` is non-empty, BEFORE the main commit.

Order (per repo that has tracked ephemerals):

1. Ensure-excluded (already ran — confirm):
   ```bash
   EXCLUDE=$(rtk git rev-parse --git-path info/exclude)
   # append missing paths (idempotent guard from Ensure-Excluded step)
   ```
2. Unlink from index without deleting files:
   ```bash
   rtk git rm --cached -r --ignore-unmatch \
     .claude/.agent-state/ .claude/.metrics/ .claude/.pipeline-states/ \
     .claude/.detect-cache.json .claude/.knowledge-seen.json
   ```
3. Dedicated commit:
   ```bash
   rtk git commit -m "chore: ignore ephemeral runtime state

Untracks Claude/RTK runtime paths that should not be versioned.

Co-Authored-By: Claude <noreply@anthropic.com>"
   ```
4. THEN proceed to the user-requested main commit (with resolved `--scope`).

This prevents ephemerals from being dragged into the user-intended commit diff.

## Commit: Submodule Steps

### Analyze all changes (single parallel batch)

Run in **one parallel batch**:
- `rtk git status --short`
- `rtk git submodule status` (skip if no `.gitmodules`)
- `rtk git diff --stat`
- `rtk git log --oneline -5`

### Commit dirty submodules (monorepo only)

Launch **ONE parallel Task agent per dirty submodule** (agents inherit the session model — no model selection). Each agent runs ONE chained Bash command:

```bash
cd <SUBMODULE_ABSOLUTE_PATH> && rtk git add $SCOPE_EXPR && rtk git diff --cached --stat && rtk git commit -m "<message>"
```

For `staged` scope: skip the `rtk git add` step.

## Rules Summary

- Submodules BEFORE parent (in sync, push, commit, and merge)
- NEVER use `rtk git add .` — use `rtk git add -A` / `rtk git add <pattern>` from the correct directory
- Always prefix every git invocation with `rtk` (even inside `&&`/`;` chains and `$(...)` substitutions) — RTK passes through when no filter applies, so it is always safe
- **Single repo**: skip all submodule steps — just operate on the root
- NEVER touch `.git/info/exclude` directly — always resolve via `rtk git rev-parse --git-path info/exclude` (submodule-safe)
