# /mustard:spec — Resume flow (continue pipeline)

Loaded on demand by SKILL Step 5 when `stage=Execute` (or `Analyze`/`QaReview`/`ReviewPending`/`QaPending`/`Close`). All mode decisions (`continued` vs `reanalyzed`), operational spec resolution, stub detection, `needsDiff`/`needsContextSlice`, `lastDispatchFailure` parsing, **and the post-execute REVIEW/QA decision**, have been moved to `mustard-rt run resume-bootstrap --spec X --json`. Literal agent prompt construction was moved to `mustard-rt run agent-prompt-render`; wave routing + prompts arrive **already rendered** via `mustard-rt run wave-advance` — including the post-impl **review round** (see below). This ref only keeps what the binary cannot decide on its own.

## Stage values post-execute (never freelance)

The binary can return three extra `stage` values when all waves complete. The orchestrator NEVER decides on its own — always dispatch what `nextAction` indicates:

| `stage` | `nextAction` | Companion field | What to do |
|---------|--------------|-----------------|------------|
| `ReviewPending` | `dispatch-review` | `reviewRoles: [...]` | Fallback only (resumed session / missing or rejected verdict): dispatch one REVIEW Task per role. Inside the wave-advance loop the review round already arrives rendered — prefer that path |
| `QaPending` | `run-qa` | `qaCommand: "..."` | Run `mustard-rt run close-pipeline --spec {specName}` (it chains `qa-run`) — not the manual sequence |
| `Close` | `emit-complete` | — | Run `mustard-rt run close-pipeline --spec {specName}` — the close happens only when it returns `completed: true` |

When `nextAction` is `null`, there is still a wave to run — follow the normal wave-dispatch flow below.

`close-pipeline` composes the whole CLOSE tail in **one call**: review.result verdicts (advisory) + `qa-run` + — only on QA pass — `complete-spec` + `pipeline-summary`, returning `{reviews, qa, completed, summary}`. On QA fail/skip it returns `completed: false` and does NOT close — report the failing AC instead of retrying the close or calling `complete-spec`/`pipeline-summary` by hand.

## Hard gate on `emit-pipeline --kind pipeline.complete`

As of 2026-05-25 the binary refuses to emit `pipeline.complete` without a `qa.result` (overall=pass) in the spec's ndjson — exit 2 + message `BLOCKED: …`. The escape hatch `--allow-no-qa` exists only for `qa-run` itself and explicit user overrides. Do not try to work around it.

## Step 12c — Wave Plan Scope (conditional, only if `isWavePlan === true`)

When the bootstrap JSON indicates a wave plan, the orchestrator dispatches the current **dispatch level** — every wave that shares the lowest not-yet-completed `level` — in one message, never the entire spec and never a single wave when several waves share that level.

### Routing is decided by Rust — the orchestrator is a relay

The wave **order, routing and prompts** are not interpreted by the LLM. Run:

```bash
mustard-rt run wave-advance --spec {specName}
```

It reads the wave DAG and returns the **current round only** — every wave of the first dependency level whose waves lack `pipeline.wave.complete` — as a deterministic JSON array. Once every impl wave is complete it returns the **review round** (next section) instead of `[]`; `[]` comes only after that round is covered too. Each item is:

```json
{ "wave": 2, "role": "cli", "subproject": "apps/cli", "subagent_type": "general-purpose",
  "prompt": "…the FULL Task prompt, ALREADY RENDERED…" }
```

- Items returned together share one dependency level → dispatch them **together in one message** (several `<invoke>` blocks). Never reach for a later level by hand — re-run `wave-advance` after the round completes and it advances on its own.
- **`prompt`** IS the final Task prompt — already rendered by `agent-prompt-render` inside the binary. There is no `prompt_cmd` round-trip and nothing to assemble; pass it **verbatim** as the Task `prompt` — it is a 2-line `MUSTARD-PROMPT-REF` stub the PreToolUse hook expands at dispatch; NEVER read the `.dispatch/` file in the parent (that pays the full prompt back into your context).
- **`subagent_type`**: each item carries its own — the tool picks the agent per role (read-only roles run tool-restricted: `explore`→`Explore`, `review`/`qa`→`mustard-review`, `guards`→`mustard-guards`; writing roles → `general-purpose`). Pass it through; never pick by hand.

The orchestrator does NOT decide the order, group rounds, or assemble the loop by hand — `wave-advance` owns that ("free section" determinised). `resume-bootstrap` stays the **stage** decision (mode / stage / progress); `wave-advance` is the **wave-routing + render** decision. (`dispatch-plan` still exists — use it only to **inspect** the full DAG/levels, e.g. when debugging routing; it is not the dispatch path. Do NOT drive the loop off the bootstrap's scalar `currentWave`: it names one wave, but a round can hold several independent waves of the same level.)

### Review round — REVIEW enters the same loop

Once EVERY impl wave carries `pipeline.wave.complete`, `wave-advance` does not return `[]` yet: it returns the **review round** — one item per **distinct subproject touched by the plan's waves**, in alphabetical order, each with its prompt already rendered:

```json
{ "wave": 0, "role": "review", "subproject": "apps/cli", "subagent_type": "mustard-review",
  "prompt": "…the FULL Task prompt, ALREADY RENDERED…" }
```

Dispatch these exactly like any other round — `prompt` verbatim, the item's `subagent_type`, all in ONE message. What stays with the **orchestrator** is recording each verdict after the review returns:

```bash
mustard-rt run review-result --spec {specName} --verdict approved|rejected [--critical N] --subproject {subproject}
```

The `review.result` event is the "already reviewed" signal: re-running `wave-advance` re-emits only the subprojects still lacking one (dedup by the event payload's `subproject`; an absent/null/empty payload subproject counts as `"."` — a whole-project review). Once every touched subproject carries a verdict, `wave-advance` returns `[]` (terminal).

### Per-level loop

The **round** is exactly the array one `wave-advance` call returns — impl waves and the review round alike. Process one round at a time — never one wave at a time when the round holds several.

1. **Dispatch the whole round in ONE message.** For each item in the round: check its `precheck` field (Step 12d — `wave-advance` already computed it inline; impl waves only, review-round items carry none), then dispatch a Task with the item's `prompt` (verbatim) and the item's `subagent_type`. ALL `<invoke>` blocks go in a single message so the agents run concurrently.
2. **After each impl wave N in the round returns:**
   - Commit `/mustard:git commit` style with message `feat(wave-{N}/{role}): {summary}`. Fallback: `git add {files} && git commit -m "..."`.
   - Finalize the wave in ONE call: `mustard-rt run wave-done --spec {specName} --wave {N} --duration-ms {elapsed}`. It emits `pipeline.wave.complete` (the projection derives `completedWaves` from it — no JSON state file) AND caches this wave's diff stat into `wave-{N}-{role}/diff.md` for the next round's render — an **atomic LF write**, so there is no shell redirect and none of the relative-vs-absolute / CRLF footgun the raw `> diff.md` had. The render inside `wave-advance` injects the cached file on the next round; you pass nothing explicitly.
3. **After each review item in the round returns:** record the verdict — `mustard-rt run review-result --spec {specName} --verdict approved|rejected [--critical N] --subproject {subproject}`. Emitting `review.result` per subproject stays the orchestrator's responsibility (the review agent does not emit it); without it the next `wave-advance` re-emits the same item. No commit, no `pipeline.wave.complete`, no diff cache for review items. REJECTED (any CRITICAL) → Fix Loop Protocol (`../resume/fix-loop-wave.md`) before moving on.
4. **After the round completes**, run `mustard-rt run wave-tree --spec-dir .claude/spec/{specName}` to show progress, then re-run `wave-advance` — it returns the next round (the review round once every impl wave is complete).
5. When `wave-advance` returns `[]` (every impl wave complete **and** every touched subproject reviewed) → do **NOT** emit `pipeline.complete`. Re-run `resume-bootstrap` and follow `nextAction` — with the verdicts already recorded it normally goes straight to `close-pipeline` (QA + CLOSE); `ReviewPending` reappears only when a verdict is missing or rejected.
6. If a wave fails (REJECTED after 2 fix-loops, or BLOCKED) → see Escalation Statuses + `../resume/fix-loop-wave.md`. A failed wave blocks the higher levels that depend on it; independent waves in the same round still complete.

## Step 12d — Dependency Precheck (factual gate)

`wave-advance` runs the precheck **inline** and annotates each impl-wave item with a `precheck` field — you do NOT call `dependency-precheck` per wave any more; the round you already hold carries the verdict. For each impl item, read `item.precheck`:

- `{ "ok": true }` (or absent — review-round items) → dispatch normally.
- `{ "ok": false, "missing": [...], "suggested_tactical_fix_files": [...] }`:
  1. Print inline: `BLOCKED — N missing symbols: {missing.symbol}. Suggestion: create tactical-fix.`
  2. Emit `mustard-rt run emit-pipeline --kind pipeline.dispatch_failure --spec {specName} --payload '...'`.
  3. AskUserQuestion: **Create tactical-fix automatically** / **Investigate manually** / **Force dispatch (override)**.

**Skip acting on `precheck` when `resume-bootstrap` returned `mode: continued`** (a continued resume already cleared these) or env `MUSTARD_DEPENDENCY_PRECHECK_MODE=off` (the annotation's `ok` is forced `true` anyway). `dependency-precheck` stays a standalone command for ad-hoc/debug runs; the wave loop just reads the annotation instead of re-invoking it per wave.

## Escalation Statuses

After each agent returns, check the return value before advancing:

| Status | Handling |
|--------|----------|
| Internal error | Re-dispatch sequentially, max 1 retry. Still failing → STOP + report |
| `CONCERN` | Record verbatim under `## Concerns`; continue. ≥2 → surface together before advancing |
| `BLOCKED` | Stop; AskUserQuestion with the exact blocker; do NOT advance |
| `PARTIAL` | Granular Retry Protocol; do NOT restart |
| `DEFERRED` | Note in the spec; ask if load-bearing before CLOSE |
| REJECTED (after REVIEW) | Fix Loop Protocol (max 2 loops); 2 fails → STOP |
| Wave failure | Update `failedWaves`, write `failure.md`, AskUserQuestion |

See `.claude/pipeline-config.md § Escalation Statuses` and `../resume/fix-loop-wave.md` for details.

## INVIOLABLE RULES

- Main context **IS** the Pipeline Runner — NEVER wrap it in a single Task agent.
- NEVER implement code directly — ALL via Task agents (1 per subproject per wave).
- Wave dispatch: ALL items of one `wave-advance` round (the same dependency level) in ONE SINGLE message.
- Each sub-agent reads its own `{subproject}/CLAUDE.md` + auto-loads relevant skills.
- ALWAYS use `mustard-rt run wave-advance` to decide wave order/routing **and the post-impl review round** — NEVER read `wave-plan.md` and assemble the dispatch loop by hand. The LLM is a relay: iterate the returned array, pass each item's `prompt` to Task. (`dispatch-plan` is an inspection fallback for the full DAG — not the dispatch path.)
- NEVER hand-craft prompts — `wave-advance` IS the render: each item's `prompt` arrives already rendered by `agent-prompt-render` (as a `MUSTARD-PROMPT-REF` stub). Never build one from scratch, and never expand a stub by hand — the PreToolUse hook does it at dispatch.
- ALWAYS use `mustard-rt run resume-bootstrap` to decide mode/path/diff/slice/`nextAction` — NEVER reimplement those rules in the SKILL.
- ALWAYS run REVIEW + QA before CLOSE — `pipeline.complete` is refused (exit 2) without `qa.result`(overall=pass). REVIEW is NOT a manual side-step: it arrives as a `wave-advance` round (`role: review`, `mustard-review`, prompts rendered) — dispatch it like any round and record each verdict via `review-result --subproject`. Follow `nextAction` blindly. `close-pipeline` enforces this: QA fail/skip → `completed: false`, no close.
- ALWAYS check each impl item's `precheck` (Step 12d) before dispatch — `wave-advance` computes it inline; never re-invoke `dependency-precheck` per wave in the loop.
- Wave plan CLOSE only when every wave is in `completedWaves` (count === `totalWaves`) AND every touched subproject carries a `review.result` (i.e. `wave-advance` returns `[]`) AND `nextAction === "emit-complete"` — then close via `close-pipeline`, never the manual `qa-run` → `complete-spec` → `pipeline-summary` sequence. Do not gate CLOSE on the scalar `currentWave`.
