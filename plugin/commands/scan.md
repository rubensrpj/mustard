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

The per-subproject "how we write an X module here" skills that auto-load when an agent edits that folder (`{subproject}/.claude/skills/{role}-pattern/SKILL.md`). **A mustard-generated mold is derived — it is regenerated from scratch on every scan, never preserved.** The origin signal is the frontmatter `source:` field: `source: scan` = mustard-generated (swept and re-authored); `source: manual` (or hand-authored) = human-owned, preserved forever. To adopt a generated mold and stop regenerating it, flip `source: scan` → `source: manual`. The flow is **sweep → list → author → apply**:

0. `mustard-rt run scan-patterns-sweep` → deletes every `source: scan` mold under the tree BEFORE authoring, so each is written fresh with no bias from its old text (also drops orphans whose cluster no longer exists). Preserves `source: manual`. Prints `{removed, preserved}`.
1. `mustard-rt run scan-patterns-list` → JSON `[{subproject, label, slug, moldPath, affix, exemplars, …}]` — every mineable role cluster (≥3 members, ≥2 hand-written exemplars, not a test/fixture path), uncapped. Post-sweep everything is a create; a surviving `source: manual` mold and slugs in `.claude/scan-declined.json` are never listed; unparseable model → `[]`. Empty → skip.
2. Group by `subproject`; dispatch one agent per subproject with candidates, `subagent_type: mustard-patterns` (read-only — returns each mold as a `=== FILE: {moldPath} ===` block, each refusal as a `=== DECLINE: {slug} ===` block). The prompt lists ONLY that subproject's candidates (each `slug`/`label`/`affix` + `exemplars`) + the canonical mold format (which includes `source: scan`). Single message, parallel.
3. Relay each `=== FILE ===` block to `mustard-rt run scan-patterns-apply --path <moldPath> --content -` (create-only; injects the `<!-- mustard:generated -->` notice). Relay each `=== DECLINE ===` block to `mustard-rt run scan-patterns-decline --slug <slug> --reason <one line>` so the candidate stops re-entering the worklist (re-audit = delete its entry in `.claude/scan-declined.json`). NEVER the orchestrator's own Write. Report per subproject: created / preserved (hand-authored) / declined.

## Inviolable

- The deterministic pass NEVER calls AI and NEVER reads source; it always writes `grain.model.json` + the lean maps (preserving `## Guards`, root excluded).
- Enrichment is STANDARD + fail-open — no opt-in flag, no confirmation, no dollar cost EVER. If a pass can run it runs silently; if it cannot it skips silently.
- An LLM in enrichment writes exactly TWO things: subproject `## Guards` (capped ~6 lines, non-destructive) and `{role}-pattern` molds — swept fresh each scan (every `source: scan` mold is deleted then re-authored; a `source: manual`/hand-authored mold is NEVER touched). Never the root, never source, never system prompts.
