---
description: Use when the user runs /pr or asks to see the open pull requests, review one, or merge one. The PR door — list, review, merge; the merge runs the verification gates, then prunes the unit and returns to the base.
argument-hint: <list|review|merge> [<pr-number>] [--confirm]
source: manual
disable-model-invocation: true
---
<!-- mustard:generated -->
# /pr — The Pull Request Door

**Iron law: a merge is never silent.** Merging a unit whose review did not come back `approved` is allowed — the operator decides case by case — but it is always ASKED about first, never done quietly and never refused outright.

`/pr <action> [<pr-number>] [--confirm]`

**Review, QA and close are STEPS here, not doors.** None of them is ever what the operator set out to do — they are what has to happen on the way to a merge, and they were commands only by inheritance. `review` is the second action below; QA and CLOSE are the gate the third action crosses before it touches the provider. Inside a spec's own wave loop the same gates already run deterministically (`wave-advance`'s review round, then `close-pipeline`) — this door is where they come due for a unit that reaches its base.

## Actions

| Action | Description |
|--------|-------------|
| `list` | Every open PR of the base you are standing on: number, title, the provider's mergeable word, whether it is a draft, and the head branch its unit lives on. **Runs only from an integration base** (`git.flow`) — from a work branch it refuses and names the base to switch to, because "which PRs are open" is a question about the base, not about one unit. |
| `review [<pr>]` | Review ONE pull request against its own spec and the project's molds. Resolves the PR to its work unit (`{base}_{slug}` → the spec slug), prints the brief — spec path, subproject, that subproject's skill shelf — then runs the review and **records the verdict**. The merge step reads exactly this record. |
| `merge [<pr>] [--confirm]` | Cross the verification gate (build + tests, QA, review spans, docs), then merge and prune: back to the base, pull it, remove the worktree, delete the local and remote branch. No `approved` verdict recorded → it **warns and asks**, touches nothing, and waits for your answer. `--confirm` is that answer coming back. |

## Iron rules

- **`rtk` prefixes every `git` and every `gh`** — inside `&&`/`;` chains and `$(…)` substitutions too.
- **Print each JSON verbatim.** Every step below answers with one JSON document; relay it, do not paraphrase it away.
- **Never invent a verdict.** Record `rejected` honestly when the findings are blocking. Recording `approved` to unblock a merge is the one failure this door cannot detect.
- **QA is read-only and never inferred.** A pass is an OBSERVED exit code; fixing code mid-QA invalidates the result. Max 3 iterations.
- **The merge step is the only one that writes to the base.** `list` and `review` touch nothing.
- **Submodules before parent, always** — the exit ritual is per repo, exactly as `/mustard:git pr close` describes it. → `${CLAUDE_PLUGIN_ROOT}/refs/git/submodule-rules.md`
- **Cancelling an abandoned unit is not a merge and not a close** — it is `/mustard:git delete <branch>`, from the base. One gesture removes the branch, its remote and its open PR, and everything the unit produced lived on that branch.

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

Then fetch the diff and the phase context:

```bash
mustard-rt run review-prefetch <n> --format json
mustard-rt run diff-context --phase execute --subproject {sub}
```

`review-prefetch` returns `title`/`body`/`author`/`base`/`head`/`additions`/`deletions`/`changedFiles`/`files[]`/`comments[]`/`reviews[]` — source of truth, do NOT re-fetch. Fallback: `gh pr view --json …` + `gh pr diff`.

Bracket the read with the two review events, so the resume gate and the metrics see the same window:

```bash
mustard-rt run emit-event --event review.start --spec "$MUSTARD_SPEC" --payload "spec=$MUSTARD_SPEC" --payload "target=$PR_TARGET"
```

Paste the diff as a `## DIFF` block → `Skill({ skill: "code-review", args: "<n>" })`. Fallback (skill unavailable): `Task(general-purpose)` with the DIFF as source of truth, reading source only when ambiguous. Checklist: SOLID, Security, Performance, Patterns, Integration. Then:

```bash
mustard-rt run emit-event --event review.complete --spec "$MUSTARD_SPEC" --payload "spec=$MUSTARD_SPEC" --payload "target=$PR_TARGET"
```

Record the outcome — this is what step 3 reads:

```bash
mustard-rt run pr-review --pr <n> --verdict <approved|rejected> --critical <N>
```

`<N>` = count of critical findings (0 when `approved`). **Two records, two readers, and they are not interchangeable:** `pr-review` records the PR-scoped verdict the merge step reads; a review dispatched INSIDE the wave loop records the spec-scoped one with `mustard-rt run review-result --spec {spec} --verdict … --subproject {sub}`, which is what `resume-bootstrap` advances past `ReviewPending`. A unit that never left the loop already carries the second; this door adds the first.

**Tactical-fix discovery (detect + propose, never auto-create).** Scan the return for `## Tactical Fix Candidates` / `## Candidatos a Tactical Fix`; per entry print *"Tactical fix candidate: <desc>\nRun: /mustard:tactical-fix <parent> \"<desc>\""*. It never blocks an APPROVED; a REJECTED still routes through the normal fix-loop (`${CLAUDE_PLUGIN_ROOT}/refs/spec/resume-loop.md § Fix Loop`). Qualification → `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Tactical Fix Discovery`. Include a `tactical_fix_candidates` array in the recorded payload (each `{description (required), scope?, severity?}`) so `mustard-rt run tactical-fix-detect --spec <spec>` proposes each deterministically — one idempotent `tactical_fix.proposed` event per candidate; it never scaffolds, because creation stays a one-confirmation step.

### 3. `merge <pr>` — verify, merge, prune

**3a. The verification gate (skip only when the spec already reads `completed`).** A unit carrying a spec reaches its base through this chain, run while the unit is still live on its branch — merging first integrates unverified work:

```bash
mustard-rt run close-orchestrate --spec {spec}
```

One command runs every gate and, on pass, finalizes in-process. Gates: (1) **build + tests** `verify-pipeline`; (2) **QA** `qa-run` — only a recorded `overall=pass` opens the close; (3) **review-spans** (any red span → block); (4) **docs audit** `docs-stale-check` (`--skip-docs` for a non-architectural spec); (5) **pipeline-summary** (advisory). It derives `overall`.

**The finalize is automatic — you never decide whether to call `complete-spec`.** On `overall == "pass"` the spec flips to `completed`, `pipeline.complete` is emitted and auto-verified, and `meta.json` is stamped (`"chained": true`, `"verified": true|false`). On `overall == "fail"` it is report-only (`"chained": false`) — fix the failing gate and re-run. NEVER hand-call `complete-spec` to bypass a red gate: when the red gate is QA it refuses on its own with exit 2, reading the same `qa.result overall=pass` the close gate requires, and no environment switch relaxes that. A red `review-spans` or `docs-stale-check` is NOT read by that refusal — those block through `close-orchestrate`'s own gate vector, so hand-calling past them is on you.

Preconditions this chain does not cover, checked before it runs: an unresolved `BLOCKED` blocks; `CONCERN`/`DEFERRED` surface and proceed; any unchecked `- [ ]` in the Checklist ABORTS with the unmarked items listed. Epic auto-fold is handled in-process (children all closed → folded) — nothing to run by hand.

**Reading the QA half of that report.** `qa-run` executes each `AC-N` carrying a `Command:` in the operative AC file (`spec.md`, or `wave-plan.md` after a decompose) and emits `qa.result`; the close gate reads the record, never a summary.

- **`pass`** → the chain continues into the finalize.
- **`fail`** → list the failing AC. After 3 failures → `AskUserQuestion`: (a) fix + retry, (b) relax the AC through `ac-amend`, (c) abort.
- **`skip`** → a skip is not a verification, so it blocks the close exactly like a fail. Two shapes, told apart by `criteria` in the result. **No AC at all** (`criteria` empty) → the spec has nothing to verify, so it has nothing to claim: author a criterion (one reproduction command, red before the work and green after) and re-run. **ACs exist but every one skipped** (per-AC timeout 120s, spawn failure, or a self-invoked run that cannot rebuild the binary its criteria target) → fix the AC commands (raise the timeout, split the AC) and re-run, or record the verdict from an EXTERNAL `mustard-rt run qa-run --spec {spec}` — that is the run that can actually attempt them.

Env: `MUSTARD_QA_GATE_MODE=strict|warn|off`. Any `spec.md`/`wave-plan.md` edit after a pass marks QA STALE — the gate blocks until it is re-run. If `mustard-rt` is unavailable, dispatch `Task(general-purpose)` with `${CLAUDE_PLUGIN_ROOT}/context/qa/qa.core.md`.

**3b. The merge itself.**

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

`pr-qa-gate` warns separately at `gh pr create`/`merge` time — that warning and gate 3a read the same recorded `qa.result`.

**3c. After the merge — record what the unit taught (max 3 each, skip the trivial; durable prose belongs to native auto-memory).**

```bash
mustard-rt run emit-event --event decision --spec {spec} --payload "title=…" --payload "rationale=…"
mustard-rt run emit-event --event lesson --spec {spec} --payload "takeaway=…" --payload "trigger=…"
mustard-rt run capability create --slug {slug} --title "…"
```

The capability line is for a spec that shipped a durable user-facing capability — then link `[[cap.{slug}]]` in the spec.

## Inviolable

- NEVER pass a branch name where a PR number belongs.
- NEVER re-run `pr-merge --confirm` without having actually asked.
- NEVER modify code during QA, and never run QA before EXECUTE completes.
- NEVER move a spec directory — archival is event-only.
- NEVER batch-mark Checklist items on behalf of agents.
- Budget: ≤1 Bash per step, ≤1 Skill/Task call per review.
