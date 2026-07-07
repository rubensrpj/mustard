---
name: mustard-scan
description: Use when the user runs /scan or asks to analyze, discover, or rescan the codebase. Mines the repo into grain.model.json (deterministic, language-agnostic) and then enriches it as standard — subproject Guards prose + missing pattern-skill molds — the durable model the feature/spec pipeline consumes.
source: manual
---
<!-- mustard:generated -->
# /scan — Codebase model

`/scan`, `/scan --root <dir>`, `/scan --out <path>`. **Enrichment is STANDARD** — there is no `--full` or `--enrich` flag. One `/scan` always does the deterministic model, the subproject maps, and the enrichment (subproject Guards prose + missing pattern-skill molds).

## Process

### 1. Deterministic model + maps (no AI, you do NOT read source)

```bash
mustard-rt run scan --full [--root <dir>] [--out <path>]
```

Always writes `.claude/grain.model.json` — the rich, language-agnostic model (modules, declarations, dependency graph, mined roles, recurring vertical slices, shared contracts, touchpoints, projects) — AND deterministically (re)generates a lean `CLAUDE.md` map per subproject from that model: it **preserves** any hand-written `## Guards`, seeds a `pending` `## Guards` placeholder where none exists, and **never touches the workspace root**. The model is the durable product: downstream (`/feature`, `/bugfix`) consumes it through `mustard-rt run feature` (the digest research step) and `mustard-rt run scan spec` — never by reading the model or the repo directly.

Parse the JSON result (`{ ok, model, regenerated?, oversized? }`); report the model path and surface any `oversized` warnings or `regenerated` paths.

### 2. Enrichment (STANDARD — the AI part of `/scan`)

After the model is built, ALWAYS run the enrichment below: the subproject **Guards** (do/don't prose). It is **incremental** (only the delta since the last scan — subprojects still `pending`) and **fail-open** (headless / no LLM / empty worklist → skip silently; the deterministic model is already complete and correct). The FIRST scan of a repo pays the one-time enrichment cost; every later scan only does what changed.

1. **Worklist.** `mustard-rt run scan-guards-list` emits a JSON array `[{path, subproject, kind, frameworks}]` of every subproject `CLAUDE.md` still `pending` (root already excluded). Fail-open: on error it returns `[]` (exit 0) — nothing to do. Empty → skip the enrichment.
2. **Render one prompt per subproject.** For each worklist item: `mustard-rt run agent-prompt-render --role guards --subproject <subproject> --emit ref` — spec-less (no `--spec`): the renderer reads the pending block's facts and derives the project's language/tone from `mustard.json`. Pass its stdout to the Task **verbatim** (with `--emit ref` it is a 2-line stub the PreToolUse hook expands at dispatch — never read the `.dispatch/` file in the parent); never hand-craft the prompt.
3. **Dispatch in parallel + relay.** Dispatch **one agent per subproject**, `subagent_type` `mustard-guards` (read-only — returns the lines as its final message), all in a **single message** so they run in parallel. Relay each agent's authored guards to `mustard-rt run scan-guards-apply --path <path> --guards -` (text on stdin). The apply is non-destructive (only the block's span changes), capped at ~6 lines, idempotent (flips the marker off `pending`).
4. **Root never enters** — `scan-guards-list` already excludes it.

### 3. Pattern skills (STANDARD — the second and last AI part of `/scan`)

After the Guards, ALWAYS author the missing **pattern-skill molds** — the per-subproject "how we write an X module here" skills that load automatically when an agent edits that folder (`{subproject}/.claude/skills/{role}-pattern/SKILL.md`). Same economics as Guards: **incremental** (only missing molds are authored — existing ones are NEVER touched, they may carry hand maintenance) and **fail-open** (headless / no LLM / nothing missing → skip silently).

1. **Worklist (deterministic-first).** From the model you already hold (step 1's digest — do NOT re-run it): for each subproject, list its role clusters with **≥3 member files sharing one convention** (same role folder / affix). Then Glob `{subproject}/.claude/skills/*-pattern/SKILL.md` and keep only clusters with NO existing mold. Cap at **≤4 new molds per subproject per scan**. Empty → skip silently.
2. **Dispatch in parallel.** One agent per subproject that has candidates, `subagent_type` `mustard-patterns` (read-only — returns each mold as a demarcated `=== FILE: … ===` block). The dispatch prompt lists ONLY that subproject's candidate clusters, each with 2-3 exemplar file paths from the model, plus the canonical mold format and the repo's existing-mold language. Single message, all agents in parallel.
3. **Write + report.** The orchestrator writes each returned block verbatim to its `=== FILE: … ===` path (create only — if the file appeared meanwhile, skip it). Report molds created per subproject; a cluster the agent judged too weak to teach is skipped, not padded.

## INVIOLABLE RULES

- The **deterministic pass** (model + maps) NEVER calls AI and NEVER reads source; it always writes `grain.model.json` and the lean per-subproject `CLAUDE.md` maps (preserving hand-written `## Guards`, root excluded). It is unconditional.
- **Enrichment is a STANDARD and COMPLETE part of every `/scan`** — no opt-in flag, no confirmation prompt: Guards prose and missing pattern molds are the ONLY LLM steps, one cheap read-only agent per subproject per product. Both are **fail-open**: if a step cannot run (headless, no LLM, empty worklist) it skips silently, never blocking and never corrupting the model.
- An LLM in enrichment WRITES exactly TWO things: subproject `## Guards` prose (capped ~6 lines, non-destructive) and MISSING `{role}-pattern` skills (create-only — an existing mold is NEVER overwritten; it may carry hand maintenance). Never the workspace root, never source, never system prompts.
- **`/scan` IS the approval — there is NO spend prompt, EVER.** Do NOT ask whether to (re)run the enrichment, do NOT present "full / partial / skip" options, do NOT cite a dollar cost. If a step can run it runs silently; if it cannot it skips silently.
