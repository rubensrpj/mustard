# Submodule Rules Reference

> Detail for monorepo / submodule handling in `/git`.

## Work branch per repo — a submodule never commits onto its base

The work unit `{slug}` materialises as a branch `{base}_{slug}` in EVERY repo it touches: the parent (cut by `work_branch_gate` on the first edit) AND each dirty submodule (cut by `/git` at commit time, below). Each repo's `{base}_` prefix records THAT repo's own base — the parent's base comes from `mustard.json#git.flow`; a submodule's base is its OWN default branch, since a submodule is an independent repo that need not share the parent's `dev`/`main` flow.

**Resolve a submodule's base + work branch** (per submodule; `<SUB_ABS>` absolute, via `git -C`, never `cd`):

```bash
PARENT_BRANCH=$(rtk git rev-parse --abbrev-ref HEAD)   # run in the parent root; it is {base}_{slug}
SLUG=${PARENT_BRANCH#*_}          # slug = everything after the first `_` (the parent's base prefix)
SUB_BASE=$(rtk git -C "<SUB_ABS>" symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null | sed 's#^origin/##')
[ -z "$SUB_BASE" ] && SUB_BASE=$(rtk git -C "<SUB_ABS>" rev-parse --abbrev-ref HEAD)   # fallback: current branch
SUB_WORK="${SUB_BASE}_${SLUG}"    # the submodule's own work branch — same slug, its own base prefix
```

**Cut it at commit time — only when the submodule sits on its base with changes.** If `rtk git -C <SUB_ABS> rev-parse --abbrev-ref HEAD` equals `$SUB_BASE`, create the work branch before staging: `rtk git -C <SUB_ABS> checkout -b "$SUB_WORK"` carries the working-tree edits over. If the submodule is ALREADY on `$SUB_WORK` (a later edit of the same unit), skip the checkout. **Never add/commit/push while a submodule is on its bare base** — this is the parent's branch-protection rule extended to every repo.

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

Launch **ONE parallel Task agent per dirty submodule** (agents inherit the session model — no model selection). Each agent FIRST puts the submodule on its `{base}_{slug}` work branch (resolve `$SUB_BASE`/`$SUB_WORK` per **Work branch per repo** above), THEN stages + commits — ONE chained Bash command:

```bash
rtk git -C "<SUB_ABS>" checkout "$SUB_WORK" 2>/dev/null || rtk git -C "<SUB_ABS>" checkout -b "$SUB_WORK"; \
rtk git -C "<SUB_ABS>" add $SCOPE_EXPR && rtk git -C "<SUB_ABS>" diff --cached --stat && rtk git -C "<SUB_ABS>" commit -m "<message>"
```

The leading `checkout` switches to an existing `$SUB_WORK` (a later edit of the same unit) or, failing that, `checkout -b` cuts it off the current base — carrying the working-tree edits over. Either way the commit lands on the work branch, never on the base. For `staged` scope: skip the `rtk git add` step.

`<SUB_ABS>` MUST be **absolute** and is passed via `git -C` (never `cd`), per the "Absolute paths, no cd" rule. `.gitmodules` / `rtk git submodule status` report paths **relative** to the superproject root, so resolve the absolute form first — `<superproject-root>/<relative-submodule-path>`, where `<superproject-root>` = `rtk git rev-parse --show-toplevel`. A bare relative path or `cd <relative>` fails whenever the shell cwd is not the superproject root.

## PR per repo — submodules before parent

`/git pr` (on a work branch) opens ONE PR per repo, **submodules FIRST**. Order matters: the parent commit bumps each submodule's gitlink to a submodule work-branch commit; merging the submodule PR first lands that commit on the submodule's base, so the parent's pointer is never left dangling when the parent PR merges.

After `push` (which committed + pushed each submodule's `$SUB_WORK` and the parent's work branch):

1. **Each submodule ahead of its base** — a PR from inside the submodule. Skip a submodule whose work branch is not ahead (`rtk git -C <SUB_ABS> rev-parse "$SUB_BASE..$SUB_WORK"` empty → no commits → no PR):
   ```bash
   ( cd "<SUB_ABS>" && rtk gh pr create --base "$SUB_BASE" --head "$SUB_WORK" --fill )
   ```
   The subshell `cd` is fine here — this is `gh`, which reads the repo from its cwd, and a `( … )` subshell isolates the change so the outer cwd never moves. (The "no `cd`" rule targets `git`; there you still use `git -C`.) `gh` infers the submodule's repo from its own `origin`; or pass `-R <owner/repo>` explicitly. An existing PR → print its URL instead of re-creating.
2. **Then the parent** — `rtk gh pr create --base "$BASE" --head <parent-work-branch> --fill`.
3. **Return every repo to its base** — `rtk git -C <SUB_ABS> checkout "$SUB_BASE"` for each submodule, then `rtk git checkout "$BASE"` in the parent. The unit is delivered (PRs open, awaiting review); leaving each repo on its base stops the next unit from piling onto this one and keeps the tree clean. The push already published every work branch, so nothing is lost by checking out the base.

Only for the **work-branch** `pr`. A **base→base** `pr` (promotion/backport) opens its single PR and does NOT push, cut submodule branches, or return anywhere.

## Rules Summary

- Submodules BEFORE parent (in sync, push, commit, merge, **and pr**)
- Every repo (parent + dirty submodule) carries the unit on its own `{base}_{slug}` work branch, cut from THAT repo's base — never commit/push onto a bare base, in any repo
- NEVER use `rtk git add .` — use `rtk git add -A` / `rtk git add <pattern>` from the correct directory
- Always prefix every git invocation with `rtk` (even inside `&&`/`;` chains and `$(...)` substitutions) — RTK passes through when no filter applies, so it is always safe
- **Single repo**: skip all submodule steps — just operate on the root
- NEVER touch `.git/info/exclude` directly — always resolve via `rtk git rev-parse --git-path info/exclude` (submodule-safe)
