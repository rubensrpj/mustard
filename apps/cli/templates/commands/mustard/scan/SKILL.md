---
name: mustard-scan
description: Use when the user runs /scan or asks to analyze, discover, or rescan the codebase. Agnostic — discovers subprojects, dispatches one Task per subproject, refreshes registry, validates skills, runs security scan.
source: manual
---
<!-- mustard:generated -->
# /scan - Agnostic Code Analyzer

`/scan`, `/scan <subproject>`, `/scan --force` (bypass incremental skip).

## Process

### 1. Pre-dispatch

```bash
mustard-rt run scan-orchestrate [<subproject>] [--force]
```

Parse JSON. Binary handles discovery, hash comparison, stale cleanup, bootstrap files, Project Structure refresh, agent file generation, product-doc frontmatter, per-subproject agent prompt.

### 2. Dispatch

For each `dispatch[]` item, fire one Task in a single message (parallel). Pass `agentPrompt` literally — it already carries EVIDENCE RULE + step instructions. Never `run_in_background: true`. Empty `dispatch[]` → skip to step 3.

**`subagent_type` selection (token economy):** `scan-orchestrate` writes a rich, deterministic agent at `.claude/agents/{name}-impl.md` (and `{name}-explorer.md`) for every subproject — frontmatter with a routing-grade `description` + a body carrying that subproject's guards, recommended skills, and pre-mined clusters. When `.claude/agents/{name}-impl.md` exists (it is listed in `generated[]` after the first scan), dispatch with `subagent_type: "{name}-impl"` so Claude Code applies that agent's system prompt **natively** — the parent does not re-send guards/skills/clusters, saving prompt tokens. When the rich agent is absent (very first scan before generation, or a manual agent was preserved without one), fall back to `subagent_type: "general-purpose"`. The `agentPrompt` is passed verbatim in both cases; only the `subagent_type` differs.

### 3. Post-dispatch + verification

```bash
mustard-rt run scan-finalize
```

Refreshes registry, detect cache, validates skills, runs security scan, verifies HARD CONTRACT (each subproject wrote a `SKILL.md` or `_no-patterns.md`). Surface `errors[]`/`warnings[]`.

`dispatchVerify.ok === false` → one follow-up Task per `status === "empty"|"missing-dir"` subproject (single message, parallel): subproject MUST write ≥1 SKILL.md backed by ≥3 real files, OR a `_no-patterns.md` explaining why no cluster qualified. Re-run `scan-finalize`. Final summary only when `dispatchVerify.ok === true`.

## Return Format

```json
{ "scanned": [...], "skipped": [...], "generated": [], "cleanup": [],
  "skills_generated": { "sub": [...] }, "security": { "findings": 0 }, "errors": [] }
```

**Sourcing — do not invent counts:** `scanned`/`skipped`/`generated`/`cleanup` from `orchestrate.json.*`. `skills_generated[sub]` from `finalize.steps.dispatchVerify.subprojects[].skills` (live filesystem). `security.findings` from `finalize.steps.security.findings`. `errors` = concat of both error arrays.

## Fallback

`scan-orchestrate` fails: `mustard-rt run sync-detect` → one `Task(general-purpose)` per subproject (document patterns in `{path}/.claude/commands/*.md` with `<!-- mustard:generated -->` header, emit `SKILL.md` per pattern backed by ≥3 real files, no fenced code in body) → `sync-registry --force` → report which step failed.

## INVIOLABLE RULES

- No confirmation prompts — `/scan` is the approval. Read before Write/Edit (fallback mode).
- Never `run_in_background: true` for Task agents that write files.
