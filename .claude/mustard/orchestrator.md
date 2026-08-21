# Orchestrator Rules

You are the router: for every request that touches the codebase, classify it, narrate your reading in one didactic line, then dispatch the matching flow. This file routes intent → flow only; the `/mustard:*` flows carry the detailed protocol (phases, gates, wave mechanics, spec layout) in their command files and refs.

## Intent Routing (the single door)

Classify intent + coarse scope — your reading; there is no pre-spec classifier. Narrate it before anything runs. Once a spec opens, `mustard-rt run scope-classify --from-spec <spec>` checks your call deterministically (`layerCount` is a fact there) — reclassify if it contradicts you.

| Intent | Signals | Kind |
|--------|---------|------|
| Feature (new entity / ≥2 layers) | create, add, implement across layers | `feature` |
| Enhancement (single-layer) | improve, adjust, add field, optimize | `task` (→ `feature` if it grows to ≥2 layers / new entity) |
| Bugfix | error, broken, fix | `bugfix` |
| Analyze | analyze, audit, compare, inspect | `task` (direct Grep/Glob; Explore if >3 places) |
| Vibe / spike | prototype, throwaway | `task` — no spec, no gates |
| Simple | config tweak, one-line edit, rename, version bump | direct (no Task) |

Each kind dispatches the `/mustard:<kind>` flow. **Dispatching means LOADING the flow's protocol, not improvising it:** for a `feature`/`bugfix`/`task`/`tactical-fix` kind, invoke `Skill(mustard:<kind>)` FIRST — that loads the command file (phases, the exact `mustard-rt` sequence with the right args, spec layout, gates) into the turn; then follow it, never your recollection of the commands. The flow is the source of the sequence — `spec-draft` is the ONLY `spec.md` writer (never hand-write it). **`--spec` means two different things and passing the wrong one fails as `spec-not-readable`, which reads like a broken tool and is not:** `scope-classify`/`plan-prepare`/`analyze-validation`/`dependency-precheck`/`exec-rewave-check` take the spec PATH (`.claude/spec/{slug}` or its `spec.md`); everything else takes the bare slug. `ac-negative-check` and `ac-amend` accept EITHER — they resolve a slug through the same locator `qa-run` uses. If the skill is unavailable, the same protocol lives in the plugin's `commands/<kind>.md`. `/mustard:*` also works as a direct power-override. Confirm only on a genuine fork (bugfix-vs-feature, light-vs-full, under-specified): ONE batched question — obvious cases proceed. Routing economy: the full pipeline only amortizes on genuine ≥2-layer work or a new entity (trust `layerCount`); single-layer or already-located → task or direct. Guards + digest need no pipeline — never enter it just for guidance.

**The DISPATCH itself — the one question a unit opens with, the base gate, and the name it mints — lives in the companion injectable `.claude/mustard/dispatch.md`, delivered at `sessionStart`.** Two files on two events is deliberate: one hook response carries at most 10,000 characters of `additionalContext`, and past that the harness saves the overflow to a file and hands the window a preview plus a path — the text is not lost, but it stops being IN FORCE. Since the composer folds every injectable of the same event into one payload, separate events are what give each half its own ceiling. Read that file before opening a unit; it is not optional detail.

## Delegate via Task

Delegate non-trivial code work: pipeline EXECUTE/PLAN, exploration >3 files or >2 dirs, multi-file new code, refactor ≥3 files, any agent-typed work. Do directly: read one file to answer, edit ≤2 identified files, status/version commands, a single Grep/Glob, vibe mode. Verdict rule — two claims are never relayed on a briefing alone. (1) A runtime symptom the user reported cannot be refuted by static reading: verify a contradiction by reading before relaying it. (2) A MEASUREMENT an agent says it took — a suite that passed, a count, a verdict, a close that completed — is not evidence until you take it yourself; re-run the command and read the output. Everything else in a briefing IS the answer: do not re-derive what the agent already did, and do not spend a subagent double-checking your own work.

## Phases

`ANALYZE → PLAN → /approve → EXECUTE → REVIEW → QA → CLOSE`. Light skips PLAN and prefers direct Grep/Glob (the flow reclassifies upward as file count grows — trust its thresholds); Full runs them all. The flows drive these phases; `qa-run` runs each `## Acceptance Criteria` and the close gate blocks CLOSE without a QA pass (`MUSTARD_QA_GATE_MODE=strict|warn|off`). The full phase, gate and mid-pipeline change-request protocol lives in those flows — this file does not restate it.

**Four doors, and only four: `/mustard:git`, `/mustard:pr`, `/mustard:spec`, `/mustard:upsert`.** Everything else is a flow YOU dispatch, never something the user types. Review, QA and CLOSE are steps of `/mustard:pr merge`; the census refresh is a step of the base gate in `dispatch.md`; turning the harness off or on and diagnosing the install are flags of `/mustard:upsert`; cancelling an abandoned unit is `/mustard:git delete`. Never tell the user to run a command outside those four.

## Locating code

The terrain census is injected at session start — don't grep to orient. A known literal token → `grep`/`glob`. A concept with an unknown name → `mustard-rt run feature --intent "..."`, then READ the pointed files (recall is strong, not perfect).

**`base-gate: enrichment stale` on stderr means that census is only half-authored** — the deterministic model is fresh, but the `## Guards` prose and `{role}-pattern` molds the line names were never written by an agent. Say so to the operator in ONE sentence, and offer the `scan` flow as a unit of its OWN: it rewrites versioned files, so it wants a clean tree and is dispatched only once the current unit closes — never as a step inside the one running.

## Efficiency

Before any Read/Grep/Bash: is it already in context? Use it. Trust a subagent's briefing; re-read only under the Verdict rule. Run a deterministic `mustard-rt run …` once — capture to a file, then slice the file. Prefix standard shell with `rtk` (`rtk git/grep/ls/cargo`, 60-90% off); `mustard-rt run …` stays bare. `rtk` wraps ONE filtered command — never a builtin, loop, or heredoc. `git add` is always `-A`.
