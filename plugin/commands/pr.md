---
description: Use when the user runs /pr or asks to open a pull request, see the open ones, review one, or merge one. The PR door — open, list, review, merge; the merge runs the verification gates, then prunes the unit and returns to the base.
argument-hint: <open|list|review|merge> [<pr-number>] [--confirm]
source: manual
disable-model-invocation: true
---
<!-- mustard:generated -->
# /pr — The Pull Request Door

**Iron law: a merge is never silent.** Merging a unit whose review did not come back `approved` is allowed — the operator decides case by case — but it is always ASKED about first, never done quietly and never refused outright.

`/pr <action> [<pr-number>] [--confirm]`

**This door owns the pull request on the provider, and the gates that can refuse work.** That is the line against `${CLAUDE_PLUGIN_ROOT}/commands/git.md`, which moves bits in your tree and decides nothing. `open` moved here from `/git pr` for exactly that reason: two doors that both created pull requests read as duplicates of each other, and the one that could not refuse anything was the wrong home.

**Review, QA and close are STEPS here, not doors.** None of them is ever what the operator set out to do — they are what has to happen on the way to a merge, and they were commands only by inheritance. `review` is the second action below; QA and CLOSE are the gate the third action crosses before it touches the provider. Inside a spec's own wave loop the same gates already run deterministically (`wave-advance`'s review round, then `close-pipeline`) — this door is where they come due for a unit that reaches its base.

## Actions

| Action | Description |
|--------|-------------|
| `open [<target>]` | Write `<spec>/pr-body.md` (mandatory — see the procedure), then open/update the pull request with it (idempotent) — **one per repo, submodules before parent**. While ANY submodule PR is still open the parent opens `--draft` with a `Blocked by <sub PR url>` body line (GitHub refuses to merge a draft — that is the mechanical half of the order). Work stays live on the branch; each `push`/`open` updates the SAME PR until it merges. Work branch → the base its kind implies (`feature/`/`fix/` → the `*` base, `hotfix/` → the base that is not it; an older `{base}_` name → its prefix); bare base `B` → `<target>` or `flow[B]` (promote `dev→main` / backport `main→dev`). Was `/mustard:git pr` until the doors were split by what they touch. |
| `list` | Every open PR of the base you are standing on: number, title, the provider's mergeable word, whether it is a draft, and the head branch its unit lives on. **Runs only from a base, not from a unit** — from a work branch it refuses and names where to switch to, because "which PRs are open" is a question about the base, not about one unit. |
| `review [<pr>]` | Review ONE pull request against its own spec and the project's molds. Resolves the PR to its work unit (`{kind}/{slug}`, or an older `{base}_{slug}`, → the spec slug), prints the brief — spec path, subproject, that subproject's skill shelf — then runs the review and **records the verdict**. The merge step reads exactly this record. |
| `merge [<pr>] [--confirm]` | Cross the verification gate (build + tests, QA, review spans, docs), then merge and prune: back to the base, pull it, remove the worktree, delete the local and remote branch. No `approved` verdict recorded → it **warns and asks**, touches nothing, and waits for your answer. `--confirm` is that answer coming back. |

## Iron rules

- **`rtk` prefixes every `git` and every `gh`** — inside `&&`/`;` chains and `$(…)` substitutions too.
- **Print each JSON verbatim.** Every step below answers with one JSON document; relay it, do not paraphrase it away.
- **Never invent a verdict.** Record `rejected` honestly when the findings are blocking. Recording `approved` to unblock a merge is the one failure this door cannot detect.
- **QA is read-only and never inferred.** A pass is an OBSERVED exit code; fixing code mid-QA invalidates the result. Max 3 iterations.
- **The merge step is the only one that writes to the base.** `list` and `review` touch nothing.
- **Submodules before parent, always** — for `open` AND for the prune, exactly as `/mustard:git finish` describes it. → `${CLAUDE_PLUGIN_ROOT}/refs/git/submodule-rules.md`
- **Cancelling an abandoned unit is not a merge and not a close** — it is `/mustard:git delete <branch>`, from the base. One gesture removes the branch, its remote and its open PR, and everything the unit produced lived on that branch.

## Procedure

### 0. `open [<target>]` — publish it

**First, the PR body — and it is not optional.** A pull request whose description is a commit list makes the reviewer reconstruct the reasoning you already did, and the unit's own record is where that reasoning lives. Write `<spec>/pr-body.md` BEFORE opening, and pass it with `--body-file`. It is committed with the spec, so the explanation travels with the unit instead of living only on the provider.

**It is rewritten on every update, and BOTH halves are pushed.** The body is correct for exactly the commits it was written against; every later `push` re-targets the SAME pull request, so a body written once drifts behind the diff it describes — and a reviewer believes a wrong explanation more readily than a missing one. So whenever the unit gains work: rewrite the file, commit it, AND send it to the provider with `mustard-rt run pr-edit --number <n> --body-file <spec>/pr-body.md`. **Updating the file alone changes nothing for the person reading the PR** — that is the half that is easy to forget and the only half the reviewer ever sees. `pr_body_gate` measures it (the file's mtime against `.git/HEAD`) and warns on the push, so a stale body is caught where it happens rather than at review.

Compose it from what the unit already recorded — never re-derive and never invent: `## Context` and `## Decisions` (with their reasons) from `spec.md`, the criteria and their commands from `## Acceptance Criteria`, the non-obvious calls from each wave's report, and what was deliberately left out from `## Non-Goals`. Sections, in order:

| Section | What goes in |
|---|---|
| Opening | ONE paragraph: what the reader gets that they did not have. No preamble. |
| Why | the concrete situation that forced it — the case, not the abstraction |
| What changed | before/after, and a **mermaid** diagram when the change is structural (the provider renders it; ASCII art does not survive their markdown) |
| How to validate | commands the reader RUNS, in a throwaway directory, that touch nothing of theirs |
| Tests | one row per criterion: what it guarantees + its command. State that each was proven RED before the code existed |
| Decisions worth explaining | the non-obvious calls and the reason each one is not the obvious alternative |
| Out of scope | what was deliberately left out, each with its reason — this is what stops a reviewer filing what you already decided |

Two rules that keep it honest. **Every number is measured, never estimated** — a test count comes from the run, not from memory. **Name what is still open**, including work deliberately not done: a reviewer who finds an omission you did not declare stops trusting the rest of the document.

Then publish. Work branch: `/mustard:git push` first, then one PR per repo (submodules first) into each prefix base; do NOT return to base. **While ANY submodule PR is still open the parent opens as a DRAFT**: `mustard-rt run pr-open --base "$BASE" --head <parent-work-branch> --body-file <spec>/pr-body.md --draft` (plus a `Blocked by <sub PR url>` line appended to that body). The provider refuses to merge a draft PR, which is what turns "submodules before parent" from a sentence into a block — the order governed only PR OPENING, and on GitHub the two PRs are siblings anyone can merge in either direction. A draft ALSO does not request review from code owners (CODEOWNERS); those requests fire at `mustard-rt run pr-ready`, which runs in `/mustard:git finish` after the bump lands — so expect no reviewers until then. Every submodule PR already merged → open the parent normally (`--body-file`, no `--draft`). Bare base `B`: no push → `mustard-rt run pr-open --base <target|flow[B]> --head "$B" --body-file <spec>/pr-body.md`. Existing PR in any repo → rewrite the body and `mustard-rt run pr-edit --number <n> --body-file <spec>/pr-body.md`, then print its URL. Each command answers ONE JSON report (`ok`/`provider`/`number`/`url`) — print it verbatim; the provider it speaks to is its internal detail, never typed here.

**Then close the loop: read the unit's notebook** (`mustard-rt run notebook`) and print its items under the PR URL — the work is now in review, so what the notebook holds is the next cycle's prompt, carried back to the base gate as the next request. An empty notebook prints nothing; do not invent items for it.

This is the one action here that crosses NO gate: publishing is not integrating. The gates come due at `merge`.

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

The brief comes back with `spec`, `spec_path`, `subproject` and `patterns` — the skill shelf the implementer was dispatched with, so the review measures the work against the very molds it was written to. `spec: null` means the head branch names no unit of this project — neither a `{kind}/` one nor a declared `{base}_` prefix; review it as a plain diff.

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

One command runs every gate and, on pass, finalizes in-process. Gates: (1) **build + tests** `verify-pipeline`; (2) **QA** `qa-run` — only a recorded `overall=pass` opens the close; (3) **review-spans** (any red span → block); (4) **docs audit** `docs-stale-check` (`--skip-docs` for a non-architectural spec); (5) **close gates** — the same sub-gates `emit-phase --to CLOSE` runs (debt markers, checklist, **findings**, QA, build), so this door and that one refuse the same trees; the refusal text arrives in the gate's `summary`; (6) **pipeline-summary** (advisory). It derives `overall`.

**The finalize is automatic — you never decide whether to call `complete-spec`.** On `overall == "pass"` the spec flips to `completed`, `pipeline.complete` is emitted and auto-verified, and `meta.json` is stamped (`"chained": true`, `"verified": true|false`). On `overall == "fail"` it is report-only (`"chained": false`) — fix the failing gate and re-run. NEVER hand-call `complete-spec` to bypass a red gate: when the red gate is QA it refuses on its own with exit 2, reading the same `qa.result overall=pass` the close gate requires, and no environment switch relaxes that. A red `review-spans` or `docs-stale-check` is NOT read by that refusal — those block through `close-orchestrate`'s own gate vector, so hand-calling past them is on you.

Preconditions checked before it runs: an unresolved `BLOCKED` blocks; `CONCERN`/`DEFERRED` surface and proceed; any unchecked `- [ ]` in the Checklist ABORTS with the unmarked items listed — that one is now ALSO a gate inside (5), so a checklist item you meant to let go is settled with `mark-checklist-item --drop --reason`, not left unmarked. Epic auto-fold is handled in-process (children all closed → folded) — nothing to run by hand.

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

- **`confirm`** — `ok` is still true and NOTHING was touched. **Read `reason` before you do anything: the five causes do not share one answer, and `--confirm` is the right move for only some of them.** Two are about the RECORDED VERDICT — `no-review-verdict`, `verdict-not-approved` — and there the report's own `hint` is right: put the question to the operator, and on a yes re-run with `--confirm`. Three are about what the PROVIDER's own runs say, and they arrived with this door's `checks` field: **`provider-checks-running` is a WAIT, not a question** — the runs are still going, so `--confirm` merges past the only thing that could still stop the work, and offering it as the first move is how a validation ends up finishing after the merge it was supposed to gate (measured on this repository, 2026-08-29: PR 237 merged two minutes after opening, its run still going three minutes later). Wait, re-run `pr merge`, and raise `--confirm` only if the operator asks to go without it. `provider-checks-failed` is a FIX — say what came back red; a cancelled run reduces here too, so never report it to the operator as a failure without looking. `provider-checks-unreadable` means the provider did not answer at all: nothing is running, so waiting changes nothing — check its tooling, then re-run. Never treat any of the five as a failure, and never merge past one on your own.
- **`merged`** — merged, then settled. The folded `settle` document is `git-settle`'s own report: `repos` carries one entry per repository of the unit, `complete:false` means one is still unsettled, and `alsoMergeable` lists other units awaiting their own merge. Print it verbatim.
- **`merge-failed`** — the provider refused (conflicts, draft state, required checks). Nothing was pruned; the unit is untouched.

```bash
mustard-rt run pr-merge --pr <n> --confirm
```

`pr-qa-gate` warns separately at PR create/merge time — that warning and gate 3a read the same recorded `qa.result`.

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
