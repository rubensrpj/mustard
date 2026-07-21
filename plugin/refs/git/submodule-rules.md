# Submodule Rules Reference

> Detail for `/git`: monorepo/submodule handling, ephemeral runtime paths, auto-stash, per-repo procedures, and the forbidden-ops pointer. Branch flow & commit scope: `${CLAUDE_PLUGIN_ROOT}/refs/git/git-flow.md`.

## Contents
- Work branch per repo
- Step 0c — submodule HEAD check
- Ephemeral paths (single home)
- Auto-stash protocol
- sync / push per-repo procedures
- Commit: submodule steps
- PR per repo
- Final status report
- Forbidden operations
- Performance budget & rules

## Work branch per repo — a submodule never commits onto its base

The unit `{slug}` materialises as `{base}_{slug}` in EVERY repo it touches: the parent (cut by `work_branch_gate` on the first edit) and each dirty submodule (cut by `/git` at commit time). Each repo's `{base}_` prefix records THAT repo's own base — the parent's from `mustard.json#git.flow`, a submodule's its OWN default branch (a submodule is an independent repo, need not share the parent's flow).

Resolve a submodule's base + work branch (`<SUB_ABS>` absolute, via `git -C`, never `cd`):

```bash
PARENT_BRANCH=$(rtk git rev-parse --abbrev-ref HEAD)   # in the parent root; it is {base}_{slug}
SLUG=${PARENT_BRANCH#*_}                                # everything after the first `_`
SUB_BASE=$(rtk git -C "<SUB_ABS>" symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null | sed 's#^origin/##')
[ -z "$SUB_BASE" ] && SUB_BASE=$(rtk git -C "<SUB_ABS>" rev-parse --abbrev-ref HEAD)
SUB_WORK="${SUB_BASE}_${SLUG}"                          # same slug, the submodule's own base prefix
```

**Cut it at commit time, only when the submodule sits on its base with changes.** If the submodule's current branch equals `$SUB_BASE`, `rtk git -C <SUB_ABS> checkout -b "$SUB_WORK"` carries the edits over before staging. Already on `$SUB_WORK` (a later edit) → skip the checkout. **Never add/commit/push while a submodule is on its bare base** — the parent's branch-protection rule extended to every repo.

`<SUB_ABS>` is `<superproject-root>/<relative-path>` (`.gitmodules` paths are relative); `<superproject-root>` = `rtk git rev-parse --show-toplevel`. Always pass via `git -C`, never `cd <relative>`.

## Step 0c — submodule HEAD check (monorepo only)

Before any sync that traverses submodules, emit one state line per submodule:

```bash
for sm in $(rtk git config --file .gitmodules --get-regexp path | awk '{print $2}'); do
  ( cd "$sm" && echo "$sm: $(rtk git rev-parse --abbrev-ref HEAD) ($(rtk git rev-parse --short HEAD))" )
done
```

A submodule in **detached HEAD** → report BEFORE any checkout on it; the user decides (manual fix or proceed via the auto-stash protocol).

## Ephemeral paths — the single home

Claude/RTK write these continuously during a skill. They are not code, must never be tracked, and must never block a checkout:

```
.claude/.agent-state/
.claude/.metrics/
.claude/.pipeline-states/
.claude/.detect-cache.json
.claude/.knowledge-seen.json
```

**Submodule-safe exclude path** — `.git` is a *file* in submodules, so `.git/info/exclude` fails there. Always resolve the real path first (works in parent, submodule, worktree). Never edit `.git/info/exclude` directly.

```bash
EXCLUDE=$(rtk git rev-parse --git-path info/exclude)
```

**Ensure-excluded** — at the start of every write action, in each repo operated, idempotently append any missing path (grep-guarded):

```bash
EXCLUDE=$(rtk git rev-parse --git-path info/exclude)
for p in .claude/.agent-state/ .claude/.metrics/ .claude/.pipeline-states/ .claude/.detect-cache.json .claude/.knowledge-seen.json; do
  grep -qxF "$p" "$EXCLUDE" 2>/dev/null || echo "$p" >> "$EXCLUDE"
done
```

**Already-tracked ephemerals** — after ensure-excluded, `rtk git ls-files -- <paths>`; non-empty → run this sub-flow BEFORE the main commit, so ephemerals stay out of the user's diff:

1. Unlink from the index without deleting files: `rtk git rm --cached -r --ignore-unmatch <paths>`.
2. Dedicated commit `chore: ignore ephemeral runtime state`.
3. THEN the user-requested commit (resolved `--scope`).

## Auto-stash protocol

Every checkout a sub-flow performs (sync, or any branch switch) MUST be wrapped.

- **Sentinel** — `SENTINEL="mustard-git-autostash-<action>-$(date +%s%N)"`, one per action entry, reused for push/pop within it (different actions → different sentinels).
- **Protected push** — `rtk git stash push -u -m "$SENTINEL"` (`-u`: runtime files may be untracked).
- **Retry on checkout race** — Claude/RTK rewrite `.claude/.agent-state/*` between push and checkout → *"would be overwritten"*; max 3 attempts, then abort:

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

- **Safe pop** — NEVER pop blind; find the sentinel index first so pre-existing user stashes stay put:

```bash
IDX=$(rtk git stash list | grep -F "$SENTINEL" | head -n1 | sed -E 's/^stash@\{([0-9]+)\}.*$/\1/')
[ -n "$IDX" ] && rtk git stash pop "stash@{$IDX}"
```

Empty `$IDX` → do nothing.

## sync / push per-repo procedures

Submodules run in parallel (one Bash each), the parent after.

- **sync** (per repo) — ensure-excluded → auto-stash → `rtk git fetch origin "$BASE" && rtk git rebase "origin/$BASE"` → safe pop. Conflict → abort the rebase, report, STOP.
- **push** — Phase 1 `sync` (conflict → STOP). Phase 2 commit + push: each submodule onto its `$SUB_WORK` first (`checkout "$SUB_WORK" 2>/dev/null || checkout -b "$SUB_WORK"`, then `add $SCOPE_EXPR && commit && push -u origin "$SUB_WORK"`), then the parent (`add $SCOPE_EXPR && commit && push origin <parent-work-branch>`). Never push a base.

## Commit: submodule steps

Analyze in ONE parallel batch: `rtk git status --short`, `rtk git submodule status` (skip if no `.gitmodules`), `rtk git diff --stat`, `rtk git log --oneline -5`.

Then launch **one parallel Task agent per dirty submodule** (inherits the session model). Each puts the submodule on its `$SUB_WORK` (above), then stages + commits in ONE chained Bash:

```bash
rtk git -C "<SUB_ABS>" checkout "$SUB_WORK" 2>/dev/null || rtk git -C "<SUB_ABS>" checkout -b "$SUB_WORK"; \
rtk git -C "<SUB_ABS>" add $SCOPE_EXPR && rtk git -C "<SUB_ABS>" commit -m "<message>"
```

`staged` scope → skip the `add`. The commit lands on the work branch, never the base.

### Then return to the parent — the gitlink step (MANDATORY)

Once every submodule agent has committed, the parent's pointer to each submodule is **stale**: the
parent still references the OLD commit, and that shows up as a lone ` M <sub>` line — the "only
dirt left". Re-sample and stage it **explicitly**; never rely on `add -A` catching it as a side
effect (a `staged`/pattern scope misses it entirely, and the pre-commit analysis at the top of this
section ran BEFORE the submodule commits, so it never saw the moved pointer):

```bash
rtk git submodule status; \
rtk git add -- "<SUB_PATH>" ["<SUB_PATH>"…]
```

Then include it in the parent's commit. **The parent may have nothing of its own to change and
STILL owe this commit — the moved gitlink IS the change**; in that case commit it alone
(`chore(submodule): sincroniza ponteiro do submodulo`). Skipping it leaves the super-repo pointing
at a commit that no longer reflects the submodule's published work.

## PR per repo — submodules before parent

`/git pr` opens ONE PR per repo, **submodules FIRST**: the parent commit bumps each submodule's gitlink to a submodule-work-branch commit, so merging the submodule PR first lands that commit on its base and the parent pointer never dangles.

1. Each submodule ahead of its base (`rtk git -C <SUB_ABS> rev-parse "$SUB_BASE..$SUB_WORK"` non-empty): `( cd "<SUB_ABS>" && rtk gh pr create --base "$SUB_BASE" --head "$SUB_WORK" --fill )`. The `( … )` subshell isolates the `cd`; the "no `cd`" rule targets `git`, not `gh` (which reads the repo from cwd). Existing PR → print its URL.
2. Then the parent — `rtk gh pr create --base "$BASE" --head <parent-work-branch> --fill`.
3. No return to base — every repo stays live on its work branch; a later `push`/`pr` re-targets the SAME PR until `pr close`.

A base→base `pr` opens its single PR only — no push, no submodule branches, no return.

## Final status report

**MANDATORY** at the end of every write action. Categorize `rtk git status --short` per repo:

```bash
echo "=== $(basename "$PWD") (branch: $(rtk git rev-parse --abbrev-ref HEAD)) ==="
rtk git status --short | while IFS= read -r line; do
  path=$(echo "$line" | awk '{print $NF}')
  case "$path" in
    .claude/.agent-state/*|.claude/.metrics/*|.claude/.detect-cache.json|.claude/.knowledge-seen.json)
      echo "  [ephemeral] $line" ;;
    *) [ "${line:0:2}" = "??" ] && echo "  [untracked] $line" || echo "  [pending]   $line" ;;
  esac
done
```

Legend: `[ephemeral]` runtime state, safe to ignore; `[pending]` real change still in the worktree; `[untracked]` new file not yet added. Omit empty categories; all repos clean → `All repos clean.`

**Read a lone ` M <submodule-path>` in the PARENT as a MISSED gitlink step**, not as ordinary
pending work: it means the submodule committed but the parent was never re-pointed. Go back and run
the gitlink step above before declaring the action done.

## Forbidden operations

The destructive-ops ban has ONE home — `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Destructive-ops Law` (`permissions.deny` + the `bash_command_gate` residue). Do NOT restate the command list here. Rule of thumb: every transition stays recoverable via `rtk git reflog` / `rtk git stash list` — prefer the safe unlink (`rtk git rm --cached`), `info/exclude`, and the auto-stash protocol above.

## Performance budget & rules

- Max 1 Task agent per dirty submodule; max 1 Bash per agent (chained); max 3 checkout retries per repo, then abort.
- Submodules BEFORE parent in every action (sync, push, commit, pr).
- Every repo carries the unit on its own `{base}_{slug}` branch, cut from THAT repo's base — never commit/push onto a bare base, in any repo.
- Prefix every git invocation with `rtk` (inside `&&`/`;` chains and `$(...)` too).
- Single repo → skip all submodule steps.
