---
name: mustard-scan
description: Use when the user runs /scan or asks to analyze, discover, or rescan the codebase. Agnostic — Rust discovers subprojects, populates the entity-registry, renders per-cluster skills + stack.md + guards.md + the concept-graph wirelinks, and inserts a pending enrich block into every generated doc; then one AI prose agent per subproject fills those blocks (purpose), and a finalize step validates + runs a security scan.
source: manual
---
<!-- mustard:generated -->
# /scan - Agnostic Code Analyzer

`/scan`, `/scan <subproject>`, `/scan --force` (bypass incremental skip).

## Process

### 1. Generate (deterministic, no AI)

```bash
mustard-rt run scan-orchestrate [<subproject>] [--force]
```

Parse the JSON. The binary does ALL the mechanical work deterministically (no AI): discovery, hash comparison, stale cleanup, bootstrap files, **registry population**, **per-cluster `SKILL.md` + `references/examples.md` + `stack.md` + `guards.md`**, rich `.claude/agents/*`, the concept-graph **wirelinks** — and it inserts a **pending enrich block** (`<!-- mustard:enrich hash=… -->`) into every generated doc.

It returns **`enrichPending[]`**: one entry per subproject whose docs still carry a pending block, each with the exact `files` list and a ready-to-pass `agentPrompt`.

### 2. Enrich (automatic) — when `enrichPending[]` is non-empty

Emit **N `Task` tool-calls in ONE single assistant message** — one per `enrichPending[]` item. All N go out together (no text between them and the turn end); never `run_in_background: true`. Pass each item's `agentPrompt` **verbatim** — it already lists the exact files to edit and the hard rule: fill ONLY the text between the `<!-- mustard:enrich … -->` markers (a `## Purpose`, grounded in the real code the doc points to), never the skeleton, the markers, or the `hash=`.

**`subagent_type` selection (token economy):** when `.claude/agents/{name}-impl.md` exists (listed in `orchestrate.json.generated[]`), dispatch with `subagent_type: "{name}-impl"` so Claude Code applies that agent's system prompt natively. Otherwise `subagent_type: "general-purpose"`. The `agentPrompt` is passed verbatim either way.

If `enrichPending[]` is empty, skip straight to step 3.

### 3. Finalize + verification

```bash
mustard-rt run scan-finalize
```

Refreshes the detect cache, validates skills (the enrich blocks pass the validator), regenerates the concept-graph (`steps.graph`: nodes + wirelinks), runs the security scan. Surface `errors[]`/`warnings[]`. The enriched prose survives the next `/scan`: the Rust render preserves each block whose `hash` still matches the skeleton.

## Return Format

```json
{ "scanned": [...], "skipped": [...], "generated": [], "cleanup": [],
  "skills_generated": { "sub": [...] }, "enriched": 0, "security": { "findings": 0 }, "errors": [] }
```

**Sourcing — do not invent counts:** `scanned`/`skipped`/`generated`/`cleanup` from `orchestrate.json.*`. `skills_generated` from `orchestrate.json.generated` (`skills: N SKILL.md`). `enriched` = number of `enrichPending[]` subprojects you dispatched. `security.findings` from `finalize.steps.security.findings`. `errors` = concat of both error arrays.

## Fallback

`scan-orchestrate` fails: `mustard-rt run sync-detect` → `mustard-rt run sync-registry --force` → `mustard-rt run scan-skill-render` + `mustard-rt run scan-structural` per subproject (all deterministic, no AI) → report which step failed; skip enrich.

## INVIOLABLE RULES

- Enrich agents edit ONLY inside the `<!-- mustard:enrich … -->` markers — never the deterministic skeleton, the markers, or the `hash=`.
- Dispatch ALL enrich Tasks in ONE message (parallel), one per subproject.
- No confirmation prompts — `/scan` is the approval.
- Never `run_in_background: true` for Task agents.
