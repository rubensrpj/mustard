# Merge Protocol Reference

> Detail for auto-stash, forbidden operations, and final status report.

## Auto-stash Protocol

EVERY checkout a sub-flow performs (sync, or any branch switch inside an action) MUST be wrapped.

### Sentinel

`mustard-git-autostash-<action>-<unix_timestamp_ns>` — one per action attempt. Generate once per action entry (`SENTINEL="mustard-git-autostash-<action>-$(date +%s%N)"`) and reuse for push/pop within that action. Different actions → different sentinels (avoids collisions on parallel submodule ops).

### Protected stash push

`rtk git stash push -u -m "$SENTINEL"` (`-u` because runtime-regenerated files may be untracked).

### Retry on checkout race

Race: between `stash push` and `checkout <target>`, Claude/RTK rewrite `.claude/.agent-state/*` → checkout aborts with *"would be overwritten by checkout"*. Protocol (max 3 attempts, then abort):

```bash
ATTEMPT=1; MAX=3
while [ $ATTEMPT -le $MAX ]; do
  rtk git stash push -u -m "$SENTINEL" 2>/dev/null
  CO_OUT=$(rtk git checkout "$TARGET" 2>&1); CO_RC=$?
  [ $CO_RC -eq 0 ] && break
  echo "$CO_OUT" | grep -qE "would be overwritten|local changes" \
    && ATTEMPT=$((ATTEMPT+1)) || { echo "checkout failed: $CO_OUT" >&2; exit 1; }
done
[ $ATTEMPT -gt $MAX ] && { echo "checkout race unresolved after $MAX attempts"; exit 1; }
```

### Safe stash pop (preserving pre-existing user stashes)

**NEVER** `rtk git stash pop` without first locating the exact sentinel. Pre-existing user stashes at `stash@{0}` must not be disturbed.

```bash
IDX=$(rtk git stash list | grep -F "$SENTINEL" | head -n1 | sed -E 's/^stash@\{([0-9]+)\}.*$/\1/')
[ -n "$IDX" ] && rtk git stash pop "stash@{$IDX}"
```

Empty `$IDX` (sentinel not found) → do nothing.

## Forbidden Operations

Irreversible at filesystem or history level — **BANNED**.

| Forbidden | Reversible alternative |
|-----------|------------------------|
| `rm -f` / `rm -rf <path>` | `rtk git rm --cached <path>` (preserves file on disk) |
| `git clean -fd` / `-fdx` | Append to `$(rtk git rev-parse --git-path info/exclude)` |
| `git checkout -f` / `--force` | Auto-stash Protocol with retry |
| `git reset --hard` | `rtk git stash push` snapshot, then `rtk git checkout <ref>` |
| Forced unlink of lock files | Investigate process holding lock; never delete blindly |

Rationale: all state transitions must be recoverable via `rtk git reflog` / `rtk git stash list`. Filesystem-destructive shortcuts silently lose user work.

## sync — Per-repo Procedure

1. Ensure-excluded (ephemerals) — silent, idempotent.
2. Auto-stash: `SENTINEL="mustard-git-autostash-sync-$(date +%s%N)"`.
3. Fetch + rebase: `rtk git fetch origin "$PARENT" && rtk git rebase "origin/$PARENT"`.
4. Safe stash pop (sentinel index).
5. Rebase conflicts → abort rebase, report, STOP.

Submodules in parallel (one Bash call each). Parent runs after.

## push — Sequential Phase Procedure

**Phase 1 — Sync.** Execute `sync`. Conflicts → STOP. **Phase 2 — Commit & Push.** Run `commit` flow (Ensure-Excluded, Ephemeral Tracked sub-flow, scope resolution). Then push:

- **Submodules** (parallel, monorepo only, one Bash each): put the submodule on its `{base}_{slug}` work branch FIRST, then commit + push THAT branch — never its base. Resolve `$SUB_BASE`/`$SUB_WORK` per submodule-rules.md § *Work branch per repo*:
  `rtk git -C <SUB_ABS> checkout "$SUB_WORK" 2>/dev/null || rtk git -C <SUB_ABS> checkout -b "$SUB_WORK"; rtk git -C <SUB_ABS> add $SCOPE_EXPR && rtk git -C <SUB_ABS> commit -m "<msg>" && rtk git -C <SUB_ABS> push -u origin "$SUB_WORK"`.
- **Parent/Root** (one Bash): `rtk git add $SCOPE_EXPR && rtk git commit -m "<msg>" && rtk git push origin <parent-work-branch>`.

## Final Status Report

**MANDATORY** at the end of every write action (`commit`, `push`). Categorizes `rtk git status --short` per repo.

```bash
echo "=== $(basename "$PWD") (branch: $(rtk git rev-parse --abbrev-ref HEAD)) ==="
rtk git status --short | while IFS= read -r line; do
  path=$(echo "$line" | awk '{print $NF}')
  case "$path" in
    .claude/.agent-state/*|.claude/.metrics/*|.claude/.detect-cache.json|.claude/.knowledge-seen.json)
      echo "  [ephemeral] $line" ;;
    *)
      [ "${line:0:2}" = "??" ] && echo "  [untracked] $line" || echo "  [pending]   $line" ;;
  esac
done
```

Legend (printed once at the top): `[ephemeral]` Claude/RTK runtime state — safe to ignore (excluded going forward); `[pending]` real code change still in worktree — decide whether to commit; `[untracked]` new file not yet added — may be real or intentional scratch.

Empty categories omitted. ALL repos clean → single line `All repos clean.`
