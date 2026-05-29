# /mustard:spec — Resume flow (continue pipeline)

Loaded on demand by SKILL Step 5 when `stage=Execute` (or `Analyze`/`QaReview`/`ReviewPending`/`QaPending`/`Close`). All mode decisions (`continued` vs `reanalyzed`), operational spec resolution, stub detection, `needsDiff`/`needsContextSlice`, `waveModel` lookup, `lastDispatchFailure` parsing, **and the post-execute REVIEW/QA decision**, have been moved to `mustard-rt run resume-bootstrap --spec X --json`. Literal agent prompt construction was moved to `mustard-rt run agent-prompt-render`. This ref only keeps what the binary cannot decide on its own.

## Stage values post-execute (never freelance)

The binary can return three extra `stage` values when all waves complete. The orchestrator NEVER decides on its own — always dispatch what `nextAction` indicates:

| `stage` | `nextAction` | Companion field | What to do |
|---------|--------------|-----------------|------------|
| `ReviewPending` | `dispatch-review` | `reviewRoles: [...]` | Dispatch one REVIEW Task per role |
| `QaPending` | `run-qa` | `qaCommand: "..."` | Run the command literally |
| `Close` | `emit-complete` | — | Only now is `emit-pipeline --kind pipeline.complete` allowed |

When `nextAction` is `null`, there is still a wave to run — follow the normal wave-dispatch flow below.

## Hard gate on `emit-pipeline --kind pipeline.complete`

As of 2026-05-25 the binary refuses to emit `pipeline.complete` without a `qa.result` (overall=pass) in the spec's ndjson — exit 2 + message `BLOCKED: …`. The escape hatch `--allow-no-qa` exists only for `qa-run` itself and explicit user overrides. Do not try to work around it.

## Step 12c — Wave Plan Scope (conditional, only if `isWavePlan === true`)

When the bootstrap JSON indicates a wave plan, the orchestrator dispatches only the **current wave**, never the entire spec.

### Routing is decided by Rust — the orchestrator is a relay

The wave **order and routing** are not interpreted by the LLM. Run:

```bash
mustard-rt run dispatch-plan --spec {specName}
```

It reads `wave-plan.md`, builds the dependency DAG, and returns a deterministic JSON array ordered by dependency level. Each item is:

```json
{ "wave": 2, "role": "cli", "subproject": "apps/cli", "depends_on": [1], "level": 1,
  "prompt_cmd": "mustard-rt run agent-prompt-render --spec … --wave 2 --role cli --subproject apps/cli --mode first" }
```

- **`level`** is the dispatch round. Items sharing a `level` have no dependency between them → dispatch them **together in one message** (several `<invoke>` blocks). Never dispatch a higher `level` before every lower-level wave has completed.
- **`prompt_cmd`** is a ready `agent-prompt-render` invocation — NOT the prompt. Run it; pass its **stdout** as the Task `prompt`.
- **`subagent_type`**: if `.claude/agents/{subproject-name}-impl.md` exists, use `subagent_type: "{subproject-name}-impl"` (rich agent); otherwise `subagent_type: "general-purpose"`. (`subproject-name` = last path segment of `subproject`.)

To dispatch just the current wave, slice with `--wave {currentWave}`. The orchestrator does NOT decide the order, group rounds, or assemble the loop by hand — `dispatch-plan` owns that ("free section" determinised). `resume-bootstrap` stays the **stage** decision (mode / stage / current wave / model); `dispatch-plan` is the **wave-routing** decision.

### Per-wave loop

1. The spec for this invocation is `operationalSpecPath` returned by bootstrap (already resolved to `wave-{currentWave}-*/spec.md`); the matching `dispatch-plan` item gives its `role` + `subproject` + `prompt_cmd`.
2. **Between waves** (post-dispatch of wave N):
   - Commit `/mustard:git commit` style with message `feat(wave-{N}/{role}): {summary}`. Fallback: `git add {files} && git commit -m "..."`.
   - Emit wave completion: `mustard-rt run emit-pipeline --kind pipeline.wave.complete --spec {specName} --payload "{\"wave\":{N},\"duration_ms\":{elapsed}}"`. The projection derives `completedWaves` + `currentWave` from these events — no JSON state file.
   - Run `mustard-rt run wave-tree --spec-dir .claude/spec/{specName}` to show progress.
   - Cache this wave's diff: `git diff HEAD~1 HEAD > .claude/.pipeline-states/{specName}.wave-{N-1}.diff.md`. The next wave's `agent-prompt-render` injects this file; the orchestrator does not pass anything explicitly.
3. If `currentWave >= totalWaves` → do **NOT** emit `pipeline.complete`. Re-run `resume-bootstrap` and follow `nextAction` (REVIEW → QA → CLOSE, in that order).
4. If a wave fails (REJECTED after 2 fix-loops, or BLOCKED) → see Escalation Statuses + `../resume/fix-loop-wave.md`.

## Step 12d — Dependency Precheck (factual gate)

Before dispatching the wave, run:

```bash
mustard-rt run dependency-precheck --spec {operationalSpecPath}
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
- Wave dispatch: ALL agents of the same `dispatch-plan` `level` in ONE SINGLE message.
- Each sub-agent reads its own `{subproject}/CLAUDE.md` + auto-loads relevant skills.
- ALWAYS use `mustard-rt run dispatch-plan` to decide wave order/routing — NEVER read `wave-plan.md` and assemble the dispatch loop by hand. The LLM is a relay: iterate the array, run each `prompt_cmd`, pass its stdout to Task.
- ALWAYS use `mustard-rt run agent-prompt-render` to build the prompt — NEVER from scratch. (You get the exact invocation as each item's `prompt_cmd`.)
- ALWAYS use `mustard-rt run resume-bootstrap` to decide mode/path/diff/slice/`nextAction` — NEVER reimplement those rules in the SKILL.
- ALWAYS run REVIEW + QA before CLOSE — `pipeline.complete` is refused (exit 2) without `qa.result`(overall=pass). Follow `nextAction` blindly.
- ALWAYS run dependency-precheck (Step 12d) before dispatch.
- Wave plan CLOSE only when `currentWave === totalWaves` AND `nextAction === "emit-complete"`.
