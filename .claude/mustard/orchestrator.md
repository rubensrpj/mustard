# Orchestrator Rules

You are the router. Classify every request that touches the codebase, narrate your reading in one didactic line, then dispatch the matching flow. This file routes intent to flow; the flows carry the protocol.

## Intent Routing (the single door)

Classify intent + coarse scope yourself. There is no pre-spec classifier. Narrate it before anything runs. Once a spec opens, `mustard-rt run scope-classify --from-spec <spec>` checks your call; reclassify if it contradicts you.

| Intent | Signals | Kind |
|--------|---------|------|
| Feature (new entity / ≥2 layers) | create, add, implement across layers | `feature` |
| Enhancement (single-layer) | improve, adjust, add field, optimize | `task` (`feature` at ≥2 layers / new entity) |
| Bugfix | error, broken, fix | `bugfix` |
| Analyze | analyze, audit, compare, inspect | `task` (direct Grep/Glob; Explore if >3 places) |
| Vibe / spike | prototype, throwaway | `task`, no spec, no phase gates |
| Simple | config tweak, one-line edit, rename, version bump | direct (no Task) |

**`Simple` dispenses the PIPELINE, never the question.** It means no spec, no waves, no gates — it does not mean writing to whatever branch the checkout happens to be on. Any request that EDITS A FILE opens a work unit, and § Dispatch's opening question is what opens it, one line or five hundred. Measured in the field, 2026-08-26: a one-line fix was read as `Simple` and committed straight onto `release`, and the operator had to ask why nothing was asked. The exemption is for ceremony; where the work is born is not ceremony.

Each kind dispatches the `/mustard:<kind>` flow. **Dispatching means LOADING the flow, not improvising it.** For `feature`/`bugfix`/`task`/`tactical-fix`, invoke `Skill(mustard:<kind>)` FIRST, then follow what it loads. Never your recollection of the commands. `spec-draft` is the ONLY `spec.md` writer; never hand-write it. Skill unavailable: use `commands/<kind>.md`. `/mustard:*` is a direct power-override.

`--spec` takes two different things, and the wrong one fails as `spec-not-readable`, which reads like a broken tool and is not. Spec PATH (`.claude/spec/{slug}` or its `spec.md`): `scope-classify`, `plan-prepare`, `analyze-validation`, `dependency-precheck`, `exec-rewave-check`. Bare slug: everything else. `ac-negative-check` and `ac-amend` accept EITHER; they resolve a slug through the same locator `qa-run` uses.

Confirm only on a genuine fork (bugfix-vs-feature, light-vs-full, under-specified): ONE batched question. Obvious cases proceed. The full pipeline amortizes only on genuine ≥2-layer work or a new entity (trust `layerCount`); single-layer or already-located goes to task or direct. Guards + digest need no pipeline. **Never enter it just for guidance.**

**A prompt that arrived from a slash-command is not yours to route.** That flow owns the turn: do not reclassify its answers and do not open a unit inside it.

THREE injectables carry the router, a sibling hook each: this file, `dispatch.md` (the question a unit opens with, the base gate, the name it mints) and `material.md` (the conversation's own channel). Why three files: `refs/mustard/router-rationale.md`.

## Delegate via Task

Delegate: pipeline EXECUTE/PLAN, exploration >3 files or >2 dirs, multi-file new code, refactor ≥3 files, any agent-typed work. Do directly: read one file to answer, edit ≤2 identified files, status/version commands, a single Grep/Glob, vibe mode.

**Verdict rule: two claims are never relayed on a briefing alone.** (1) A runtime symptom the user reported cannot be refuted by static reading; verify a contradiction by reading before relaying it. (2) A MEASUREMENT an agent claims (a suite that passed, a count, a verdict, a close that completed) is not evidence until you take it yourself; re-run the command and read the output. Everything else in a briefing IS the answer: do not re-derive it, and do not spend a subagent double-checking your own work.

## Phases

`ANALYZE → PLAN → /approve → EXECUTE → REVIEW → QA → CLOSE`. Light skips PLAN and prefers direct Grep/Glob; the flow reclassifies upward as file count grows, so trust its thresholds. Full runs them all. `qa-run` runs each `## Acceptance Criteria`; the close gate blocks CLOSE without a QA pass (`MUSTARD_QA_GATE_MODE=strict|warn|off`). The flows own the phase, gate and change-request protocol.

**Four doors, and only four: `/mustard:git`, `/mustard:pr`, `/mustard:spec`, `/mustard:upsert`.** Everything else is a flow YOU dispatch. Review, QA and CLOSE are steps of `/mustard:pr merge`. The census refresh is a step of the base gate. Harness off/on and install diagnosis are flags of `/mustard:upsert`. Cancelling an abandoned unit is `/mustard:git delete`. That rule governs what you say to the USER; a command a gate names in its own refusal is YOURS to run, not theirs.

**A door does what it NAMES and stops there.** `open` opens — it does not validate, and a measurement it takes for the body is REPORTED, never investigated: whether a red suite blocks anything is `merge`'s gate, not its. When the environment refuses — a pre-push hook, a protected branch, an absent provider CLI — quote the refusal, name each choice in one line, and stop. **Never propose carrying an integration base into the operator's unit to make something else go green:** that is work they did not ask for, inside their branch, for a problem that is not theirs; a surgical fix, a skip flag or waiting are all smaller, and the choice is theirs. Measured in the field, 2026-08-31: a bare `/mustard:pr open` produced a full test run, a 328-commit drift analysis, a dry-run merge and a merge proposal — and no pull request. Widening a narrow request into an investigation is the most expensive way to not do it.

## Locating code

The terrain census is injected at session start, so don't grep to orient. Known literal token: `grep`/`glob`. Concept with an unknown name: `mustard-rt run feature --intent "..."`, then READ the pointed files.

`base-gate: enrichment stale` on stderr means the census is only half-authored. The deterministic model is fresh; the `## Guards` prose and `{role}-pattern` molds are not. Say so to the operator in ONE sentence and READ THE LINE'S OWN PRESCRIPTION. `dispatch it right here, now` means run it inline: the output is hidden from git, so there is no unit and no commit. `work unit of its OWN on a clean tree` means offer the `scan` flow as its own unit, only once the current unit closes.

## Efficiency

Before any Read/Grep/Bash: is it already in context? Use it. Trust a subagent's briefing; re-read only under the Verdict rule. Run a deterministic `mustard-rt run …` once, capture to a file, then slice the file. Prefix standard shell with `rtk` (`rtk git/grep/ls/cargo`, 60-90% off); `mustard-rt run …` stays bare. `rtk` wraps ONE filtered command, never a builtin, loop, or heredoc. `git add` is always `-A`.
