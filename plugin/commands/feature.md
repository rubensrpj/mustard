---
description: An internal flow — dispatched by the orchestrator router (CLAUDE.md § Intent Routing), not chosen directly by the user. Feature pipeline for a new entity or a change spanning ≥2 layers: ANALYZE → scope gate → inline EXECUTE (Light) or PLAN via the full-plan ref (Full). Weak fallback only: use when the router did not engage and the user asks to add, create, or implement a feature.
user-invocable: false
source: manual
---
<!-- mustard:generated -->
# /feature — Feature Pipeline

This file is the LIGHT path (most runs) plus the shared ANALYZE. Full-scope PLAN machinery lives in `${CLAUDE_PLUGIN_ROOT}/refs/feature/full-plan.md` — open it ONLY when scope detection returns `full`.

Law: no code before the approved spec — `scope_guard` refuses it anyway. Full stops at PLAN; only `/spec` unlocks EXECUTE; urgency never changes scope. Full CLARIFIES before approval: after the glossary grill, the clarify-finalize records `<spec>/.clarified`, and `approve-spec` refuses a Full plan without it. The spec dir (`spec.md` + `meta.json`) is born at §2 via `spec-draft` — never reference it during research. Red flags to stop on: "spec after the code works"; "scope says full but feels light"; "the gate blocked me, work around it".

## When

Router dispatched a `feature` kind, or (fallback) the user asks to create / add / implement across ≥2 layers or a new entity. The one fork: single-layer, already-located work is a `/mustard:task`, not a feature — route there and stop.

## 1. ANALYZE — understand + research

No stage emit here; the slug is born at §2 (`spec-draft` backfills the ANALYZE marker). First, audit stale specs: `${CLAUDE_PLUGIN_ROOT}/refs/feature/spec-hygiene.md`.

1. Note the intent in your own words plus every concrete critique.
2. `mustard-rt run scan` when `grain.model.json` is absent or materially stale.
3. Lapidate the intent to code vocabulary YOURSELF, then call ONCE: `mustard-rt run feature --intent "<lapidated terms + the request content words>"` (deterministic, no model call). Lapidation + query-shaping rules: `${CLAUDE_PLUGIN_ROOT}/refs/locating-code.md`.

| Digest field | Rule |
|---|---|
| stdout | compact payload — read ONCE, never redirect |
| long tail | already written to `.claude/feature-digest.json` — Read it sliced (`offset`/`limit`); NEVER re-run the command |
| `strong` | SELECT the 5-10 files a developer would open from `candidates` by their evidence lines — never all ~25, never the repo or `grain.model.json`; prefer production code over migrations/seeds/skeletons; keep frontend AND backend when the request spans layers |
| `weak`/`none` | planning fields withheld — read the `vocabulary` menu, sharpen terms, re-call. A `miss` is NOT absent; true net-new is DESIGN |
| `uncovered` (absence radar) | request concepts with NO candidate — settle EACH with one Grep/Glob (existence gate) BEFORE planning; never conclude it does not exist from the pool alone |
| confirmed bridge | after a settled re-query or `uncovered` row: `mustard-rt run equivalence-learn --term <missed> --tokens <code-terms>` (learned overlay, survives re-scans; explicit, never automatic) |

4. Read the survivors — the §1.3 locator already LOCATED; Explore READS those anchors, never re-maps the repo from scratch: ONE consolidated `Task(Explore)` (≤40 lines each) when they fit one subagent; one per subproject only when anchors span ≥2 subprojects with volume in each; direct sliced parent reads for a single-subproject feature too small for a subagent. Composition/enhancement → the `slices` lead (each names the pattern and carries `exemplarFiles`); net-new entity → the anchors of a sibling lead.
5. Glossary grill (optional, non-blocking): `${CLAUDE_PLUGIN_ROOT}/refs/feature/glossary-grill.md`.
6. Specification grill (selective, EARLY — before any §2 ceremony): digest still `weak`/`none` after the re-query, or the request names an outcome/symptom without the mechanism → ONE batched AskUserQuestion (2-3 targeted questions, options inferred from the anchors); fold answers into the intent. A concrete, well-covered request skips this.

## 2. Route + scope (deterministic — never your eye alone)

1. Routing economy: pruned anchors show single-layer work, no new entity → run it as `/mustard:task` on those anchors and STOP.
2. `mustard-rt run spec-draft --intent "<request>" --scope <your light/full read> --lang <bcp47> [--query-terms "<repo terms when raw words were weak/none>"]` — the ONLY scaffold writer; its auto-downgrade gate is the deterministic backstop.
3. `mustard-rt run plan-prepare --from-spec .claude/spec/{slug}/spec.md --slice-match-count <sliceMatchCount from the digest>` — the authority for `scope` (plus decompose/waves) on a populated census. On `filesSectionEmpty:true` it returns `scope:"abstain"` (the census is not authored yet) — keep the `meta.json#scope` `spec-draft` wrote; an empty-census read never overrides a requested `full`.
4. `mustard-rt run analyze-validation --spec .claude/spec/{slug}/spec.md` → append `issues[]` to `## Concerns`. It WARNs weak/tautological ACs (a bare `cargo build`/`grep` verifies nothing): ACs are EARS — `when/then` + a behaviour-asserting `Command:`, never a lone build-green.
5. Emit the transitions (exact commands — there is NO `run emit`): scope → `mustard-rt run emit-pipeline --kind pipeline.scope --spec {slug} --payload <json>`; stage → `mustard-rt run emit-phase --spec {slug} --to Plan`.
6. Route on the effective scope (`meta.json#scope` when plan-prepare returned `abstain`): `light` → §3; `full` → open `${CLAUDE_PLUGIN_ROOT}/refs/feature/full-plan.md` and stop reading this file.
7. Digest `concerns` ≥2 → each is its own unit, scoped to its anchors (Full: a wave; light/task: its own dispatch).

Orientation labels (plan-prepare decides on a populated census): light = 1-2 layers, ≤5 files, mirrors a slice · extended-light (internal flow label — emits the canonical scope `light`) = matched slice + modifies existing, 6-8 files · full = 3+ layers, net-new, ≥2 slices with ≥2 layers, or >8 files.

## 3. Light / Extended-Light EXECUTE (inline — Full never reaches here)

- Present the spec WITH the approval question: print it in the final message AND attach it as the `preview` of the AskUserQuestion options — "Approve and implement?" / "Adjust (give feedback)" / "Save for later (stop)". Never ask about a plan the user has not seen.
- On approve: `emit-phase --to Execute` → `exec-rewave-check` (decomposed → use the wave-1 spec) → `dependency-precheck` (block on missing externals) → dispatch via `agent-prompt-render --emit ref` — never hand-craft (stub stdout passed verbatim as the Task prompt; all agents of a wave in one message; each with its role subagent_type) → per-wave validate → REVIEW per subproject (`review-result`, max 2 fix loops) → QA (`qa-run`: pass → CLOSE; fail → return the failing AC; skip → warn + allow CLOSE).
- Prompt render + subagent_type mapping: `${CLAUDE_PLUGIN_ROOT}/refs/agent-prompt/agent-prompt.md`. The dispatch loop itself: `${CLAUDE_PLUGIN_ROOT}/refs/spec/resume-loop.md § B`.

## Inviolable (all scopes)

- Research via the digest; read only the selected anchors (~12), never the repo or `grain.model.json` whole. Settle existence/duplication by Grep enumeration BEFORE any subagent — sampled reading never proves absence: `${CLAUDE_PLUGIN_ROOT}/refs/feature/existence-gate.md`.
- Trust each subagent briefing as the answer; re-read directly ONLY when a conclusion contradicts the user or claims absence.
- The scaffold is materialised ONLY by `spec-draft`; never hand-write `spec.md`; never Read back a spec / `meta.json` you just wrote.
- Prompts only via `agent-prompt-render`; dispatch with the recommended `subagent_type` (`explore`→Explore, `review`/`qa`→`mustard:mustard-review`, `guards`→`mustard:mustard-guards`; writing roles→general-purpose — plugin agents namespaced, builtins bare; canonical map: `refs/agent-prompt/agent-prompt.md`).
- Never skip `analyze-validation` or `dependency-precheck`.
- Flat `.claude/spec/{name}/` layout, lifecycle in `meta.json`, escalation statuses: `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md`.

## Refs

- Full-scope DECOMPOSE + PLAN (stops-at-PLAN, the `scope_guard` hard-gate, wave-body authoring, `scan spec` for net-new units): `${CLAUDE_PLUGIN_ROOT}/refs/feature/full-plan.md`
- Spec headings + narrative language: `${CLAUDE_PLUGIN_ROOT}/refs/feature/spec-language.md`
- AC cross-shell quirks: `${CLAUDE_PLUGIN_ROOT}/refs/feature/ac-cross-shell.md`

## Escalate

Internal dispatch error → re-dispatch once; still failing → STOP (resume via `/spec`). CONCERN / BLOCKED / PARTIAL / DEFERRED → `${CLAUDE_PLUGIN_ROOT}/refs/spec/resume-loop.md § Escalation` (statuses defined in `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Escalation Statuses`).
