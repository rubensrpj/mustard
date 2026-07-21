# /feature — Full-scope DECOMPOSE + PLAN

> You are here because `scope=full`. PLAN is the TERMINAL phase of /feature: materialise the plan, present it, STOP. EXECUTE unlocks ONLY after the user approves via `/spec` — NEVER emit `pipeline.stage: Execute` here. Approval also requires a prior CLARIFY (step 6): `approve-spec` refuses a Full plan until `<spec>/.clarified` exists.

## DECOMPOSE

From the insumos + anchors, split the request into three natures (the judgement grain cannot make):

- Units with precedent → each maps to a matched slice. Only a net-new unit (CREATES an entity that does not yet exist) gets a `mustard-rt run scan spec` compile (`--like <sibling>` when one exists) — a create mold. An enhancement unit (modifies an existing entity) SKIPS `scan spec` and consumes the digest anchors (`context_enrichment`, pre-filled by `spec-draft` into the scaffold `context`).
- Cross-cutting invariants → contracts/hubs the repo already enforces (e.g. an injected `ICurrentTenant`); pass each via `scan spec --invariant <Name>` so the draft anchors the real wiring. NEVER invent the mechanism — mirror the anchored consumers.
- Net-new gaps (no precedent; `miss` after a repo-vocabulary re-query) → surface as a design decision; a `scan spec` draft never implies a precedent-less unit is safe to clone.

Concerns come from the digest — deterministic, no judge. `concerns` ≥2 → each is one unit above and maps to its own wave; a single concern needs no partition.

## Wave decision — 1-vs-N

Invariant: Full ⇒ ≥1 wave (parent = coordination doc, wave = executing subagent). This decides 1-vs-N, never zero — `decompose:false` still yields a single-wave plan (`totalWaves:1`), never a non-wave Full spec.

Reuse the `decompose`/`waves` `plan-prepare` returned in ANALYZE (multi-wave on `fileCount` / `layerCount≥2` / `newEntityCount`, else one). To recompute: `mustard-rt run scope-decompose --from-spec .claude/spec/{slug}/spec.md` (signals from the census; never pipe stdin — the `run` face never receives it).

Validate/derive `depends_on` from the import DAG:
```bash
mustard-rt run wave-dependency --plan plan.json
```
`--plan` reads a FILE — the only reliable transport (the `run` face gets no harness stdin; a pipe dies under `rtk`). Either shape works: `{"files":[…ANALYZE paths],"projectRoot":"."}` or the rich plan JSON below — the SAME plan.json `plan-materialize` takes validates here first. Cases: `cyclic-dependency` → warn + single-wave fallback + note in `## Concerns`; other `error` → fail-open to single wave; `{waves}` of 1 → genuine single layer (net-new with no edges auto-splits by role via `mustard.json#waveLayerOrder`); `{waves}` of 2+ → emit the rich `--plan` JSON and scaffold.

## PLAN — fixed materialisation order

Resolve Lang via cascade (`meta.json#lang` → `mustard.json#specLang` → AskUserQuestion once); hold it for `spec-draft --lang`. Headings, narrative rules, Component Contract: `${CLAUDE_PLUGIN_ROOT}/refs/feature/spec-language.md`. Each artifact exists before the next step consumes it:

1. Lapidate the body (no file yet). Per net-new unit: `mustard-rt run scan spec --entity {Unit} [--like {Sibling}] [--invariant {Contract}] [--ops create,...]` → a draft with the pattern menu, per-project sections, anchors, AC. An enhancement unit skips it, lapidating from the digest anchors. Read draft + anchors + request; resolve the bifurcation, prune, add domain rules, mark assumptions — in the project language/tone (`mustard.json#specLang` / `tone`). The draft's project-unit sections ARE the wave decomposition; hold in context.
2. Materialise the scaffold. `mustard-rt run spec-draft --intent "<request>" --scope full --lang <bcp47> [--waves N] [--query-terms "<repo terms>"]` writes `.claude/spec/{slug}/spec.md` + `meta.json` (slug from `--intent`; `meta.json` is the single lifecycle source; `context` pre-filled with scan anchors). If the raw words came back `weak`/`none` in ANALYZE, pass `--query-terms` with the code-terms that scored strong — else the draft repeats the weak query and withholds the enrichment. The ONLY scaffold writer. Routing gate: `spec-draft` re-classifies its own output and AUTO-DOWNGRADES an unjustified `--scope full` (single-layer, ≤5 files, no net-new) to light, rewriting `meta.json#scope` and reporting `scopeDowngraded:{from,to}` — trust it, return to Light EXECUTE; `--force-scope` honours full (audited as `pipeline.scope.override`). No-op on a placeholder census (`filesSectionEmpty`) — fill `## Files` first. Then fire-and-forget `mustard-rt run digest-adherence-finalize --spec {slug}` (telemetry; never blocks).
3. Fold the body into the plan JSON (never by hand after the scaffold). `Edit` the lapidated bodies into the scaffold Plan-layer sections — never overwrite, so the enriched `context` survives. Each wave carries `tasks`, `files`, `acceptance`. Validate `depends_on` (above), then ONE call: `mustard-rt run plan-materialize --spec-dir <dir> --plan plan.json` — it runs the `wave-scaffold` renderer in-process (each `wave-N-{role}/spec.md` gets `## Tasks`/`## Files`, plus the AC union into `wave-plan.md`) + `analyze-validation` (AC-format WARN) + the `pipeline.scope` emit + emit-phase PLAN. `wave-scaffold` is a renderer, not a published command — this composite is its only entry point. Never run the published steps separately; never hand-author a wave body.
4. Act on validation. Read the `plan-materialize` `validation`; on `ok:false` append `issues[]` to `## Concerns` (non-blocking WARN).
5. Concern Coverage Audit. Every concrete user critique maps to covered by wave/task | non-goal justified | surfaced for decision. Orphans block the approval question.
6. Clarify-finalize (F6). `mustard-rt run grill-capture --finalize --spec {slug}` mints `<spec>/.clarified`, the marker `approve-spec` requires — no term needed, so a complete-glossary spec still finalizes.

Spec layout — canonical section keys (EN, language-agnostic; heading localises per `specLang`): PRD layer `context`, `users`, `metric`, `non-goals`, `acceptance-criteria`; Plan layer `entities`, `files`, optional `component-contract` (UI only), `tasks`, `dependencies`, `boundaries`. Materialisation split: `spec-draft` writes ONLY `spec.md` + `meta.json`; the `wave-scaffold` renderer inside `plan-materialize` owns `wave-plan.md` + per-wave specs.

## Plan JSON schema

`plan-materialize --plan` consumes this (feeding its `wave-scaffold` renderer):
```json
{
  "waves": [
    { "n": 1, "role": "backend", "summary": "one line",
      "depends_on": [],
      "tasks": ["wire the contract"], "files": ["src/api/handler.rs"],
      "acceptance": ["AC-1 — handler returns 200. Command: `curl -sf ...`"] },
    { "n": 2, "role": "frontend", "summary": "...",
      "depends_on": ["wave-1-backend"],
      "tasks": ["render the page"], "files": ["src/ui/page.tsx"],
      "acceptance": ["AC-2 — page renders. Command: `...`"] }
  ],
  "total_waves": 2, "lang": "pt-BR"
}
```
`tasks` / `files` / `acceptance` are optional (a summary-only entry still scaffolds; no `tasks` emits a stderr WARN). ACs are EARS (`when/then` + a behaviour-asserting `Command:`), never a lone build-green; `analyze-validation` (in `plan-materialize`) WARNs on tautological build/test/grep ACs, AC↔wave/file gaps, and a test-shaped `Command:` with no `Expect:` (`test-ac-no-expect`). Optional `Expect: <regex>` (same line as `Command:` or the next): opt-in evidence — on exit 0 `qa-run` also requires the regex to match the combined **stdout+stderr** (a miss fails the AC despite exit 0; a malformed regex fails open). Omit it → legacy exit-0-passes. Trace waves to criteria with `satisfies` (AC ids), else `acceptance` lines — coverage is a gate: `plan-materialize` folds the parent `spec.md` `## Acceptance Criteria` ids into the traced set, so **every parent AC MUST be claimed by ≥1 wave**. An uncovered parent AC makes `plan-materialize` refuse the PLAN (exit 2 + `uncovered_acs`), unconditionally — the coverage gate has no mode knob. An untraced wave is always WARN, never blocking. `depends_on` MUST use the `wave-N-role` form (e.g. `["wave-1-backend"]`), never the bare role — an unresolved dep is dropped silently, flattening the DAG to one level. `plan-materialize` writes `wave-plan.md` (table + the localised AC union under `## Acceptance Criteria`, where QA reads) and each `wave-N-{role}/spec.md` (`## Summary` + `## Network` + materialised `## Tasks`/`## Files`); `agent-prompt-render --spec <wave-dir>` reads those back as the agent `## TASK` + `{reference_files}`. Headings render in the project language — do not hand-localise.

## Present + approve — STOP at PLAN

`plan-materialize` already emitted `pipeline.scope` + PLAN — do not re-emit. Print the spec verbatim + `wave-tree`. NEVER ask about a plan the user cannot see. Primary — plan mode: the wave-plan (+ spec body) IS the plan file; `ExitPlanMode` acceptance mints `<spec>/.approved-by-user` (the marker `approve-spec` requires). Fallback (no plan mode): print the spec, attach `wave-plan.md` as the AskUserQuestion `preview`:

- "Approve wave plan for later" → STOP; user runs `/mustard:spec {letter}` (new session) or `{letter}r` (resume inline right after approving — `r` never skips the approval itself).
- "Edit decomposition (hint PLAN)" → user gives a hint (e.g. merge waves 2 and 3); re-decompose once.
- "Reject decomposition" → `mustard-rt run wave-collapse --spec {spec} --mode full` (the reject path — `${CLAUDE_PLUGIN_ROOT}/refs/spec/resume-loop.md § A`). NEVER a non-wave Full spec.

PLAN is terminal — the only approval that unlocks EXECUTE is the event `/spec` emits (see Inviolable). On "Approve and implement?", direct the user to `/spec`.

## COORDINATE — parent/epic specs

A spec with `children_specs.length > 0` may enter COORDINATE: the orchestrator tracks children, it does NOT implement. `mustard-rt run emit-phase --spec {epic} --to COORDINATE` after linking; when all children reach CLOSE, `mustard-rt run emit-phase --spec {epic} --to CLOSE`.

## Inviolable rules (Full)

- Full STOPS at PLAN and REQUIRES `/spec` before any EXECUTE. /feature must NEVER emit `pipeline.stage: Execute`, dispatch, Edit, or Write production code for a `scope=full` spec — `scope_guard` enforces it.
- The scaffold is materialised ONLY by `mustard-rt run spec-draft`; fold `scan spec` bodies in with `Edit`. NEVER hand-write `spec.md`, NEVER hand-author a wave body after `plan-materialize` renders it — emit it in the plan JSON.
- ALWAYS compile each net-new unit with `mustard-rt run scan spec` (create-only mold); an enhancement unit consumes the digest anchors.
- The locator is 100% deterministic — the digest (and `scan spec` for net-new) is the whole research step; NO judge layer. NEVER dispatch a model to re-rank, partition, or validate it — work from the flat anchors (pruned by provenance) and `concerns` when ≥2.
