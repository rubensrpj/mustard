# /mustard:spec — Resume flow (continue pipeline)

Loaded on demand by SKILL Step 5 when `stage=Execute` (or `Analyze`/`QaReview`/`ReviewPending`/`QaPending`/`Close`). All mode decisions (`continued` vs `reanalyzed`), operational spec resolution, stub detection, `needsDiff`/`needsContextSlice`, `lastDispatchFailure` parsing, **and the post-execute REVIEW/QA decision**, have been moved to `mustard-rt run resume-bootstrap --spec X --json`. Literal agent prompt construction was moved to `mustard-rt run agent-prompt-render`; wave routing + prompts arrive **already rendered** via `mustard-rt run wave-advance`. This ref only keeps what the binary cannot decide on its own.

## Stage values post-execute (never freelance)

The binary can return three extra `stage` values when all waves complete. The orchestrator NEVER decides on its own — always dispatch what `nextAction` indicates:

| `stage` | `nextAction` | Companion field | What to do |
|---------|--------------|-----------------|------------|
| `ReviewPending` | `dispatch-review` | `reviewRoles: [...]` | Dispatch one REVIEW Task per role |
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

It reads the wave DAG and returns the **current round only** — every wave of the first dependency level whose waves lack `pipeline.wave.complete` — as a deterministic JSON array; `[]` when all waves are done. Each item is:

```json
{ "wave": 2, "role": "cli", "subproject": "apps/cli", "subagent_type": "general-purpose",
  "prompt": "…the FULL Task prompt, ALREADY RENDERED…" }
```

- Items returned together share one dependency level → dispatch them **together in one message** (several `<invoke>` blocks). Never reach for a later level by hand — re-run `wave-advance` after the round completes and it advances on its own.
- **`prompt`** IS the final Task prompt — already rendered by `agent-prompt-render` inside the binary. There is no `prompt_cmd` round-trip and nothing to assemble; pass it **verbatim** as the Task `prompt`.
- **`subagent_type`**: each item carries its own — the tool picks the agent per role (read-only roles run tool-restricted: `explore`→`Explore`, `review`/`qa`→`mustard-review`, `guards`→`mustard-guards`; writing roles → `general-purpose`). Pass it through; never pick by hand.

The orchestrator does NOT decide the order, group rounds, or assemble the loop by hand — `wave-advance` owns that ("free section" determinised). `resume-bootstrap` stays the **stage** decision (mode / stage / progress); `wave-advance` is the **wave-routing + render** decision. (`dispatch-plan` still exists — use it only to **inspect** the full DAG/levels, e.g. when debugging routing; it is not the dispatch path. Do NOT drive the loop off the bootstrap's scalar `currentWave`: it names one wave, but a round can hold several independent waves of the same level.)

### Per-level loop

The **round** is exactly the array one `wave-advance` call returns. Process one round at a time — never one wave at a time when the round holds several.

1. **Dispatch the whole round in ONE message.** For each item in the round: run Step 12d (dependency-precheck) on that wave's spec, then dispatch a Task with the item's `prompt` (verbatim) and the item's `subagent_type`. ALL `<invoke>` blocks go in a single message so the agents run concurrently.
2. **After each wave N in the round returns:**
   - Commit `/mustard:git commit` style with message `feat(wave-{N}/{role}): {summary}`. Fallback: `git add {files} && git commit -m "..."`.
   - Emit wave completion: `mustard-rt run emit-pipeline --kind pipeline.wave.complete --spec {specName} --payload "{\"wave\":{N},\"duration_ms\":{elapsed}}"`. The projection derives `completedWaves` from these events — no JSON state file.
   - Cache this wave's diff for dependent waves: `rtk git diff HEAD~1 HEAD --stat > .claude/spec/{specName}/wave-{N}-{role}/diff.md` — keep the redirect target **relative** (never an absolute `C:\...` path; the bash gate rejects Windows-style redirect targets). The next round's render (inside `wave-advance`) injects this file; the orchestrator does not pass anything explicitly.
3. **After the round completes**, run `mustard-rt run wave-tree --spec-dir .claude/spec/{specName}` to show progress, then re-run `wave-advance` — it returns the next round.
4. When `wave-advance` returns `[]` (no pending wave) → do **NOT** emit `pipeline.complete`. Re-run `resume-bootstrap` and follow `nextAction` (REVIEW, then `close-pipeline` for QA + CLOSE, in that order).
5. If a wave fails (REJECTED after 2 fix-loops, or BLOCKED) → see Escalation Statuses + `../resume/fix-loop-wave.md`. A failed wave blocks the higher levels that depend on it; independent waves in the same round still complete.

## Step 12d — Dependency Precheck (factual gate)

Before dispatching each wave in the round, run it on that wave's spec:

```bash
mustard-rt run dependency-precheck --spec .claude/spec/{specName}/wave-{N}-{role}/spec.md
```

Parse the JSON. If `ok: false`:

1. Print inline: `BLOCKED — N missing symbols: {missing.symbol}. Suggestion: create tactical-fix.`
2. Emit `mustard-rt run emit-pipeline --kind pipeline.dispatch_failure --spec {specName} --payload "..."`.
3. AskUserQuestion: **Create tactical-fix automatically** / **Investigate manually** / **Force dispatch (override)**.

**Skip if `resume-bootstrap` returned `mode: continued`** or env `MUSTARD_DEPENDENCY_PRECHECK_MODE=off`.

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
- ALWAYS use `mustard-rt run wave-advance` to decide wave order/routing — NEVER read `wave-plan.md` and assemble the dispatch loop by hand. The LLM is a relay: iterate the returned array, pass each item's `prompt` to Task. (`dispatch-plan` is an inspection fallback for the full DAG — not the dispatch path.)
- NEVER hand-craft prompts — `wave-advance` IS the render: each item's `prompt` arrives already rendered by `agent-prompt-render`. Never build one from scratch.
- ALWAYS use `mustard-rt run resume-bootstrap` to decide mode/path/diff/slice/`nextAction` — NEVER reimplement those rules in the SKILL.
- ALWAYS run REVIEW + QA before CLOSE — `pipeline.complete` is refused (exit 2) without `qa.result`(overall=pass). Follow `nextAction` blindly. `close-pipeline` enforces this: QA fail/skip → `completed: false`, no close.
- ALWAYS run dependency-precheck (Step 12d) before dispatch.
- Wave plan CLOSE only when every wave is in `completedWaves` (count === `totalWaves`, i.e. `wave-advance` returns `[]`) AND `nextAction === "emit-complete"` — then close via `close-pipeline`, never the manual `qa-run` → `complete-spec` → `pipeline-summary` sequence. Do not gate CLOSE on the scalar `currentWave`.
