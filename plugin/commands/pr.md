---
description: Use when the user runs /pr or asks to see the open pull requests, review one, or merge one. The PR door — list, review, merge; the merge also prunes the unit and returns to the base.
argument-hint: <list|review|merge> [<pr-number>] [--confirm]
source: manual
disable-model-invocation: true
---
<!-- mustard:generated -->
# /pr — The Pull Request Door

**Iron law: a merge is never silent.** Merging a unit whose review did not come back `approved` is allowed — the operator decides case by case — but it is always ASKED about first, never done quietly and never refused outright.

`/pr <action> [<pr-number>] [--confirm]`

## Actions

| Action | Description |
|--------|-------------|
| `list` | Every open PR of the base you are standing on: number, title, the provider's mergeable word, whether it is a draft, and the head branch its unit lives on. **Runs only from an integration base** (`git.flow`) — from a work branch it refuses and names the base to switch to, because "which PRs are open" is a question about the base, not about one unit. |
| `review [<pr>]` | Review ONE pull request against its own spec and the project's molds. Resolves the PR to its work unit (`{base}_{slug}` → the spec slug), prints the brief — spec path, subproject, that subproject's skill shelf — then runs the review and **records the verdict**. The merge step reads exactly this record. |
| `merge [<pr>] [--confirm]` | Merge the PR, then prune: back to the base, pull it, remove the worktree, delete the local and remote branch. No `approved` verdict recorded → it **warns and asks**, touches nothing, and waits for your answer. `--confirm` is that answer coming back. |

## Iron rules

- **`rtk` prefixes every `git` and every `gh`** — inside `&&`/`;` chains and `$(…)` substitutions too.
- **Print each JSON verbatim.** Every step below answers with one JSON document; relay it, do not paraphrase it away.
- **Never invent the verdict.** Record `rejected` honestly when the findings are blocking. Recording `approved` to unblock a merge is the one failure this door cannot detect.
- **The merge step is the only one that writes to the base.** `list` and `review` touch nothing.
- **Submodules before parent, always** — the exit ritual is per repo, exactly as `/mustard:git pr close` describes it. → `${CLAUDE_PLUGIN_ROOT}/refs/git/submodule-rules.md`

## Procedure

### 1. `list` — what is waiting

```bash
mustard-rt run pr-list
```

Read `ok` first.

- `ok:false` with `reason:"not-on-integration-base"` → the `hint` names the base. Print it and stop; do not list anything.
- `ok:true` → print one line per entry of `prs`: `#<number> <title> — <mergeable> <head>`. A `draft:true` row cannot be merged yet (the parent of a monorepo unit opens as a draft while any submodule PR is still open). `gh_error` present → say the provider did not answer; the checkout was fine.

### 2. `review <pr>` — read it against its own spec

```bash
mustard-rt run pr-review --pr <n>
```

The brief comes back with `spec`, `spec_path`, `subproject` and `patterns` — the skill shelf the implementer was dispatched with, so the review measures the work against the very molds it was written to. `spec: null` means the head branch carries no `{base}_` unit; review it as a plain diff.

Then fetch and review:

```bash
mustard-rt run review-prefetch <n> --format json
```

Paste the diff as a `## DIFF` block → `Skill({ skill: "code-review", args: "<n>" })`. Fallback (skill unavailable): `Task(general-purpose)` with the DIFF as source of truth. Checklist: SOLID, Security, Performance, Patterns, Integration.

Record the outcome — this is what step 3 reads:

```bash
mustard-rt run pr-review --pr <n> --verdict <approved|rejected> --critical <N>
```

`<N>` = count of critical findings (0 when `approved`).

### 3. `merge <pr>` — merge, prune, return

```bash
mustard-rt run pr-merge --pr <n>
```

Three answers, told apart by `action`:

- **`confirm`** — `ok` is still true and NOTHING was touched. The `warning` says why (no verdict recorded, or the last one was not `approved`). Put the question to the operator with `AskUserQuestion`; on a yes, re-run with `--confirm`. Never treat this as a failure and never merge past it on your own.
- **`merged`** — merged, then settled. The folded `settle` document is `git-settle`'s own report: `repos` carries one entry per repository of the unit, `complete:false` means one is still unsettled, and `alsoMergeable` lists other units awaiting their own merge. Print it verbatim.
- **`merge-failed`** — the provider refused (conflicts, draft state, required checks). Nothing was pruned; the unit is untouched.

```bash
mustard-rt run pr-merge --pr <n> --confirm
```

QA precedes integration: a unit carrying a spec should reach its base only after a passing `qa.result` (`mustard-rt run qa-run --spec <slug>`). The merge step warns about the review; the QA coupling warns separately at `gh pr merge` time.

## Inviolable

- NEVER pass a branch name where a PR number belongs.
- NEVER re-run `pr-merge --confirm` without having actually asked.
- Budget: ≤1 Bash per step, ≤1 Skill/Task call per review.
