---
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

The per-subproject "how we write an X module here" skills that auto-load when an agent edits that folder (`{subproject}/.claude/skills/{role}-pattern/SKILL.md`). **Machine-authored molds stay FRESH**: every mold `scan-patterns-apply` writes carries a provenance marker (a SHA-256 of the written body); while that marker verifies — nobody edited the file — the mold is re-authored from the current exemplars on every scan. A mold with hand edits (or without a marker: legacy / hand-authored) is preserved forever.

1. `mustard-rt run scan-patterns-list` → JSON `[{subproject, label, slug, mode, moldPath, affix, exemplars, …}]` — every mineable role cluster (≥3 members, ≥2 hand-written exemplars, not a test/fixture path), uncapped. `mode: "create"` = no mold yet; `mode: "refresh"` = a machine-pristine mold being regenerated. Hand-edited/unmarked molds and slugs recorded in `.claude/scan-declined.json` are never listed; unparseable model → `[]`. Empty → skip.
2. Group by `subproject`; dispatch one agent per subproject with candidates, `subagent_type: mustard-patterns` (read-only — returns each mold as a `=== FILE: {moldPath} ===` block, each refusal as a `=== DECLINE: {slug} ===` block). The prompt lists ONLY that subproject's candidates (each `slug`/`mode`/`label`/`affix` + `exemplars`) + the canonical mold format. The agent authors refresh entries exactly like creates — fresh from the current exemplars, never from the old text. Single message, parallel.
3. Relay each `=== FILE ===` block to `mustard-rt run scan-patterns-apply --path <moldPath> --content -`, adding `--refresh` when that candidate's `mode` is `refresh` (apply re-verifies the marker before overwriting — hand edits survive even a confused relay). Relay each `=== DECLINE ===` block to `mustard-rt run scan-patterns-decline --slug <slug> --reason <one line>` so the candidate stops re-entering the worklist (re-audit = delete its entry in `.claude/scan-declined.json`). NEVER the orchestrator's own Write. Report per subproject: created / refreshed / preserved (hand-maintained) / declined.

## Inviolable

- The deterministic pass NEVER calls AI and NEVER reads source; it always writes `grain.model.json` + the lean maps (preserving `## Guards`, root excluded).
- Enrichment is STANDARD + fail-open — no opt-in flag, no confirmation, no dollar cost EVER. If a pass can run it runs silently; if it cannot it skips silently.
- An LLM in enrichment writes exactly TWO things: subproject `## Guards` (capped ~6 lines, non-destructive) and `{role}-pattern` molds — missing ones, plus machine-pristine ones regenerated fresh (the provenance marker must verify; a hand-edited or unmarked mold is NEVER overwritten). Never the root, never source, never system prompts.
