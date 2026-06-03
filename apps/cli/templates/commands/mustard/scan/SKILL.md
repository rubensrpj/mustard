---
name: mustard-scan
description: Use when the user runs /scan or asks to analyze, discover, or rescan the codebase. Mines the repo into grain.model.json (deterministic, language-agnostic, no AI) — the durable model the feature/spec pipeline consumes.
source: manual
---
<!-- mustard:generated -->
# /scan — Codebase model

`/scan`, `/scan --root <dir>`, `/scan --out <path>`, `/scan --full`, `/scan --enrich` (opt-in AI Guards).

## Process

One deterministic step — no AI, and you do NOT read source:

```bash
mustard-rt run scan [--root <dir>] [--out <path>] [--full]
```

Always writes `.claude/grain.model.json` — the rich, language-agnostic model (modules,
declarations, dependency graph, mined roles, recurring vertical slices, shared
contracts, touchpoints, projects). It is the durable product: run once per repo,
re-run when the code changes materially. Downstream (`/feature`, `/bugfix`) consumes it
through `mustard-rt run feature` (the digest research step) and `mustard-rt run scan spec`
— never by reading the model or the repo directly.

### Per-subproject CLAUDE.md

- **Default (no `--full`)**: writes NOTHING into subprojects. It only *measures* each
  subproject's `CLAUDE.md`; if one is large enough to weigh on token usage when
  auto-injected, the JSON `oversized[]` lists it and a warning suggests `--full`.
- **`--full`**: deterministically (re)generates a lean `CLAUDE.md` per subproject from the
  grain model (a small orientation map), creating `{subproject}/.claude/` if absent. It
  **preserves** any hand-written `## Guards` section, and for subprojects without one it
  seeds a `## Guards` block with a `<!-- mustard:guards pending -->` … `<!-- /mustard:guards -->`
  placeholder (the hand-off the optional enrich step fills). The workspace **root** is
  excluded — its human-seeded guards are never touched. Still no AI, still no source reading.

Parse the JSON result (`{ ok, model, regenerated?, oversized? }`); report the model path
and surface any `oversized` warnings or `regenerated` paths.

### Enrich the pending Guards (opt-in, AI)

This is the **only** step where `/scan` uses AI, and it runs **only on explicit opt-in** —
`/scan --enrich` (or the user confirming "fill the guards now"). Without that trigger, `/scan`
stays purely deterministic and cheap; it never dispatches an agent. When opted in:

1. **Seed the placeholders.** Run `mustard-rt run scan` (or `scan --full` to regenerate the
   maps too) so subprojects carry a `pending` `## Guards` block. The root is excluded.
2. **Build the worklist.** `mustard-rt run scan-guards-list` emits a JSON array
   `[{path, subproject, kind, frameworks}]` of every subproject `CLAUDE.md` still `pending`
   (root already excluded). Fail-open: on error it returns `[]` (exit 0) — nothing to do.
   Parse it.
3. **Render one prompt per subproject.** For each worklist item:
   `mustard-rt run agent-prompt-render --role guards --subproject <subproject>` — this path is
   **spec-less** (no `--spec`): the renderer reads the pending block's facts and derives the
   project's language/tone from `mustard.json`. Pass its stdout to the Task **verbatim**;
   never hand-craft the prompt.
4. **Dispatch in parallel + relay.** Dispatch **one agent per subproject**, `subagent_type`
   `mustard-guards` (read-only — it has no Edit/Write/Bash, so it cannot write a file; it
   returns the lines as its final message), all in a **single message** so they run in
   parallel. Relay each agent's
   authored guards to `mustard-rt run scan-guards-apply --path <path> --guards -` (text on
   stdin). The apply is non-destructive (only the block's span changes), capped at ~6 lines,
   and idempotent (it flips the marker off `pending`, so a re-run of `scan-guards-list`
   skips it).
5. **Root never enters** — `scan-guards-list` already excludes it, so no agent ever authors
   guards for the workspace root.

## INVIOLABLE RULES

- Default `/scan` produces only `grain.model.json` and never writes into subprojects; it may *warn* about oversized subproject `CLAUDE.md` files.
- `--full` only (re)writes a deterministic, lean `CLAUDE.md` map per subproject, preserves hand-written `## Guards`, and seeds a `pending` block where none exists. The deterministic model + render never invoke AI.
- **AI is opt-in and Guards-only.** Only `/scan --enrich` (or explicit user confirmation) invokes AI, and only to author the `## Guards` prose of **subprojects** — never the workspace root, never system prompts. Each apply is capped (~6 lines) and non-destructive. `/scan` without `--enrich` never calls AI.
- No confirmation prompts for the deterministic pass — `/scan` is the approval. The enrich step is the lone exception (its opt-in trigger *is* the confirmation).
