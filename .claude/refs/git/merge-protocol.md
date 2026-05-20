# Merge Protocol Reference

> Detail for auto-stash, ff-only merge, forbidden operations, and final status report.

## Auto-stash Protocol

EVERY checkout operation in this skill (sync, merge feature→dev, merge main step 1, merge main step 2, `checkout $ORIGIN` at end) MUST be wrapped by the auto-stash protocol.

### Sentinel format

Each skill invocation generates ONE sentinel per action attempt:

```
mustard-git-autostash-<action>-<unix_timestamp_ns>
```

Examples:
- `mustard-git-autostash-sync-1744934400123456789`
- `mustard-git-autostash-merge-1744934401987654321`
- `mustard-git-autostash-merge-main-step2-1744934402000000000`

Generate once per action entry (`SENTINEL="mustard-git-autostash-<action>-$(date +%s%N)"`) and reuse for push/pop within that action. Different actions get different sentinels so parallel-ish submodule ops do not collide.

### Protected stash push

```bash
rtk git stash push -u -m "$SENTINEL"
# untracked included (-u) because runtime-regenerated files may be untracked
```

### Retry loop on checkout race

Race scenario: between `rtk git stash push` and `rtk git checkout <target>`, Claude/RTK rewrite `.claude/.agent-state/*` etc. → checkout aborts with `"would be overwritten by checkout"` or `"local changes would be overwritten"`.

Protocol (max 3 attempts, then abort with descriptive error):

```bash
ATTEMPT=1
MAX=3
while [ $ATTEMPT -le $MAX ]; do
  rtk git stash push -u -m "$SENTINEL" 2>/dev/null
  CO_OUT=$(rtk git checkout "$TARGET" 2>&1)
  CO_RC=$?
  if [ $CO_RC -eq 0 ]; then
    break
  fi
  if echo "$CO_OUT" | grep -qE "would be overwritten|local changes"; then
    ATTEMPT=$((ATTEMPT+1))
    continue
  fi
  # different failure class — stop and surface
  echo "checkout failed: $CO_OUT" >&2
  exit 1
done
[ $ATTEMPT -gt $MAX ] && { echo "checkout race unresolved after $MAX attempts. Offending paths:"; echo "$CO_OUT"; exit 1; }
```

### Safe stash pop (preserving pre-existing user stashes)

**NEVER** run `rtk git stash pop` without first locating the exact sentinel. Pre-existing user stashes at `stash@{0}` must not be disturbed.

```bash
IDX=$(rtk git stash list | grep -F "$SENTINEL" | head -n1 | sed -E 's/^stash@\{([0-9]+)\}.*$/\1/')
if [ -n "$IDX" ]; then
  rtk git stash pop "stash@{$IDX}"
fi
```

If `$IDX` is empty (sentinel not found — nothing was stashed this run), do nothing.

## Forbidden Operations

These operations are **irreversible** at filesystem or history level and are **BANNED** from this skill under any condition.

| Forbidden | Reversible alternative |
|-----------|------------------------|
| `rm -f <path>`, `rm -rf <path>` | `rtk git rm --cached <path>` (preserves file on disk) |
| `git clean -fd`, `git clean -fdx` | Append to `$(rtk git rev-parse --git-path info/exclude)` instead |
| `git checkout -f`, `git checkout --force` | Auto-stash Protocol with retry (above) |
| `git reset --hard` | `rtk git stash push` to snapshot state, then `rtk git checkout <ref>` |
| Forced unlink of lock files | Investigate process holding lock; never delete blindly |

**Rationale**: all skill state transitions must be recoverable via `rtk git reflog` / `rtk git stash list`. Filesystem-destructive shortcuts silently lose user work.

## merge — Fast-Forward Protocol

Promote current branch into its parent via **local fast-forward merge** — no PRs, no merge commits, 100% linear history. Single hop only — always merges into `dev` (via `*` wildcard). Never cascades.

### Step 1 — Sync (mandatory)

Execute `sync` action to rebase from `dev`. If conflicts → STOP. Do not proceed to merge.

### Step 2 — Ensure pushed

Check if local is ahead of remote. If yes, execute `push` first.

### Step 3 — Merge into parent (auto-stashed, retry-capable, compact output)

`$SOURCE` = current branch, `$TARGET` = `$PARENT` (resolved in Step 0, always `dev` for feature/fix branches).

Per-repo procedure (submodules parallel first, then parent):

1. Generate `SENTINEL="mustard-git-autostash-merge-$(date +%s%N)"`.
2. **Ensure-excluded** (ephemerals).
3. Auto-stash Protocol push (`-u`).
4. Checkout chain with retry (Auto-stash Protocol) to `$SOURCE`, pull, then to `$TARGET`, pull:
   ```bash
   rtk git fetch origin && \
     rtk git checkout "$SOURCE" && rtk git pull origin "$SOURCE" && \
     rtk git checkout "$TARGET" && rtk git pull origin "$TARGET"
   ```
5. Fast-forward merge with compact output:
   ```bash
   rtk git merge --ff-only -q "$SOURCE" && rtk git --no-pager diff --stat HEAD@{1} HEAD | tail -3
   ```
6. Push:
   ```bash
   rtk git push origin "$TARGET"
   ```
7. Return to `$SOURCE`:
   ```bash
   rtk git checkout "$SOURCE"
   ```
8. **Safe stash pop** by sentinel index.

Skip submodules with no commits ahead (nothing to merge).

### Fast-forward failure

If `--ff-only` fails (branches diverged), STOP and report to user. **NEVER** fall back to `rtk git reset --hard` or `rtk git checkout -f`.

### Example: `/git merge` from `feature/login`

```
feature/login → dev
  ├── SubprojectA:  ff-merged + pushed
  ├── SubprojectB:  ff-merged + pushed
  └── Parent:       ff-merged + pushed
```

## merge main — Full Promotion Protocol

Full promotion to `main` — cascades through the entire flow chain, then returns to the original branch.

**Branch check**: If on `main` → refuse (terminal branch).

### Behavior

`$ORIGIN` = current branch (saved for return at end).

1. If NOT on `dev`: first execute `merge` action (current branch → dev). If it fails → STOP.
2. Then promote `dev → main`.
3. Return to `$ORIGIN`.

This means from ANY feature/fix branch: `/git merge main` does `feature → dev → main → back to feature` in one shot.

### Step 2 — Merge dev into main (auto-stashed, retry-capable, compact output)

`$SOURCE` = `dev`, `$TARGET` = `main`.

Per-repo procedure (submodules parallel first, then parent):

1. Generate `SENTINEL="mustard-git-autostash-merge-main-$(date +%s%N)"`.
2. **Ensure-excluded** (ephemerals).
3. Auto-stash Protocol push (`-u`).
4. Checkout chain with retry via Auto-stash Protocol:
   ```bash
   rtk git fetch origin && \
     rtk git checkout dev && rtk git pull origin dev && \
     rtk git checkout main && rtk git pull origin main
   ```
5. Compact ff-merge + push:
   ```bash
   rtk git merge --ff-only -q dev && rtk git --no-pager diff --stat HEAD@{1} HEAD | tail -3 && \
     rtk git push origin main
   ```
6. Return to `$ORIGIN` (parent uses `$ORIGIN`; submodules return to `dev`):
   ```bash
   rtk git checkout "$ORIGIN"   # parent
   rtk git checkout dev         # submodule
   ```
7. **Safe stash pop** by sentinel index.

### Example: `/git merge main` from `feature/login`

```
feature/login → dev → main → back to feature/login
  Step 1: feature/login → dev  (ff-merged + pushed)
  Step 2: dev → main           (ff-merged + pushed)
  Return: checkout feature/login
```

### Output (merge main summary)

Print a summary table at the end AND the **Final Status Report** (below):

```
| Step                    | Status             |
|-------------------------|--------------------|
| feature/login → dev     | ff-merged + pushed |
| dev → main              | ff-merged + pushed |
| Return to feature/login | done               |
```

## sync — Per-repo Procedure

Pull the parent branch changes into the current branch.

1. **Ensure-excluded** (ephemerals) — silent, idempotent.
2. **Auto-stash Protocol**: `SENTINEL="mustard-git-autostash-sync-$(date +%s%N)"`.
3. Fetch + rebase in one chain:
   ```bash
   rtk git fetch origin "$PARENT" && rtk git rebase "origin/$PARENT"
   ```
4. **Safe stash pop** (by sentinel index).
5. If rebase has conflicts → abort rebase, report to user, STOP.

Submodules run in parallel (one Bash call each). Parent repo runs after.

## push — Sequential Phase Procedure

Sequential: **sync first**, then commit + push.

### Phase 1 — Sync

Execute `sync` action. If conflicts → STOP.

### Phase 2 — Commit & Push

Run `commit` flow (including Ensure-Excluded, Ephemeral Tracked Sub-flow, scope resolution). Then push:

#### Submodules (PARALLEL — monorepo only, one Bash call each)

```bash
cd <SUBMODULE_ABSOLUTE_PATH> && rtk git add $SCOPE_EXPR && rtk git commit -m "<message>" && rtk git push origin <branch>
```

#### Parent / Root (ONE Bash call)

```bash
rtk git add $SCOPE_EXPR && rtk git commit -m "<message>" && rtk git push origin <branch>
```

## Final Status Report

**MANDATORY** at the end of every write action (`commit`, `push`, `merge`, `merge main`). Categorizes `rtk git status --short` per repo.

### Per-repo categorizer

```bash
echo "=== $(basename "$PWD") (branch: $(rtk git rev-parse --abbrev-ref HEAD)) ==="
rtk git status --short | while IFS= read -r line; do
  path=$(echo "$line" | awk '{print $NF}')
  case "$path" in
    .claude/.agent-state/*|.claude/.metrics/*|.claude/.pipeline-states/*|.claude/.detect-cache.json|.claude/.knowledge-seen.json)
      echo "  [ephemeral] $line" ;;
    *)
      if [ "${line:0:2}" = "??" ]; then
        echo "  [untracked] $line"
      else
        echo "  [pending]   $line"
      fi
      ;;
  esac
done
```

### Interpretation legend (printed once at the top)

```
  [ephemeral] — Claude/RTK runtime state; safe to ignore (excluded going forward).
  [pending]   — real code change still in worktree; decide whether to commit.
  [untracked] — new file not yet added; may be real or intentional scratch.
```

If a category is empty, omit its lines. If ALL repos are clean, print a single line: `All repos clean.`
