# Submodule Rules Reference

> Detail for `/git`: monorepo/submodule handling, ephemeral runtime paths, auto-stash, per-repo procedures, and the forbidden-ops pointer. Branch flow & commit scope: `${CLAUDE_PLUGIN_ROOT}/refs/git/git-flow.md`.

## Contents
- Work branch per repo
- Step 0c — submodule HEAD check
- Ephemeral paths (single home)
- Auto-stash protocol
- sync / push per-repo procedures
- Commit: submodule steps (gitlink conditioned on reachability + the bump step)
- PR per repo (parent as draft while a submodule PR is open)
- Close per repo
- Final status report
- Forbidden operations
- Performance budget & rules

## Work branch per repo — a submodule never commits onto its base

The unit materialises in EVERY repo it touches: the parent (cut by `work_branch_gate` on the first edit) and each dirty submodule (cut by `/git` at commit time). The NAME travels unchanged — a unit is `{kind}/{slug}` everywhere — while the BASE is per repo: the parent's comes from its kind through `mustard.json#git.flow`, a submodule's is its OWN default branch (a submodule is an independent repo, need not share the parent's flow). A unit still in the older `{base}_{slug}` shape is the one case where the name differs per repo: its prefix records THAT repo's base, so it is re-prefixed on the way in.

Resolve a submodule's base + work branch (`<SUB_ABS>` absolute, via `git -C`, never `cd`):

```bash
PARENT_BRANCH=$(rtk git rev-parse --abbrev-ref HEAD)   # in the parent root
SUB_BASE=$(rtk git -C "<SUB_ABS>" symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null | sed 's#^origin/##')
[ -z "$SUB_BASE" ] && SUB_BASE=$(rtk git -C "<SUB_ABS>" rev-parse --abbrev-ref HEAD)
case "$PARENT_BRANCH" in
  */*) SUB_WORK="$PARENT_BRANCH" ;;                    # {kind}/{slug} — the SAME name in every repo
  *)   SUB_WORK="${SUB_BASE}_${PARENT_BRANCH#*_}" ;;   # older {base}_{slug} — this repo's base prefix
esac
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

### Then return to the parent — the gitlink step (CONDITIONED on reachability)

Once every submodule agent has committed, the parent's pointer to each submodule is **stale**: the
parent still references the OLD commit, and that shows up as a lone ` M <sub>` line — the "only
dirt left". The reflex is to stage it and be done. That reflex records a pointer to a
**work-branch** SHA that exists nowhere on the submodule's base — by construction one merge behind
the real target — which is precisely why the PR section below orders the submodule to merge FIRST.
The two instructions cannot both be obeyed, so the stage is **conditioned**, not mandatory.

Ask git the reachability question — the same question finding 1 asks about a branch, asked here
about a commit. Never rely on `add -A` catching the pointer as a side effect (a `staged`/pattern
scope misses it entirely, and the pre-commit analysis at the top of this section ran BEFORE the
submodule commits, so it never saw the moved pointer):

```bash
rtk git submodule status
SUB_SHA=$(rtk git -C "<SUB_ABS>" rev-parse HEAD)
if rtk git -C "<SUB_ABS>" merge-base --is-ancestor "$SUB_SHA" "origin/$SUB_BASE"; then
  rtk git add -- "<SUB_PATH>"          # already on the submodule's base — safe to record
else
  echo "  [pending-bump] <SUB_PATH> — $SUB_SHA is not yet on origin/$SUB_BASE"
fi
```

**Reachable** → include it in the parent's commit. **The parent may have nothing of its own to
change and STILL owe this commit — the moved gitlink IS the change**; in that case commit it alone
(`chore(submodule): sincroniza ponteiro do submodulo`).

**Not reachable** → do NOT stage it. The parent commits what is its own, and the lone ` M <sub>`
that remains is a NAMED pending state, `[pending-bump]`, not leftover dirt and not a missed step.
It is cleared by the bump step below, after the submodule PR merges.

### The bump step — after the submodule PR merges

The bump is the parent commit that moves the pointer to a commit **already present on the
submodule's base**. It runs in `pr close`, between the submodule's settle and the parent's, and
BEFORE the parent PR merges — the parent PR sits as a draft until it lands (see the PR section).

```bash
rtk git -C "<SUB_ABS>" fetch origin "$SUB_BASE"
rtk git submodule status
rtk git add -- "<SUB_PATH>" && rtk git commit -m "chore(submodule): bump pointer" && rtk git push
```

Re-sample first: after the submodule PR merged, its base carries the commit (or the squash of it),
and that is the SHA the parent must record. Then `rtk gh pr ready` on the parent PR — see the close
section.

## PR per repo — submodules before parent

`/git pr` opens ONE PR per repo, **submodules FIRST**: the submodule's PR is what lands its commit on its own base, and only then can the parent record a pointer that does not dangle. Until it merges the parent carries a `[pending-bump]`, not a gitlink — and its own PR stays a draft so nothing can merge past that gap.

1. Each submodule ahead of its base (`rtk git -C <SUB_ABS> rev-parse "$SUB_BASE..$SUB_WORK"` non-empty): `( cd "<SUB_ABS>" && rtk gh pr create --base "$SUB_BASE" --head "$SUB_WORK" --fill )`. The `( … )` subshell isolates the `cd`; the "no `cd`" rule targets `git`, not `gh` (which reads the repo from cwd). Existing PR → print its URL.
2. Then the parent — **as a DRAFT while ANY submodule PR from step 1 is still open**:

   ```bash
   rtk gh pr create --base "$BASE" --head <parent-work-branch> --fill \
     --draft --body "Blocked by <sub PR url>"
   ```

   Ordering alone never blocked anything: "submodules before parent" governs PR OPENING, and on
   GitHub the two PRs are siblings that anyone can merge in either order — the bump window is the
   delta between the two merges. GitHub's own rule closes it: **a draft pull request cannot be
   merged** until it is marked ready. That is the mechanical half the sentence lacked.

   Two consequences to expect, both documented by GitHub: a draft PR does **not** automatically
   request review from code owners (CODEOWNERS) — those requests fire when the draft is marked
   ready, so silence from reviewers before that is the design, not a misconfiguration; and marking
   it ready is `rtk gh pr ready [<number>|<url>|<branch>]`, which runs in the close ritual after
   the bump lands (`--undo` puts it back to draft).

   Every submodule PR already merged (or no submodule carries the unit) → open the parent normally,
   `--fill`, no draft.
3. No return to base — every repo stays live on its work branch; a later `push`/`pr` re-targets the SAME PR until `pr close`.

A base→base `pr` opens its single PR only — no push, no submodule branches, no return.

## Close per repo — submodules before parent

`pr close` is the exit ritual of ONE unit, and the unit lives in every repo it touched. It closes the same way it opened: **submodules FIRST**, then the parent — merging the submodule PR first is what keeps the parent's gitlink pointing at a commit that exists on the submodule's base.

1. **Each submodule whose own PR already merged** — from `<SUB_ABS>`: `mustard-rt run git-settle --unit "$SUB_WORK"` (confirm merged, advance `$SUB_BASE`, delete the local + remote branch). Not merged → it refuses and touches NOTHING; merge that PR first. Merged but the base did not advance → it refuses the same way (`ok:false`, `reason:"base-behind"`, both branches alive): clear what `baseAdvance.reason` names and rerun the command in `nextAction` — see the refusal shape in `${CLAUDE_PLUGIN_ROOT}/commands/git.md` § `pr close`. A submodule carries no `mustard.json`, so settle reads the bases from the superproject's `git.flow` — a `$SUB_BASE` that flow never names is refused with `no-base-prefix`, and the refusal prints the root, the config root and the bases it knows.
2. **Then the bump + ready, in the parent — before the parent settles.** The submodule commit now
   lives on its base, so the pointer the commit step left as `[pending-bump]` finally has a
   reachable target. Run the bump step above (re-sample, commit the pointer ALONE, push), then
   `rtk gh pr ready` on the parent PR — it opened as a draft precisely so it could not merge ahead
   of the submodule, and `ready` is also what requests the code owners. Skipping this leaves the
   super-repo pointing at a work-branch commit that the submodule's branch deletion is about to
   make unreachable.
3. **Then the parent** — the `pr close` procedure in `${CLAUDE_PLUGIN_ROOT}/commands/git.md`, run
   after the parent PR merges.
4. **Read the report, not the action.** `git-settle` acts ONLY on the repo it was pointed at, but `repos` carries one entry per repo of the unit (`settled` + `reason`) and `complete` stays false while any repo still holds it. `complete:false` with a submodule entry means step 1 was skipped for that repo — go do it; a bare `"action":"settled"` no longer means the unit is gone.

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

Legend: `[ephemeral]` runtime state, safe to ignore; `[pending]` real change still in the worktree; `[untracked]` new file not yet added; `[pending-bump]` a submodule pointer deliberately NOT staged because its SHA is not yet on the submodule's base. Omit empty categories; all repos clean → `All repos clean.`

**A lone ` M <submodule-path>` in the PARENT has TWO readings, and the submodule's PR decides
which.** Do not classify it as ordinary pending work:

- The submodule PR is still OPEN (or was never opened) → `[pending-bump]`. This is the expected
  state after a conditioned gitlink step: recording the pointer now would name a work-branch SHA
  that exists nowhere on the submodule's base. Leave it. The bump step clears it in `pr close`.
- The submodule PR already MERGED → a MISSED step. Its commit is on the base now, so the pointer
  has a reachable target and nothing justifies the parent still aiming at the old one. Run the
  gitlink step (or the bump step, if the close is already under way) before declaring the action
  done.

One command separates them — the same reachability question, never a guess:

```bash
rtk git -C "<SUB_ABS>" merge-base --is-ancestor "$(rtk git -C "<SUB_ABS>" rev-parse HEAD)" "origin/$SUB_BASE" \
  && echo "reachable → stage it" || echo "[pending-bump]"
```

## Forbidden operations

The destructive-ops ban has ONE home — `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Destructive-ops Law` (`permissions.deny` + the `bash_command_gate` residue). Do NOT restate the command list here. Rule of thumb: every transition stays recoverable via `rtk git reflog` / `rtk git stash list` — prefer the safe unlink (`rtk git rm --cached`), `info/exclude`, and the auto-stash protocol above.

## Performance budget & rules

- Max 1 Task agent per dirty submodule; max 1 Bash per agent (chained); max 3 checkout retries per repo, then abort.
- Submodules BEFORE parent in every action (sync, push, commit, pr).
- Every repo carries the unit on its own work branch (`{kind}/{slug}`; an older unit re-prefixed as `{base}_{slug}`), cut from THAT repo's base — never commit/push onto a bare base, in any repo.
- Prefix every git invocation with `rtk` (inside `&&`/`;` chains and `$(...)` too).
- Single repo → skip all submodule steps.
