# /mustard:spec — Resume flow (continue pipeline)

Loaded on demand by SKILL Step 5 when `stage=Execute` (or `Analyze`/`QaReview`/`ReviewPending`/`QaPending`/`Close`). All mode decisions (`continued` vs `reanalyzed`), operational spec resolution, stub detection, `needsDiff`/`needsContextSlice`, `lastDispatchFailure` parsing, **and the post-execute REVIEW/QA decision**, have been moved to `mustard-rt run resume-bootstrap --spec X --json`. Literal agent prompt construction was moved to `mustard-rt run agent-prompt-render`. This ref only keeps what the binary cannot decide on its own.

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

When the bootstrap JSON indicates a wave plan, the orchestrator dispatches the current **dispatch level** — every wave that shares the lowest not-yet-completed `level` — in one message, never the entire spec and never a single wave when several waves share that level.

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
- **`subagent_type`**: each item carries its own — the tool picks the agent per role (read-only roles run tool-restricted: `explore`→`Explore`, `review`/`qa`→`mustard-review`, `guards`→`mustard-guards`; writing roles → `general-purpose`). Pass it through; never pick by hand.

The orchestrator does NOT decide the order, group rounds, or assemble the loop by hand — `dispatch-plan` owns that ("free section" determinised). `resume-bootstrap` stays the **stage** decision (mode / stage / progress); `dispatch-plan` is the **wave-routing** decision. (`--wave {N}` slices the array to a single item — a utility for re-rendering one wave's dispatch, NOT the normal per-round path. Do NOT drive the loop off the bootstrap's scalar `currentWave`: it names one wave, but a round can hold several independent waves of the same `level`.)

### Per-level loop

The **round** is every `dispatch-plan` item whose `level` equals the lowest level among items whose `wave` is NOT in `completedWaves` (from `resume-bootstrap` / `wave-tree`). Process one round at a time — never one wave at a time when the round holds several.

1. **Dispatch the whole round in ONE message.** For each item in the round: run Step 12d (dependency-precheck) on that wave's spec, then run its `prompt_cmd` and dispatch a Task with the stdout as `prompt` and the item's `subagent_type`. ALL `<invoke>` blocks go in a single message so the agents run concurrently.
2. **After each wave N in the round returns:**
   - Commit `/mustard:git commit` style with message `feat(wave-{N}/{role}): {summary}`. Fallback: `git add {files} && git commit -m "..."`.
   - Emit wave completion: `mustard-rt run emit-pipeline --kind pipeline.wave.complete --spec {specName} --payload "{\"wave\":{N},\"duration_ms\":{elapsed}}"`. The projection derives `completedWaves` from these events — no JSON state file.
   - Cache this wave's diff for dependent waves: `rtk git diff HEAD~1 HEAD --stat > .claude/spec/{specName}/wave-{N}-{role}/diff.md` — keep the redirect target **relative** (never an absolute `C:\...` path; the bash gate rejects Windows-style redirect targets). The next level's `agent-prompt-render` injects this file; the orchestrator does not pass anything explicitly.
3. **After the round completes**, run `mustard-rt run wave-tree --spec-dir .claude/spec/{specName}` to show progress, then advance to the next-lowest pending `level`.
4. When no pending wave remains → do **NOT** emit `pipeline.complete`. Re-run `resume-bootstrap` and follow `nextAction` (REVIEW → QA → CLOSE, in that order).
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
- Wave dispatch: ALL agents of the same `dispatch-plan` `level` in ONE SINGLE message.
- Each sub-agent reads its own `{subproject}/CLAUDE.md` + auto-loads relevant skills.
- ALWAYS use `mustard-rt run dispatch-plan` to decide wave order/routing — NEVER read `wave-plan.md` and assemble the dispatch loop by hand. The LLM is a relay: iterate the array, run each `prompt_cmd`, pass its stdout to Task.
- ALWAYS use `mustard-rt run agent-prompt-render` to build the prompt — NEVER from scratch. (You get the exact invocation as each item's `prompt_cmd`.)
- ALWAYS use `mustard-rt run resume-bootstrap` to decide mode/path/diff/slice/`nextAction` — NEVER reimplement those rules in the SKILL.
- ALWAYS run REVIEW + QA before CLOSE — `pipeline.complete` is refused (exit 2) without `qa.result`(overall=pass). Follow `nextAction` blindly.
- ALWAYS run dependency-precheck (Step 12d) before dispatch.
- Wave plan CLOSE only when every wave is in `completedWaves` (count === `totalWaves`) AND `nextAction === "emit-complete"`. Do not gate CLOSE on the scalar `currentWave`.
