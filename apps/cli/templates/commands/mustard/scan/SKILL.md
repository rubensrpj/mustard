---
name: mustard-scan
description: Use when the user runs /scan or asks to analyze, discover, or rescan the codebase. Mines the repo into grain.model.json (deterministic, language-agnostic, no AI) — the durable model the feature/spec pipeline consumes.
source: manual
---
<!-- mustard:generated -->
# /scan — Codebase model

`/scan`, `/scan --root <dir>`, `/scan --out <path>`, `/scan --full`, `/scan --enrich` (opt-in AI: Guards + lexicon).

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

Then run `mustard-rt run lexicon-enrich --check [--root <dir>]` (deterministic, AI-free, read-only)
and, if its `unbridged` list is non-empty, surface a **one-line nudge** — e.g. *"N domain terms have
no lexicon bridge — run `/scan --enrich` to fill them."* This only LISTS the gap; it never calls AI
and never writes. Fail-open: an empty list, no model, or no vendored pair → say nothing.

### Enrich (opt-in, AI): Guards + lexicon

This is the **only** step where `/scan` uses AI, and it runs **only on explicit opt-in** —
`/scan --enrich` (or the user confirming "enrich now"). Without that trigger, `/scan` stays purely
deterministic and cheap. `--enrich` fills **two** things: the subprojects' `## Guards` prose (**A**,
below) and the project lexicon bridges (**B**, after). When opted in:

#### A) Guards

1. **Seed the placeholders.** Run `mustard-rt run scan` (or `scan --full` to regenerate the
   maps too) so subprojects carry a `pending` `## Guards` block. The root is excluded.
2. **Build the worklist.** `mustard-rt run scan-guards-list` emits a JSON array
   `[{path, subproject, kind, frameworks}]` of every subproject `CLAUDE.md` still `pending`
   (root already excluded). Fail-open: on error it returns `[]` (exit 0) — nothing to do.
   Parse it.
3. **Render one prompt per subproject.** For each worklist item:
   `mustard-rt run agent-prompt-render --role guards --subproject <subproject> --emit ref` — this
   path is **spec-less** (no `--spec`): the renderer reads the pending block's facts and derives
   the project's language/tone from `mustard.json`. Pass its stdout to the Task **verbatim** —
   with `--emit ref` that stdout is a 2-line stub the PreToolUse hook expands at dispatch (never
   read the `.dispatch/` file in the parent; that would pay the prompt back into your context);
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

#### B) Lexicon

The lexicon overlay bridges the user's vocabulary onto the code's (e.g. `titulo→payable`), closing the
digest's first-query gap **proactively** (its reactive sibling is `lexicon-suggest`). The split is
deliberate (validated empirically — see memory `project-mustard-domain-vs-plumbing-ranking-ceiling`):
the rt **narrows** the candidates deterministically (no statistic separates a domain term from generic
plumbing INSIDE one project above ~94%), and a cheap **LLM scoring pass** makes the domain-vs-generic
call (~99.7% — Haiku alone). Do NOT eyeball "is this domain?" yourself; that ad-hoc judgement is what
mis-picked `tipo→type` before. The flow:

1. **List the gap (deterministic).** `mustard-rt run lexicon-enrich --check [--root <dir>]` → `{pair,
   language, unbridged: [{term, count, samples}]}` — the mined CODE terms nothing bridges yet,
   **ranked by provenance** (recurring structural role affixes demoted so domain survives the cap).
   Empty `unbridged` or no `pair` → nothing to do, skip.
2. **Score the candidates (the judge — cheap LLM, in orchestration).** `mustard-rt run
   lexicon-judge-render [--root <dir>]` prints a byte-stable prompt asking for a 0-100
   domain-vs-generic score per candidate. Dispatch it to **Haiku** (`Task`, `model: haiku` — fast +
   cheap, already >98%); it returns a single-line JSON `{term: score}`. Keep terms scoring **≥ 60**
   (domain), drop **< 40** (generic plumbing). The **40-59 ambiguous band** is optional: skip it, or
   re-score just that band with **Sonnet** if you want near-100%. Headless / no LLM → fall back to the
   provenance order from step 1 (degraded, ~88%); never block. (The PT direction is symmetric:
   `--check-pt` surfaces `userWord→codeTerm` pairs; score them with the SAME prompt/Haiku pass.)
3. **Propose + apply, gated.** For each KEPT term give the user-side word(s) in `language`, write
   `[{"userWord": "...", "codeTerms": ["..."]}]` to a temp file, and run `mustard-rt run lexicon-enrich
   --apply <file> [--root <dir>]`. The rt validates each target EXISTS as a mined term (rejects
   hallucinations deterministically — it no longer second-guesses domain, the judge already did) and
   writes the valid ones to `.claude/lexicons/<pair>.toml`. Parse `{applied, rejected}` and surface a
   one-line summary (e.g. "added 8 bridges, 1 rejected").

Fail-open: no model, headless, or an empty proposal → skip silently; the digest keeps using the
committed overlay. The lexicon write touches ONLY `.claude/lexicons/` — never the embedded seed,
never source.

## INVIOLABLE RULES

- Default `/scan` produces only `grain.model.json`, never writes into subprojects, and never calls AI; it may *warn* about oversized subproject `CLAUDE.md` files and *list* (never fill) unbridged lexicon terms via the deterministic `lexicon-enrich --check`.
- `--full` only (re)writes a deterministic, lean `CLAUDE.md` map per subproject, preserves hand-written `## Guards`, and seeds a `pending` block where none exists. The deterministic model + render never invoke AI.
- **AI is opt-in.** Only `/scan --enrich` (or explicit user confirmation) invokes AI, for exactly two things: authoring the `## Guards` prose of **subprojects** (never the workspace root, never system prompts; each apply capped ~6 lines, non-destructive) and proposing **lexicon bridges** (each validated against the mined model before any write, into `.claude/lexicons/` only — never the seed, never source). `/scan` without `--enrich` never proposes or writes; at most it runs the deterministic `lexicon-enrich --check` to LIST the gap.
- No confirmation prompts for the deterministic pass — `/scan` is the approval. The enrich step is the lone exception (its opt-in trigger *is* the confirmation).
