---
name: mustard-scan
description: Use when the user runs /scan or asks to analyze, discover, or rescan the codebase. Mines the repo into grain.model.json (deterministic, language-agnostic) and then enriches it as standard — subproject Guards, lexicon bridges, and method purposes — the durable model the feature/spec pipeline consumes.
source: manual
---
<!-- mustard:generated -->
# /scan — Codebase model

`/scan`, `/scan --root <dir>`, `/scan --out <path>`. **Enrichment is STANDARD** — there is no `--full` or `--enrich` flag. One `/scan` always does the deterministic model, the subproject maps, and the three enrichments (Guards, lexicon, purpose).

## Process

### 1. Deterministic model + maps (no AI, you do NOT read source)

```bash
mustard-rt run scan --full [--root <dir>] [--out <path>]
```

Always writes `.claude/grain.model.json` — the rich, language-agnostic model (modules, declarations, dependency graph, mined roles, recurring vertical slices, shared contracts, touchpoints, projects) — AND deterministically (re)generates a lean `CLAUDE.md` map per subproject from that model: it **preserves** any hand-written `## Guards`, seeds a `pending` `## Guards` placeholder where none exists, and **never touches the workspace root**. The model is the durable product: downstream (`/feature`, `/bugfix`) consumes it through `mustard-rt run feature` (the digest research step) and `mustard-rt run scan spec` — never by reading the model or the repo directly.

Parse the JSON result (`{ ok, model, regenerated?, oversized? }`); report the model path and surface any `oversized` warnings or `regenerated` paths.

### 2. Enrichment (STANDARD — the AI part of `/scan`)

After the model is built, ALWAYS run the three enrichments below. Each is **incremental** (only the delta since the last scan) and **fail-open** (headless / no LLM / empty worklist → skip that one silently; the deterministic model is already complete and correct). The FIRST scan of a repo pays the one-time enrichment cost; every later scan only does what changed.

#### A) Guards (subproject `## Guards` prose)

1. **Worklist.** `mustard-rt run scan-guards-list` emits a JSON array `[{path, subproject, kind, frameworks}]` of every subproject `CLAUDE.md` still `pending` (root already excluded). Fail-open: on error it returns `[]` (exit 0) — nothing to do. Empty → skip A.
2. **Render one prompt per subproject.** For each worklist item: `mustard-rt run agent-prompt-render --role guards --subproject <subproject> --emit ref` — spec-less (no `--spec`): the renderer reads the pending block's facts and derives the project's language/tone from `mustard.json`. Pass its stdout to the Task **verbatim** (with `--emit ref` it is a 2-line stub the PreToolUse hook expands at dispatch — never read the `.dispatch/` file in the parent); never hand-craft the prompt.
3. **Dispatch in parallel + relay.** Dispatch **one agent per subproject**, `subagent_type` `mustard-guards` (read-only — returns the lines as its final message), all in a **single message** so they run in parallel. Relay each agent's authored guards to `mustard-rt run scan-guards-apply --path <path> --guards -` (text on stdin). The apply is non-destructive (only the block's span changes), capped at ~6 lines, idempotent (flips the marker off `pending`).
4. **Root never enters** — `scan-guards-list` already excludes it.

#### B) Lexicon (vocabulary bridges)

The lexicon overlay bridges the user's vocabulary onto the code's (e.g. `titulo→payable`), closing the digest's first-query gap proactively. The rt **narrows** the candidates deterministically (no statistic separates a domain term from generic plumbing inside one project above ~94% — see memory `project-mustard-domain-vs-plumbing-ranking-ceiling`); a cheap **LLM scoring pass** makes the domain-vs-generic call (~99.7% — Haiku alone). Do NOT eyeball "is this domain?" yourself — that ad-hoc judgement mis-picked `tipo→type` before.

1. **List the gap (deterministic).** `mustard-rt run lexicon-enrich --check [--root <dir>]` → `{pair, language, unbridged: [{term, count, samples}]}` — the mined CODE terms nothing bridges yet, **ranked by provenance** (recurring structural role affixes demoted so domain survives the cap). Empty `unbridged` or no `pair` → skip B.
2. **Score the candidates (the judge — cheap LLM).** `mustard-rt run lexicon-judge-render [--root <dir>]` prints a byte-stable prompt asking for a 0-100 domain-vs-generic score per candidate. Dispatch it to **Haiku** (`Task`, `model: haiku`); it returns single-line JSON `{term: score}`. Keep terms scoring **≥ 60**, drop **< 40**. The **40-59 band** is optional (skip, or re-score with Sonnet for near-100%). Headless → fall back to the provenance order from step 1 (degraded ~88%); never block. (PT direction is symmetric: `--check-pt` surfaces `userWord→codeTerm` pairs; score with the SAME Haiku pass.)
3. **Propose + apply, gated.** For each KEPT term give the user-side word(s) in `language`, write `[{"userWord": "...", "codeTerms": ["..."]}]` to a temp file, run `mustard-rt run lexicon-enrich --apply <file> [--root <dir>]`. The rt validates each target EXISTS as a mined term (rejects hallucinations deterministically) and writes the valid ones to `.claude/lexicons/<pair>.toml`. Parse `{applied, rejected}`; surface a one-line summary.

#### C) Purpose (method-meaning index — the recall enrichment)

The digest finds code by NAME; a method whose name diverges from the user's words (PT `efetivar` → `EffectivateAsync`, `dar baixa` → `WriteOffAsync`) is invisible to it (field-measured: name-match recall **0/10**, purpose-search **10/10**). The `purpose` index closes that — a one-line meaning per logic method, written by a cheap LLM reading the body, that `purpose-search` matches lexically when the digest judge reports `centralFound=false`. See [[scan-enrich-purpose]].

1. **Worklist (deterministic, incremental).** `mustard-rt run enrich-purpose --render --model .claude/grain.model.json` → `{"lang": "...", "items": [{"id": "path#name#line", "body": "..."}]}` — ONLY the `method`/`function` declarations missing a `purpose` or whose body changed (tracked by `body_hash`). Empty `items` → nothing to do, skip (the common case on a re-scan).
2. **Summarize (cheap LLM, batched + parallel).** Split `items` into batches of ~50. Dispatch one **Haiku** Task per batch (`subagent_type: general-purpose`, `model: haiku`), all in a **single message** so they run in parallel. Each batch prompt (use the `lang` from the worklist): *"You document code methods in `<lang>`. For each `{id, body}` below, infer the BUSINESS ACTION from the LOGIC (status transitions, what it sets/creates/returns, the DTOs/enums it touches) — NOT by translating the English method name — and write ONE concise `<lang>` sentence the way a developer writes a doc-comment. Return ONLY a JSON array `[{\"id\":\"...\",\"purpose\":\"...\"}]`."* Pass the batch's items verbatim.
3. **Apply (deterministic, incremental).** Concatenate every batch's `[{id, purpose}]` into one JSON file and run `mustard-rt run enrich-purpose --apply <file> --model .claude/grain.model.json`. It writes `purpose` + `body_hash`, skipping unchanged bodies. Surface a one-line summary (e.g. "enriched 6,781 method purposes").
4. **Fail-open.** Headless / no LLM → skip; `purpose-search` then returns nothing and the digest degrades to name-only (no error). The first full enrichment is the heavy one-time cost (~a few dollars + minutes for a large repo); incremental keeps every later scan cheap.

## INVIOLABLE RULES

- The **deterministic pass** (model + maps) NEVER calls AI and NEVER reads source; it always writes `grain.model.json` and the lean per-subproject `CLAUDE.md` maps (preserving hand-written `## Guards`, root excluded). It is unconditional.
- **Enrichment (A Guards, B Lexicon, C Purpose) is a STANDARD part of every `/scan`** — there is no opt-in flag. Each is **incremental** (only the delta) and **fail-open**: if it cannot run (headless, no LLM, empty worklist) it skips silently, never blocking and never corrupting the model.
- AI in enrichment only ever WRITES three things: subproject `## Guards` prose (never the workspace root, capped ~6 lines, non-destructive), lexicon bridges (validated against the mined model, into `.claude/lexicons/` only), and method `purpose` summaries (into the grain model's declarations, additive). Never the embedded seed, never source, never system prompts.
- `/scan` IS the approval — no per-step confirmation prompts.
