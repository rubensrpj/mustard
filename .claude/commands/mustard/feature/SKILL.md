---
name: mustard-feature
description: Use when the user runs /feature or asks to add, create, implement, or enhance a new feature. Starts the feature pipeline (ANALYZE → PLAN → optional inline EXECUTE for Light scope).
source: manual
---
<!-- mustard:generated -->
# /feature - Feature Pipeline

`/feature <request>` — understand the client, research the repo through the deterministic `scan` tool (never reading source by hand), then plan + implement.
Heavy lifting delegates to `mustard-rt`; the orchestrator routes phases + emits events. You (the AI) do the reasoning that grain cannot: elicitation, decomposition, lapidation.

> **Artifact order — read before editing the flow.** The spec dir + `spec.md` + `meta.json` are MATERIALISED by `mustard-rt run spec-draft` in PLAN (step 2). They do **not** exist during ANALYZE. The slug is born from `--intent` at that step. Never reference `.claude/spec/{slug}/spec.md` (or run a command that reads it) before it has been materialised.

## Action

### 1. Understand + RESEARCH (ANALYZE)

→ `../../../refs/feature/spec-hygiene.md`. Emit `pipeline.stage: Analyze`.

- **Note the client's intent** in your own words (what they actually want, plus every concrete critique from the conversation).
- **Ensure the model exists/is fresh**: `mustard-rt run scan` produces `.claude/grain.model.json` (the durable repo model). Run it if absent or the codebase changed materially.
- **Research via the scan digest, not the repo**: `mustard-rt run feature --intent "<request>"` → structured *insumos* (`matchedTerms`, `slices`, `contracts`, `hubs`, `anchors`, `stacks` — detected stacks with confidence + the signals that grounded them, use to anchor framework-specific decisions — `miss`, `note`). Re-query with **repo-vocabulary** terms on a `miss` (the term index has no synonyms and false negatives — e.g. a PT request maps to EN code terms). NEVER conclude "absent" from a `miss` alone.
- **Read ONLY the `anchors`** the insumos point to (~12 real files) — never the whole repo, never `grain.model.json` directly. This is the low-consumption contract. **When the anchors span ≥2 subprojects, do NOT read them in the parent.** Every file you open in a subproject auto-injects that subproject's `CLAUDE.md` + skill catalog into your context — the most expensive, longest-lived one — so reading across N subprojects pays that fixed overhead N×, re-sent on every later turn of ANALYZE+PLAN. Instead dispatch **one `Task(Explore)` per subproject**; each reads only that subproject's anchors and returns ≤40 lines (the pattern to mirror, files to touch, contract wiring). Reserve direct parent reads for a single-subproject feature, and read only the slice you need (`offset`/`limit`), never whole files.
- **Glossary nudge (optional, non-blocking)** → `../../../refs/feature/glossary-nudge.md`. After the digest, run `mustard-rt run glossary-coverage --intent "<request>" --context {root}/CONTEXT.md`; ONLY on `verdict ∈ {missing, weak}` surface ONE dismissible suggestion to sharpen the domain glossary via the `grill-with-docs` skill before planning. It never blocks, never grills inline, and writes nothing itself; `ok` / `na` / absent-tool → stay silent and continue.

Scope is decided by the tool, not eyeballed. Once the spec scaffold exists (PLAN step 2 materialises it), run `mustard-rt run scope-classify --from-spec .claude/spec/{slug}/spec.md --slice-match-count <sliceMatchCount from the feature digest>` and use the returned `scope` as the decision. The three labels — light (1-2 layers, ≤5 files, mirrors a matched slice) | extended-light (matched slice + modifies existing, 6-8 files) | full (3+ layers, net-new, or spans ≥2 slices, or >8 files) — are context only; the DECISION is the command's `scope` output, not your judgement. Feed it into `spec-draft --scope`: pass `full` and `light` verbatim; `extended-light` collapses to `--scope light` for the draft (PLAN is skipped either way), but you keep the `extended-light` label to govern the §4 inline EXECUTE (its higher file ceiling). MAX 5 reads beyond the anchors in ANALYZE.

ANALYZE ends at scope detection. There is **no spec file yet**, so nothing is validated here — `analyze-validation` runs in PLAN (step 3, inside `plan-materialize`), after the scaffold exists.

### 2. DECOMPOSE

From the insumos + anchors, split the request into three natures (this is the judgement grain cannot make):

- **Units with precedent** → each maps to a matched slice. Only a **net-new** unit (it CREATES an entity that does not exist yet) gets a `scan spec` compile (with `--like <sibling>` when one exists) — the compiler emits a *create* mold. An **enhancement** unit (modifies an existing entity/behavior) skips `scan spec` and consumes the feature digest's anchors instead (the `context_enrichment` that `spec-draft` pre-fills into the scaffold's `context` section).
- **Cross-cutting invariants to obey** → contracts/hubs the repo already enforces (e.g. an injected `ICurrentTenant`); pass each via `scan spec --invariant <Name>` so the draft anchors the real wiring. NEVER invent the mechanism — mirror the anchored consumers.
- **Net-new gaps** (no precedent; `miss` after a repo-vocabulary re-query) → surface as a design decision; do not let a `scan spec` draft's "deterministic" framing imply a unit that has no precedent is safe to clone.

### 3. PLAN

→ `../../../refs/feature/spec-language.md` (header translation, narrative rules, Component Contract). → `../../../refs/feature/wave-decomposition.md`.

Resolve Lang via cascade (`meta.json#lang` → `mustard.json#specLang` → AskUserQuestion once) — hold the resolved value for `spec-draft --lang` (step 2 persists it to `meta.json`).

PLAN materialises the spec in a **fixed order** — every artifact exists before the next step consumes it:

1. **Lapidate the body (no file yet).** Per **net-new** unit (creation): `mustard-rt run scan spec --entity {Unit} [--like {Sibling}] [--invariant {Contract}] [--ops create,...]` → a draft carrying the auto-chosen pattern menu, per-project sections, the anchors, and acceptance criteria. An **enhancement** unit skips `scan spec` (the compiler only emits a *create* mold) — lapidate it from the feature digest's anchors (the `context_enrichment` `spec-draft` folds into the scaffold's `context` section). Read the draft + its anchors + the client request; resolve the bifurcation, prune, add domain rules, mark assumptions — **in the project's language/tone** (`mustard.json#specLang`/`tone`). The draft's project-unit sections ARE the wave/agent decomposition. Hold the lapidated body in context; do **not** write a file here.
2. **Materialise the scaffold.** `mustard-rt run spec-draft --intent "<request>" --scope {light|full} --lang <bcp47> [--waves N]` writes `.claude/spec/{slug}/spec.md` + `meta.json` (slug born from `--intent`; `meta.json` is the single lifecycle source; the `context` section is pre-filled with the scan anchors/slices). This is the **only** writer of the spec scaffold — never hand-write `spec.md` with the Write tool.
3. **Fold the body in — into the plan JSON, not by hand after the scaffold.** `Edit` the lapidated bodies from step 1 into the scaffold's parent Plan-layer sections (`entities`, `files`, optional `component-contract` UI-only, `tasks`, `dependencies`, `boundaries`). Edit — never overwrite — so the digest-enriched `context` section survives. **Full scope sempre tem ≥1 wave** (`>= 1 wave`): the parent spec is the orchestrator/coordination doc (no own `tasks`/checklist), the wave is the executing subagent. `scope_decompose` decides **1-vs-N** waves — never 0-vs-≥1; "reject decomposition" collapses to a single wave for Full, never to zero (see `refs/spec/approve-only-flow.md`). Do not hard-code thresholds here — ask the authority: `mustard-rt run scope-decompose` (multi-wave on its signals — multi-layer, roadmap, history, wide+new-entity; otherwise a single wave). The lapidated `scan spec` project-unit decomposition becomes the **per-wave body of the plan JSON** — each wave carries `tasks` (checklist), `files` (census), and `acceptance` (AC) arrays. **Before materialising**, validate/derive the plan's `depends_on` with `mustard-rt run wave-dependency < plan.json` (it reads the plan JSON from **stdin**; stdin does not survive the `rtk` wrapper — invoke it plain or via file-based shell redirection as shown, never through an `rtk`-wrapped pipe). Then materialise with **ONE call**: `mustard-rt run plan-materialize --spec-dir <dir> --plan <plan.json>` — it composes `wave-scaffold` (each `wave-N-{role}/spec.md` gets `## Tasks`/`## Files` in the project language, plus the AC union into `wave-plan.md`) + `analyze-validation` (incl. the AC-format WARN) + the `pipeline.scope` emit + emit-phase PLAN, returning `{events, scaffold, validation}`. Do NOT run those as separate manual steps. NEVER hand-author a wave's `spec.md` body after the scaffold — emit it in the plan JSON. See `refs/feature/wave-decomposition.md` for the `--plan` schema.
4. **Act on the validation result.** `plan-materialize` (step 3) already ran `analyze-validation` — read its `validation` output and append `issues[]` to `## Concerns` on `ok: false` (non-blocking, WARN-level). Only a Light spec with no plan JSON (so no `plan-materialize`) still runs `mustard-rt run analyze-validation --spec .claude/spec/{slug}/spec.md` directly.
5. **Concern Coverage Audit.** Every concrete user critique must map to covered by wave/task | non-goal justified | surfaced for decision. Orphaned items block the AskUserQuestion.

Spec layout — **canonical section keys** (EN, language-agnostic); the rendered heading localises per `mustard.json#specLang` (e.g. `context` → `## Context` / `## Contexto`): **PRD layer** — `context`, `users`, `metric`, `non-goals`, `acceptance-criteria`; **Plan layer** — `entities`, `files`, optional `component-contract` (UI only), `tasks`, `dependencies`, `boundaries`.

`plan-materialize` already emitted `pipeline.scope` + the PLAN phase — do not re-emit them (Light, no plan JSON: emit `pipeline.scope` + `pipeline.stage: Plan` yourself). Print spec verbatim + `wave-tree`. AskUserQuestion: **"Approve and implement?"** / **"Adjust (give feedback)"** / **"Save for later (stop)"**.

> **Full scope STOPS here.** For `scope=full`, PLAN is the terminal phase of `/feature` — never proceed to EXECUTE inline, regardless of the user's answer. Approval is granted **only** by running `/spec` (which emits the canonical approval event the hard-gate checks); the `scope_guard` hook denies any production-file Edit/Write until then. On "Approve and implement?" for Full scope, direct the user to `/spec` to approve and dispatch — do **not** emit `pipeline.stage: Execute` yourself. Only Light / Extended-Light continue to §4.

> **Materialisation split (no overlap).** `spec-draft` writes ONLY the top-level `spec.md` + `meta.json` (scope/totalWaves/isWavePlan); `wave-scaffold` is the sole owner of the wave breakdown (`wave-plan.md` + per-wave specs + review/qa, plan-driven). Full scope = `spec-draft` (step 2) then `plan-materialize` (step 3 — the single call that runs `wave-scaffold`).

### 4. Light/Extended-Light EXECUTE (inline)

**Light / Extended-Light only.** Full scope never reaches this step — it stopped at PLAN (see the gate above) and resumes through `/spec`. This inline path applies solely when `scope=light` or `scope=extended-light`.

User chooses "Approve and implement now": emit `pipeline.stage: Execute` → `exec-rewave-check` (decomposed → use wave-1 spec) → `dependency-precheck` (block on missing externals) → dispatch agents via `agent-prompt-render` (NEVER hand-craft; all agents of a wave → one message; the subagent's context is the spec's project section + its anchors; dispatch each with its role's `subagent_type` — `review`→`mustard-review` (read-only), `impl`→`general-purpose`) → per-wave validate + `memory agent` → REVIEW per subproject (`review-result` emit, max 2 fix loops) → QA (`qa-run`; pass → CLOSE; fail → return failing AC; skip → warn + allow CLOSE; max 3 QA iterations).

Escalations: `CONCERN` → `## Concerns`, continue. `BLOCKED` → STOP + AskUserQuestion. `PARTIAL` → granular retry (max 2). `DEFERRED` → note + confirm. → `../../../refs/feature/ac-cross-shell.md`.

## INVIOLABLE RULES

- **Full scope STOPS at PLAN and REQUIRES `/spec` to approve before any EXECUTE.** `/feature` must NEVER emit `pipeline.stage: Execute` — nor dispatch, Edit, or Write production code — for a `scope=full` spec. The only approval that unlocks EXECUTE is the canonical event emitted by `/spec`; the `scope_guard` hard-gate enforces this. The inline §4 EXECUTE path is **Light / Extended-Light only**.
- ALWAYS research via `mustard-rt run feature` (the scan digest) — NEVER read the repo or `grain.model.json` to understand it.
- READ ONLY the `anchors` the scan tools point to (~12 files). NEVER bulk-read source. When anchors cross ≥2 subprojects, delegate the reading to one `Task(Explore)` per subproject (keeps each subproject's `CLAUDE.md`+skill catalog out of the parent context); read slices via `offset`/`limit`, never whole files.
- The spec scaffold (`spec.md` + `meta.json`) is materialised ONLY by `mustard-rt run spec-draft`; fold lapidated `scan spec` bodies in with `Edit`. NEVER hand-write `spec.md` with the Write tool.
- NEVER hand-author a wave's body after `wave-scaffold` — emit it in the plan JSON's per-wave body (`tasks` / `files` / `acceptance`). `wave-scaffold` materialises `## Tasks`/`## Files` into each wave spec and the AC union into `wave-plan.md`; editing a wave's `spec.md` body by hand is PLAN work leaking into the scaffold step.
- `analyze-validation` runs in PLAN, AFTER `spec-draft` wrote `spec.md` (it `exit(1)`s on a missing file) — NEVER at the end of ANALYZE. With a plan JSON, `plan-materialize` executes it for you — do not call it as a separate step there.
- NEVER Read back a spec, scaffold, or `meta.json` you just wrote or edited yourself — the content is already in context and Write/Edit confirmed success; re-reading is pure token round-trip. (Reading a *tool-generated* draft from `scan spec` for the first time is fine — that content is new to you.)
- NEVER hand-craft agent prompts — always `agent-prompt-render`; the subagent's context is the spec section + anchors. Dispatch each agent with the `subagent_type` the tool recommends per role (read-only roles run tool-restricted: `explore`→`Explore`, `review`/`qa`→`mustard-review`, `guards`→`mustard-guards`; writing roles → `general-purpose`).
- ALWAYS compile each **net-new** unit's draft with `mustard-rt run scan spec` (its mold is create-only); an enhancement unit skips `scan spec` and consumes the feature digest's anchors (`context_enrichment`). Then lapidate in the project's language (`mustard.json#specLang`/`tone`).
- A `miss` is NOT "absent": re-query with repo-vocabulary terms; treat true net-new as DESIGN, not recomposition.
- NEVER skip `analyze-validation` or `dependency-precheck`.
- ALWAYS emit `pipeline.scope` + `pipeline.stage` at each transition.
