# Git Flow Reference

> Detail for `/git`: branch flow, base derivation, the worktree contract, and commit scope. Command: `${CLAUDE_PLUGIN_ROOT}/commands/git.md`. Submodule / ephemeral / auto-stash detail: `${CLAUDE_PLUGIN_ROOT}/refs/git/submodule-rules.md`.

## Contents
- Configuration & flow resolution
- Work branches & the gate
- Worktree contract
- The notebook — the porta rule
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

**The gate** (`work_branch_gate`, PreToolUse Write/Edit) judges the LOCAL tree hosting the edit, so a nested worktree on a work branch is never blocked by the main checkout's branch. With no marker, a direct edit on a bare integration base is **denied** — except `.claude/plans/`, harness state authored *before* the unit exists (`.claude/spec/` is NOT carved out: the spec belongs to the unit, so it is written after the branch exists) — while any `{base}_*` work branch edits freely.

With a pending-unit marker the gate asks WHERE the checkout is before it cuts:

| The checkout is on | What happens |
|--------------------|--------------|
| a bare integration base — nobody's work | cut `{base}_{slug}` off the freshly fetched base, **in place**, silently |
| THIS unit's branch already | consume the marker, say nothing |
| a detached / unreadable HEAD | the in-place cut, unchanged — an unmeasured position never triggers a refusal nobody asked for |
| ANOTHER unit's branch, tree CLEAN | the in-place cut — nothing rides along, and the branch is still cut off its base |
| **ANOTHER unit's branch, with uncommitted work** | **REFUSED**: the edit is denied, and the refusal names that branch, the paths holding the uncommitted work, and what unblocks it — **commit or stash that work**, then open the second unit |

The last row is the whole point. A checkout carries the uncommitted work of whoever is in it, and `git checkout` moves that work along — so taking the checkout for a second unit silently drags the first unit's edits onto the second unit's branch. The harness refuses instead of guessing: it never stashes for you, never moves anyone's work, and never diverts the unit somewhere you did not ask to go. The SAME refusal is taken by the cut `spec-draft` performs at approval, which is the door that opens first — one decision, both doors. Fail-open elsewhere: a git failure warns and reconciles the marker to the branch actually active, never blocks. There is no standing "you could isolate this" nudge.

**Monorepo:** the gate cuts the branch in the PARENT only. Each dirty submodule gets its OWN `{base}_{slug}` branch (its own base prefix), cut by `/git` at commit time — see `submodule-rules.md`.

## Isolation contract — the branch IS the unit; a second unit in parallel is REFUSED, never diverted

Every unit lives on its OWN branch `{base}_{slug}`, cut in the MAIN checkout at APPROVAL by `spec-draft` — so the whole unit is written on it: `spec.md`, the waves, the ceremony and the code alike. That branch IS the isolation. The `{base}_` prefix is load-bearing — `/git` reads it to target the PR — and the branch is cut FROM `{base}`, so the right base in yields the right PR target out.

**A second unit is refused, not accommodated.** When the checkout already holds another unit's branch with uncommitted work, the harness stops and says so: commit or stash that work first. It does not stash on your behalf and it does not open a second workspace for you. Cutting a worktree for the second unit was tried and withdrawn — a fresh worktree receives only what git tracks (no `.env`, no `node_modules`), so making it usable meant linking those directories back to the main checkout, and `git worktree remove` **descends** a Windows directory junction: removing the worktree deleted the main checkout's own directory, with and without `--force`. The harness therefore plants nothing inside a worktree beyond `git submodule update`.

You can still work in parallel — cut a worktree yourself (`git worktree add`, or Claude Code's own isolated tasks), or use a second clone. What the harness will not do is move your uncommitted work for you.

**The collector reaps what is ORPHANED.** `worktree-gc` runs at every SessionStart with `--apply` — it removes, it does not report. It never touches a work unit's worktree (`{base}_…` — that is `git-settle`'s job exclusively) and never one holding uncommitted or untracked work, whatever its age. For the harness's own scratch trees the name carries the PID of whoever cut it, so "orphan or busy" is a question with an exact answer: owner gone → collected now, not in a week. Age stays only as the fallback for a worktree whose owner cannot be read — unmeasured ownership authorises nothing.

- **Desktop / background CLI** — isolated automatically. A Desktop branch has no `{base}_` prefix, so `/git` falls back to the primary base (`git.flow["*"]`); pass an explicit `<target>` for any other base.
- **Foreground CLI** — the branch is already out from approval, so the isolation step DEGRADES rather than cutting twice: `EnterWorktree name={base}_{slug}` (the `branch` echoed by `emit-pipeline`) answers with the checkout that already holds it (`inPlace:true`, nothing created). `git worktree add` over a branch another tree holds is what git refuses with exit 128 — the degrade is what keeps that from ending the step. When the branch is NOT already out, the plugin's `WorktreeCreate` hook replaces the native cut: a `{base}_` name with a DECLARED base → fresh `origin/{base}` (idempotent; attaches an existing branch); any other name (the harness's own slug, e.g. `recursing-benz-063389`) → the native default cut, refused while the tree is dirty; an UNDECLARED `{base}_` prefix → loud abort. `mustard-rt run work-unit-open --spec {slug} --base {base}` remains the manual face of the same engine (then `EnterWorktree path={path}`).
- **Abandoning an UNMERGED unit** — `/git delete <branch>`, run from an integration base. ONE gesture removes the unit whole (open PR, worktree, local branch, remote branch), and it refuses from a work branch, over a bare base, and over a name no ref carries. `pr close` stays the ritual for MERGED units only.

## The notebook — the porta rule

Every unit turns up something true but off-topic: a defect three files away, a rename nobody asked for, a question the current Acceptance Criteria cannot answer. There are exactly two doors for it, and the choice is one line:

| What surfaced | Where it goes |
|---------------|---------------|
| It belongs to THIS spec (a criterion is wrong, a boundary moved) | **Amend the spec** — the amendment path (`ac-amend` for a frozen criterion). The unit's own contract changes, and the change is proved. |
| It does NOT belong to this spec | **The notebook** — `mustard-rt run notebook --add "one line"`. The unit's contract is untouched. |

**Per branch, never one global list.** The file lives at `.claude/spec/{slug}/notebook.md` — the same directory the spec, the waves and the ceremony are materialized into — so it travels with the unit, shows up in the PR diff, and disappears with the branch when `/git delete` retires it. Which work produced a pendency is information: it sets the item's priority and is the only thing that still makes it legible weeks later. A shared list loses that on the first append.

**Closing the loop.** Read it back with `mustard-rt run notebook` (no `--add`), optionally naming another unit with `--unit {base}_{slug}`. `/git pr` prints it once the PR is open: the work is in review, so the notebook is now the next cycle's prompt — it goes back to the base gate as the next request, and the loop closes. An empty notebook prints nothing; items are never invented to fill it.

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
