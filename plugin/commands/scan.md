---
name: scan
description: Use when the user runs /scan or asks to analyze, discover, or rescan the codebase. Mines the repo into grain.model.json (deterministic, language-agnostic) and then enriches it as standard — subproject Guards prose + missing pattern-skill molds — the durable model the feature/spec pipeline consumes.
source: manual
---
<!-- mustard:generated -->
# /scan — Codebase model

`/scan [--root <dir>] [--out <path>]`. **Enrichment is STANDARD** — no `--full`/`--enrich` flag, no spend prompt. Every `/scan` does the deterministic model, the subproject maps, and both enrichment passes (Guards + pattern molds).

## 1. Deterministic model + maps (no AI, you do NOT read source)

```bash
mustard-rt run scan --full [--root <dir>] [--out <path>]
```

Writes `.claude/grain.model.json` (the language-agnostic model — modules, declarations, dependency graph, mined roles, vertical slices, shared contracts, touchpoints) AND regenerates a lean `CLAUDE.md` map per subproject: it **preserves** hand-written `## Guards`, seeds a `pending` placeholder where none exists, and **never touches the workspace root**. Downstream (`/feature`, `/bugfix`) consumes the model via `mustard-rt run feature` and `scan spec` — never by reading it directly. Parse the JSON (`{ ok, model, regenerated?, oversized? }`); report the model path + any `oversized` warnings.

Both enrichment passes below are **incremental** (only the delta since the last scan) and **fail-open** (headless / no LLM / empty worklist → skip silently; the model is already complete). One cheap read-only agent per subproject per pass.

## 2. Guards (do/don't prose)

1. `mustard-rt run scan-guards-list` → JSON `[{path, subproject, kind, frameworks}]` for every subproject `CLAUDE.md` still `pending` (root excluded; error → `[]`). Empty → skip.
2. Per item: `mustard-rt run agent-prompt-render --role guards --subproject <subproject> --emit ref` (spec-less — the renderer reads the pending block + derives language/tone from `mustard.json`). Pass the stub to the Task **verbatim**.
3. Dispatch **one agent per subproject** `subagent_type: mustard-guards` (read-only), all in ONE message. Relay each agent's lines to `mustard-rt run scan-guards-apply --path <path> --guards -` (stdin). Non-destructive, capped ~6 lines, flips the marker off `pending`.

Critical Guards: a line may open with `[critical]` to be enforced at edit time — the post-edit gate Denies (strict) or advises (warn, the default; `MUSTARD_GUARD_GATE_MODE`) an edit that violates the checkable form `[critical] never <forbidden> in <glob>`. Author sparingly; unmarked Guards stay advisory. See `mustard-guards.md`.

## 3. Pattern skills (the `{role}-pattern` molds)

The per-subproject "how we write an X module here" skills that auto-load when an agent edits that folder (`{subproject}/.claude/skills/{role}-pattern/SKILL.md`). Existing molds are NEVER touched (they may carry hand maintenance).

1. `mustard-rt run scan-patterns-list` → JSON `[{subproject, label, slug, moldPath, affix, exemplars, …}]` — every mineable role cluster (≥3 members, not a test/fixture path), existing molds filtered, capped 4/subproject; unparseable model → `[]`. Empty → skip. (Replaces hand-slicing the MB-sized model in the orchestrator.)
2. Group by `subproject`; dispatch one agent per subproject with candidates, `subagent_type: mustard-patterns` (read-only — returns each mold as a `=== FILE: {moldPath} ===` block). The prompt lists ONLY that subproject's candidates (each `slug`/`label`/`affix` + `exemplars`) + the canonical mold format. Single message, parallel.
3. Relay each returned block to `mustard-rt run scan-patterns-apply --path <moldPath> --content -` — NEVER the orchestrator's own Write. Create-only, path-guarded, atomic (writes even as a background job). Report molds created per subproject.

## Inviolable

- The deterministic pass NEVER calls AI and NEVER reads source; it always writes `grain.model.json` + the lean maps (preserving `## Guards`, root excluded).
- Enrichment is STANDARD + fail-open — no opt-in flag, no confirmation, no dollar cost EVER. If a pass can run it runs silently; if it cannot it skips silently.
- An LLM in enrichment writes exactly TWO things: subproject `## Guards` (capped ~6 lines, non-destructive) and MISSING `{role}-pattern` molds (create-only). Never the root, never source, never system prompts.
