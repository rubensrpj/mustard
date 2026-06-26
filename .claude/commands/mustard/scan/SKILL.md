---
name: mustard-scan
description: Use when the user runs /scan or asks to analyze, discover, or rescan the codebase. Mines the repo into grain.model.json (deterministic, language-agnostic) and then enriches it as standard — subproject Guards and method purposes — the durable model the feature/spec pipeline consumes.
source: manual
---
<!-- mustard:generated -->
# /scan — Codebase model

`/scan`, `/scan --root <dir>`, `/scan --out <path>`. **Enrichment is STANDARD** — there is no `--full` or `--enrich` flag. One `/scan` always does the deterministic model, the subproject maps, and the two enrichments (Guards, purpose).

## Process

### 1. Deterministic model + maps (no AI, you do NOT read source)

```bash
mustard-rt run scan --full [--root <dir>] [--out <path>]
```

Always writes `.claude/grain.model.json` — the rich, language-agnostic model (modules, declarations, dependency graph, mined roles, recurring vertical slices, shared contracts, touchpoints, projects) — AND deterministically (re)generates a lean `CLAUDE.md` map per subproject from that model: it **preserves** any hand-written `## Guards`, seeds a `pending` `## Guards` placeholder where none exists, and **never touches the workspace root**. The model is the durable product: downstream (`/feature`, `/bugfix`) consumes it through `mustard-rt run feature` (the digest research step) and `mustard-rt run scan spec` — never by reading the model or the repo directly.

Parse the JSON result (`{ ok, model, regenerated?, oversized? }`); report the model path and surface any `oversized` warnings or `regenerated` paths.

### 2. Enrichment (STANDARD — the AI part of `/scan`)

After the model is built, ALWAYS run the two enrichments below. Each is **incremental** (only the delta since the last scan) and **fail-open** (headless / no LLM / empty worklist → skip that one silently; the deterministic model is already complete and correct). The FIRST scan of a repo pays the one-time enrichment cost; every later scan only does what changed.

#### A) Guards (subproject `## Guards` prose)

1. **Worklist.** `mustard-rt run scan-guards-list` emits a JSON array `[{path, subproject, kind, frameworks}]` of every subproject `CLAUDE.md` still `pending` (root already excluded). Fail-open: on error it returns `[]` (exit 0) — nothing to do. Empty → skip A.
2. **Render one prompt per subproject.** For each worklist item: `mustard-rt run agent-prompt-render --role guards --subproject <subproject> --emit ref` — spec-less (no `--spec`): the renderer reads the pending block's facts and derives the project's language/tone from `mustard.json`. Pass its stdout to the Task **verbatim** (with `--emit ref` it is a 2-line stub the PreToolUse hook expands at dispatch — never read the `.dispatch/` file in the parent); never hand-craft the prompt.
3. **Dispatch in parallel + relay.** Dispatch **one agent per subproject**, `subagent_type` `mustard-guards` (read-only — returns the lines as its final message), all in a **single message** so they run in parallel. Relay each agent's authored guards to `mustard-rt run scan-guards-apply --path <path> --guards -` (text on stdin). The apply is non-destructive (only the block's span changes), capped at ~6 lines, idempotent (flips the marker off `pending`).
4. **Root never enters** — `scan-guards-list` already excludes it.

#### B) Purpose (method-meaning index — the recall enrichment)

The digest finds code by NAME; a method whose name diverges from the request's domain word (e.g. the user asks to *settle* a payable but the method is `WriteOffAsync`) is invisible to it. The `purpose` index closes that — a one-line ENGLISH meaning per logic method, written by a cheap LLM reading the body, that `purpose-search` matches lexically (intra-language English) when the digest judge reports `centralFound=false`. See [[scan-enrich-purpose]].

1. **Worklist (deterministic, incremental).** `mustard-rt run enrich-purpose --render --model .claude/grain.model.json` → `{"lang": "en", "items": [{"id": "path#name#line", "body": "..."}]}` — ONLY the `method`/`function` declarations missing a `purpose` or whose body changed (tracked by `body_hash`). Empty `items` → nothing to do, skip (the common case on a re-scan).
2. **Summarize (cheap LLM, batched + parallel).** Split `items` into batches of ~50. Dispatch one **Haiku** Task per batch (`subagent_type: general-purpose`, `model: haiku`), all in a **single message** so they run in parallel. Each batch prompt: *"You document code methods in ENGLISH. For each `{id, body}` below, infer the BUSINESS ACTION from the LOGIC (status transitions, what it sets/creates/returns, the DTOs/enums it touches) — NOT by paraphrasing the method name — and write ONE concise ENGLISH sentence the way a developer writes a doc-comment, blind to any query. Return ONLY a JSON array `[{\"id\":\"...\",\"purpose\":\"...\"}]`."* Pass the batch's items verbatim.
3. **Apply (deterministic, incremental).** Concatenate every batch's `[{id, purpose}]` into one JSON file and run `mustard-rt run enrich-purpose --apply <file> --model .claude/grain.model.json`. It writes `purpose` + `body_hash`, skipping unchanged bodies. Surface a one-line summary (e.g. "enriched 6,781 method purposes").
4. **Fail-open.** Headless / no LLM → skip; `purpose-search` then returns nothing and the digest degrades to name-only (no error). The first full enrichment is the heavy one-time cost (~a few dollars + minutes for a large repo); incremental keeps every later scan cheap.

## INVIOLABLE RULES

- The **deterministic pass** (model + maps) NEVER calls AI and NEVER reads source; it always writes `grain.model.json` and the lean per-subproject `CLAUDE.md` maps (preserving hand-written `## Guards`, root excluded). It is unconditional.
- **Enrichment (A Guards, B Purpose) is a STANDARD part of every `/scan`** — there is no opt-in flag. Each is **incremental** (only the delta) and **fail-open**: if it cannot run (headless, no LLM, empty worklist) it skips silently, never blocking and never corrupting the model.
- AI in enrichment only ever WRITES two things: subproject `## Guards` prose (never the workspace root, capped ~6 lines, non-destructive), and method `purpose` summaries (into the grain model's declarations, additive, ENGLISH). Never source, never system prompts.
- `/scan` IS the approval — no per-step confirmation prompts.
