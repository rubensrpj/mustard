# Git Flow Reference

> Detail for `/git`: what a base is and where it is measured, the worktree contract, and commit scope. Command: `${CLAUDE_PLUGIN_ROOT}/commands/git.md`. Submodule / ephemeral / auto-stash detail: `${CLAUDE_PLUGIN_ROOT}/refs/git/submodule-rules.md`.

## Contents
- Configuration — what `git.flow` decides, and what git decides
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
{ "git": { "flow": { "*": "dev", "dev": "main" }, "protected": ["main"] } }
```

`GitConfig` carries exactly three keys — `flow`, `protected`, `provider` — and nothing else. **`submodules` is not one of them.** It lived here once, written by `init` and read by nobody, and was removed: whether a repository has submodules is read from `.gitmodules` on disk when it is needed, because a declaration written at install time goes stale the moment someone adds one. An unknown key in an existing `mustard.json` is ignored **in silence** on load, so an older file keeps working and a copied-in `"submodules": true` is never reported — which is why it is called out here instead of left to be discovered.

**`git.flow` PRE-SELECTS, it does not permit.** Every non-`*` key ∪ every value (`{"*":"dev","dev":"main"}` → `dev`, `main`) is the set a base picker offers FIRST and the promotion map `/mustard:pr open` walks. **It refuses nothing, and the installer writes no flow at all** — a fresh install has an empty map on purpose, and everything below still works.

Two questions used to be answered with that one list, and answering both with a closed list is what made the first one wrong:

| Question | Where the answer comes from now |
|----------|--------------------------------|
| Where may a unit be cut from? | **git** — every branch `origin` really has, newest first (`mustard-rt run base-candidates`). A branch cut last Tuesday is a valid base; no file has to name it. |
| Where is a direct commit forbidden? | **the protected set** — the remote's own default branch (`git symbolic-ref refs/remotes/origin/HEAD`) ∪ `mustard.json#git.protected`. Normally a set of ONE. |
| Where does the picker open, and what promotes into what? | `git.flow` — a hint and a promotion map, nothing more. |

Read what is really protected here with `mustard-rt run doctor --check branch-protection`.

**Flow resolution** (promotion only) — match the current branch against `flow` keys, exact before glob; `*` is the fallback for anything unlisted. `dev` → `main` (promotion via `/mustard:pr open`); a branch with no `flow[B]` is terminal and needs an explicit target.

## Work branches & the gate

Every work unit runs on its own `{kind}/{slug}` branch (e.g. `fix/aba-atividade`). The prefix **records what the unit IS**, which is what an operator reading a branch list needs; the base is no longer in the name and is never parsed back out of it.

| Prefix | What it is |
|--------|------------|
| `feature/` | new capability |
| `fix/` | a correction that travels the ordinary route, with the next release |
| `hotfix/` | a correction that does NOT wait for that route |

The list is a **suggestion, not a permitted set**: `chore/`, `refactor/`, `docs/` or any token that can be a git ref segment names a unit exactly the same way.

**The prefix does NOT decide the base.** It used to — a table like the one above said `feature` → the `*` base, `hotfix` → the outermost one — and that inference is gone. The base is the OPERATOR's answer, asked ONCE per unit against the real branch list, and `hotfix` is a **destination, not a kind of work**: the same code change is a fix or a hotfix depending only on where it lands, so nothing in the request text tells them apart. The router asks kind and base together in one pre-marked question (orchestrator rules § Dispatch — with the likely answer already marked; the base row is skipped when the repository leaves only one candidate). The branch is **auto-created off the chosen base on the first file edit**: the answer rides in as `emit-pipeline --type <kind> [--base <base>]`, and `work_branch_gate` cuts + checks the branch out on the first `Write`/`Edit`. Read-only requests never branch.

An explicit `--base` is validated against the branches the repository really has — a name no branch carries is refused, and the refusal LISTS what is there. It is never refused for missing from `git.flow`.

**Old names keep working.** A unit already in flight as `{base}_{slug}` (e.g. `dev_aba-atividade`) is still recognised as that unit and still resolves to its base by longest-match — first against the pre-selected bases, then against the branches the repository really has, so a unit whose base the flow never named is not orphaned. Nothing is renamed.

**The gate** (`work_branch_gate`, PreToolUse Write/Edit) judges the LOCAL tree hosting the edit, so a nested worktree on a work branch is never blocked by the main checkout's branch. With no marker, a direct edit on a **protected** branch is **denied** — that is the measured set above, not "any branch `git.flow` mentions": a project promoting through `dev` may now both cut from it AND commit on it — except `.claude/plans/`, harness state authored *before* the unit exists (`.claude/spec/` is NOT carved out: the spec belongs to the unit, so it is written after the branch exists). Any work branch, in either shape, edits freely.

With a pending-unit marker the gate asks WHERE the checkout is before it cuts:

| The checkout is on | What happens |
|--------------------|--------------|
| a branch that is nobody's work | cut `{kind}/{slug}` off the freshly fetched base the operator chose, **in place**, silently |
| THIS unit's branch already | consume the marker, say nothing |
| a detached / unreadable HEAD | the in-place cut, unchanged — an unmeasured position never triggers a refusal nobody asked for |
| ANOTHER unit's branch, tree CLEAN | the in-place cut — nothing rides along, and the branch is still cut off its base |
| **ANOTHER unit's branch, with uncommitted work** | **REFUSED**: the edit is denied, and the refusal names that branch, the paths holding the uncommitted work, and what unblocks it — **commit or stash that work**, then open the second unit |
| **ANOTHER unit's branch, work not measurable** | **REFUSED** too: when `git status` cannot answer for the tree, "I could not measure" is not "there is nothing here". Refusing costs you one commit; the other way costs you the work |

The last two rows are the whole point. A checkout carries the uncommitted work of whoever is in it, and `git checkout` moves that work along — so taking the checkout for a second unit silently drags the first unit's edits onto the second unit's branch. The harness refuses instead of guessing: it never stashes for you, never moves anyone's work, and never diverts the unit somewhere you did not ask to go. The SAME refusal is taken by the cut `spec-draft` performs at approval, which is the door that opens first — one decision, both doors, in the main checkout and inside a linked worktree alike. Fail-open elsewhere: a git failure warns and reconciles the marker to the branch actually active, never blocks. There is no standing "you could isolate this" nudge.

**`.claude/spec/…` is work.** Everything the harness generates for a unit — the spec, the waves, `ac-proof.json`, the change log, the review verdicts — lives IN the work branch and is integrated into the base at merge time, so between approval and the merge a unit's uncommitted work usually IS its `.claude/spec/…` and nothing else. The refusal counts those paths, and names them. What it does not count is the harness's own scratch (`.claude/.session/`, `.cache/`, `.harness/`, `spec/*/.events/`, …) — the probe carries that list ITSELF, because a project's `.claude/.gitignore` may predate any given release and a decision this consequential cannot rest on configuration that might be stale. It asks git to enumerate untracked files rather than collapse them, so a `.claude/` holding only scratch and one holding scratch plus a real `spec.md` are told apart instead of arriving as the same line.

**Monorepo:** the gate cuts the branch in the PARENT only. Each dirty submodule carries the unit under the SAME `{kind}/{slug}` name, cut from THAT repo's own base by `/git` at commit time (a unit still in the old shape keeps being re-prefixed with the submodule's base) — see `submodule-rules.md`.

## Isolation contract — the branch IS the unit; a second unit in parallel is REFUSED, never diverted

Every unit lives on its OWN branch `{kind}/{slug}`, cut in the MAIN checkout at APPROVAL by `spec-draft` — so the whole unit is written on it: `spec.md`, the waves, the ceremony and the code alike. That branch IS the isolation. The PR target does NOT come from the prefix: `/git` reads the base **recorded with the unit** at the cut, which is the operator's own answer, and falls back to `origin/HEAD` when nothing recorded one — so the right answer to the opening question yields the right PR target out. A unit still named `{base}_{slug}` is read by its prefix, exactly as before.

**A second unit is refused, not accommodated.** When the checkout already holds another unit's branch with uncommitted work, the harness stops and says so: commit or stash that work first. It does not stash on your behalf and it does not open a second workspace for you. Cutting a worktree for the second unit was tried and withdrawn — a fresh worktree receives only what git tracks (no `.env`, no `node_modules`), so making it usable meant linking those directories back to the main checkout, and `git worktree remove` **descends** a Windows directory junction: removing the worktree deleted the main checkout's own directory, with and without `--force`. The harness therefore plants nothing inside a worktree beyond `git submodule update`.

You can still work in parallel — cut a worktree yourself (`git worktree add`, or Claude Code's own isolated tasks), or use a second clone. What the harness will not do is move your uncommitted work for you.

**The collector reaps what is ORPHANED.** `worktree-gc` runs at every SessionStart with `--apply` — it removes, it does not report. It never touches a work unit's worktree (any name that reads as a unit — `{kind}/{slug}`, or the older `{base}_…`; that is `git-settle`'s job exclusively), nor the `feature/`, `fix/`, `hotfix/` directories those worktrees sit inside, and never one holding uncommitted or untracked work, whatever its age. For the harness's own scratch trees the name carries the PID of whoever cut it, so "orphan or busy" is a question with an exact answer: owner gone → collected now, not in a week. Age stays only as the fallback for a worktree whose owner cannot be read — unmeasured ownership authorises nothing.

- **Desktop / background CLI** — isolated automatically. A Desktop branch reads as nobody's unit (no kind prefix, no `{base}_` prefix naming a real branch), so `/git` falls back to the primary base (`git.flow["*"]`, else `origin/HEAD`); pass an explicit `<target>` for any other base.
- **Foreground CLI** — the branch is already out from approval, so the isolation step DEGRADES rather than cutting twice: `EnterWorktree name={kind}/{slug}` (the `branch` echoed by `emit-pipeline`) answers with the checkout that already holds it (`inPlace:true`, nothing created). `git worktree add` over a branch another tree holds is what git refuses with exit 128 — the degrade is what keeps that from ending the step. When the branch is NOT already out, the plugin's `WorktreeCreate` hook replaces the native cut: a `{kind}/{slug}` name → fresh `origin/{base}` for the base recorded with the unit, else the project's primary one (idempotent; attaches an existing branch), landing at `.claude/worktrees/{kind}/{slug}`; a `{base}_` name whose prefix names a branch the repository REALLY has → the same, by its prefix, declared or not; any other name — the harness's own slug (`recursing-benz-063389`) and any `x_y` whose `x` is no branch here — → the native default cut, refused while the tree is dirty. Only a name that cannot be a worktree at all aborts: a second `/`, a `..`, a backslash. **A prefix the configuration does not declare is never a refusal, and no message ever tells you to edit `mustard.json` to make a branch acceptable.** `mustard-rt run work-unit-open --spec {slug} --type {kind} [--base {base}]` remains the manual face of the same engine (then `EnterWorktree path={path}`), and it is the one that takes an explicit `--base` when a hotfix has several candidates.
- **Abandoning an UNMERGED unit** — `/git delete <branch>`, run from OUTSIDE the unit. ONE gesture removes the unit whole (open PR, worktree, local branch, remote branch), and it refuses from inside a unit (`not-on-integration-base`), over a name that is nobody's unit or is protected (`not-a-work-unit`), and over a name no ref carries (`no-such-unit`). `/git finish` stays the ritual for MERGED units only.

## The notebook — the porta rule

Every unit turns up something true but off-topic: a defect three files away, a rename nobody asked for, a question the current Acceptance Criteria cannot answer. There are exactly two doors for it, and the choice is one line:

| What surfaced | Where it goes |
|---------------|---------------|
| It belongs to THIS spec (a criterion is wrong, a boundary moved) | **Amend the spec** — the amendment path (`ac-amend` for a frozen criterion). The unit's own contract changes, and the change is proved. |
| It does NOT belong to this spec | **The notebook** — `mustard-rt run notebook --add "one line"`. The unit's contract is untouched. |

**Per branch, never one global list.** The file lives at `.claude/spec/{slug}/notebook.md` — the same directory the spec, the waves and the ceremony are materialized into — so it travels with the unit, shows up in the PR diff, and disappears with the branch when `/git delete` retires it. Which work produced a pendency is information: it sets the item's priority and is the only thing that still makes it legible weeks later. A shared list loses that on the first append.

**Closing the loop.** Read it back with `mustard-rt run notebook` (no `--add`), optionally naming another unit with `--unit {kind}/{slug}` (or its old `{base}_{slug}` name). `/mustard:pr open` prints it once the PR is open: the work is in review, so the notebook is now the next cycle's prompt — it goes back to the base gate as the next request, and the loop closes. An empty notebook prints nothing; items are never invented to fill it.

## PRs are the integration path

A work branch reaches its base ONLY through a PR — never a local push to the base, and there is no `merge` action. Both `push` and `pr` **sync-first** (rebase onto `origin/<its base>`), so the branch never drifts from the latest base.

**Base→base PRs (promotion & backport).** `/mustard:pr open` run while ON a bare base `B` is the sole write-op allowed on a base — it opens a PR, never pushes to `B`:

- **Promotion** (up the flow): PR `B → flow[B]` (e.g. `dev → main`).
- **Backport** (against the flow): `/mustard:pr open <target>` → PR `B → <target>` (e.g. `main → dev` after a hotfix).

**Both go through the PR door, and `/git pr` is not a second one.** Publishing a pull request touches the provider, so it belongs to the door that owns the provider; `/git pr` was moved to `/mustard:pr open` when the two doors were split, and typing it now prints that one redirect line and stops. Nothing here opens a PR from the `/git` side.

Directions come from `git.flow` — no hardcoded pair. A terminal base (no `flow[B]`) needs an explicit `<target>`.

## Step 0 — resolve the base

```bash
rtk git rev-parse --abbrev-ref HEAD
```

Read `$BASE` off the branch — **from what was RECORDED, never from the kind**:

- the base **recorded with the unit** at the cut (`.claude/spec/{slug}/meta.json#base`, or `.claude/spec/{slug}/.cut-base` before the draft has folded it in) is the answer — it is a measurement of where the branch really came from. It is checked on the way out, and the check asks **whether that branch still exists on `origin`**, never whether `git.flow` lists it: a base that is gone cannot be cut from, and a base the configuration never named is still where the unit came from. An existence nobody could measure (no git, no remote, an unfetched clone) **obeys the record** — discarding a real answer over a silent probe is the same mistake as discarding it over a stale list;
- `{base}_…` (a unit in the older shape) → that prefix, longest match, against the pre-selected bases and then against the branches the repository really has;
- nothing recorded and `git.flow` leaves several candidates → the base is **not derivable**, and the harness says so instead of guessing (see below);
- nobody's unit (a Desktop branch, a hand-cut branch, no `mustard.json`) → the primary base (`git.flow["*"]`, else `rtk git symbolic-ref refs/remotes/origin/HEAD`).

There is no "the kind implies the base" row any more, in either direction.

**The operator's pick is DURABLE.** Every unit's base is a choice now — the branch name says what the unit IS and cannot carry where it came from — so a unit records it whenever there was a choice to make, and *was there a choice?* has TWO sources: the repository really carries more than one branch, or `git.flow` leaves the derivation more than one candidate. Either is enough. The CATALOGUE half is the one a declaration cannot see — the picker offers every branch `origin` has, so a project declaring a single base still had the whole list to choose from, and counting the declaration alone would drop that pick before anything could read it back. The answer rides to the cut in the session's pending marker, which is **consumed** there, so the cut writes it into the unit's own directory before clearing it — as `.cut-base`, harness state the draft tolerates, and the draft then folds it into `meta.json#base` (the sidecar that already holds every machine-parseable fact about the unit) and retires the file. The cut may NOT write that sidecar itself: a `meta.json` in the unit's directory is what tells `spec-draft` a spec is already drafted there, so recording the base that way left the unit cut and spec-less. Both doors record it (the hook gate and the cut `spec-draft` takes at approval), so the answer does not depend on which one opened the unit; a reconcile that rewrites the marker to another branch keeps the base line, because what it learned is a branch and not a base. With **nothing recorded and several declared candidates**, the base is *not derivable*: `work-unit-open` refuses with `ambiguous-base` and asks for `--base`, and the `WorktreeCreate` hook announces on stderr which base it fell back to — instead of quietly answering the outermost and aiming the unit's pull request somewhere nobody chose. The one project that records nothing is the one where NEITHER source offers a choice — a single declared base and a single branch to cut from — because there was never an answer to lose.

## Step 0b — branch protection

Before any write op (commit, push, sync): if the current branch is one of the **protected** branches — `origin/HEAD` ∪ `mustard.json#git.protected`, the measured set from the top of this file — → **REFUSE**, naming the branch that was refused and saying a work branch has to be cut first. Anything else proceeds, including a branch `git.flow` happens to name: appearing in a promotion map is not a reason to be protected, and a project that promotes through `dev` may commit on it. **Exception:** `/mustard:pr open` on a base opens a base→base PR (above) and is allowed.

**This file states the RULE; it does not quote the message.** It used to carry a refusal sentence in backticks, spelled as if the program emitted it verbatim. It does not — that string existed in exactly two places, this file and the test that read it back out of this file, which made the ratchet circular: no change to the binary could ever have failed it. The refusal is composed by whichever gate takes it (`work_branch_gate` on an edit; this step on a `/git` write op) and its wording is free to change. What may not change is the SET that is measured and the fact that the refusal names the branch it stopped. The choice recorded here is to drop the citation rather than teach the binary a sentence invented to justify one: a message that exists only to make a quotation true is a worse thing to own than a quotation removed.

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
