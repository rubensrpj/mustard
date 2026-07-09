---
name: mustard-feature
description: An internal flow — dispatched by the orchestrator router (CLAUDE.md § Intent Routing), not chosen directly by the user. Feature pipeline for a new entity or a change spanning ≥2 layers (ANALYZE → scope → inline EXECUTE for Light, or PLAN via the full-plan ref for Full). Weak fallback only: use when the router did not engage and the user asks to add, create, or implement a new feature.
source: manual
---
<!-- mustard:generated -->
# /feature — Feature Pipeline

Law: NO code before the approved spec — `scope_guard` refuses it anyway. Full scope stops at PLAN; only `/spec` unlocks EXECUTE, and urgency never changes scope. Red flags to stop on: "spec after the code works"; "scope says full but feels light"; "the gate blocked me, I'll work around it". Rationale: `docs/TEMPLATE-RATIONALE.md`.

> This file is the LIGHT path (most runs) + the shared ANALYZE. The Full-scope machinery lives in `../../../refs/feature/full-plan.md` — read it ONLY when scope detection returns `full`.
> The spec dir (`spec.md` + `meta.json`) is materialised by `spec-draft` at scope detection (§2). It does not exist during research — never reference it before then.

## 1. Understand + research (ANALYZE)

No stage emit here — the slug is born at §2; `spec-draft` backfills the ANALYZE marker.

- Note the client's intent in your own words, plus every concrete critique.
- Ensure the model: `mustard-rt run scan` when `grain.model.json` is absent or materially stale.
- Lapidate the intent YOURSELF before the digest call: strip glue words; translate to code vocabulary; verbs infinitive (`create`, `list`), collection nouns plural (`clients`); a layer-bound task leads with WORK/layer terms (`datatable`, `tooltip`) over domain nouns (`cpf`) that match every layer (→ `../../../refs/locating-code.md`); unsure of a form → include both.
- Call ONCE: `mustard-rt run feature --intent "<lapidated terms + the user's content words>"` — PT hits the code's PT, EN its EN, a wrong guess matches nothing (deterministic, no model call).

| Digest contract | Rule |
|---|---|
| stdout | compact payload (`insumos` fused top-10, `candidates` ~25 with per-file evidence, slices, contracts, hubs, anchors+anchorsDetail, report, stacks, miss, note) — read ONCE, never redirect |
| long tail | already written to `.claude/feature-digest.json` — Read it sliced (`offset`/`limit`); NEVER re-run the command |
| `strong` | SELECT from `candidates` (rule below); `insumos` is the pre-fused top-10 fallback |
| `weak`/`none` | planning fields withheld; read the `vocabulary` menu (payload/detail file), sharpen terms, re-call — a `miss` is NOT "absent"; true net-new is DESIGN |
| confirmed bridge | after a successful re-query or a settled `uncovered` row, persist it: `mustard-rt run equivalence-learn --term <missed> --tokens <bridged>` (learned overlay only; explicit, never automatic) |

- **In-session selection (YOU are the selector — no second model call):** `candidates` lists ~25 files, each with an evidence line (`rank#`/`digest#` positions + matched terms). SELECT the 5-10 a developer would actually open for THIS request, judging by the evidence; prefer production code over migrations/seeds/`loading` skeletons; keep frontend AND backend when the request spans layers. Your selection (never all 25) is what you read and what dispatched agents receive as anchors — never the repo, never `grain.model.json`.
- **Absence radar (`uncovered`):** request concepts with NO representation among the candidates (probe = folded term + its equivalence expansions). These rows are exactly where the pool is BLIND — settle EACH with one targeted Grep/Glob (existence gate) BEFORE planning, and hand the findings to the dispatched agents alongside your selection. Never conclude "doesn't exist" from the pool alone; an empty `uncovered` means every content concept matched SOMETHING, not that the pool is complete. When the gate SETTLES a row — you found which code vocabulary the concept maps to — persist the bridge: `mustard-rt run equivalence-learn --term <concept> --tokens <code-terms>`. The retrieval LEARNS it (learned overlay, survives re-scans); the next query covers it.
- Reading: ONE consolidated Task(Explore) when the survivors fit a single subagent; one Explore per subproject ONLY when anchors genuinely span ≥2 subprojects with volume in each. Each Explore returns ≤40 lines. Direct parent reads only for a single-subproject feature too small for a subagent — sliced, never whole files.
- Composition/enhancement: the `slices` LEAD — a slice names the recurring pattern and carries `exemplarFiles`; go straight to those, treat the selected candidates as the secondary signal. Net-new entity: a sibling's anchors lead instead.
- Glossary grill (optional, non-blocking): `mustard-rt run glossary-coverage --intent "<request>" --context {root}/CONTEXT.md`; only on `missing`/`weak` grill lightly (≤3 central uncovered terms, one batched AskUserQuestion) and persist confirmed pairs via `grill-capture`. `ok`/`na`/absent-tool → silent.
- Specification grill (selective, EARLY — before any §2 ceremony): when the digest stayed `weak`/`none` after the re-query, OR the request names an outcome/symptom without the mechanism → ONE batched AskUserQuestion (2-3 targeted questions offering options inferred from the anchors, didactic). Fold answers into the intent (later the spec's context/AC); maybe one digest re-call. A concrete, well-covered request SKIPS this — the grill never taxes a clear ask.

## 2. Route + scope (deterministic — never your eye alone)

1. Routing economy first: pruned anchors show single-layer work, no new entity → run it as `/mustard:task` on those anchors and STOP.
2. `mustard-rt run spec-draft --intent "<request>" --scope <your light/full read> --lang <bcp47> [--query-terms "<repo terms when raw words were weak/none>"]` — the ONLY scaffold writer; its auto-downgrade gate is the deterministic backstop.
3. `mustard-rt run plan-prepare --from-spec .claude/spec/{slug}/spec.md --slice-match-count <sliceMatchCount from the digest>` — the authority for `scope` (+ decompose/waves) ONLY on a populated census. On `filesSectionEmpty:true` its `scope=light` is unreliable (`fileCount=0`) — keep the `meta.json#scope` `spec-draft` wrote (its gate abstains on an empty census, preserving the routed intent); an empty-census `light` never overrides a requested `full`.
4. `mustard-rt run analyze-validation --spec .claude/spec/{slug}/spec.md` → append `issues[]` to `## Concerns`.
5. Emit `pipeline.scope` + `pipeline.stage: Plan`.
6. `scope="light"` → §3 here. `scope="full"` → open `../../../refs/feature/full-plan.md` and stop reading this file.
7. Digest `concerns` ≥2 → each is its own unit, scoped to its anchors (Full: a wave; light/task: its own dispatch).

Labels for orientation only (plan-prepare decides — but only on a populated census; on `filesSectionEmpty` trust the scope `spec-draft` persisted): light = 1-2 layers, ≤5 files, mirrors a slice | extended-light = matched slice + modifies existing, 6-8 files | full = 3+ layers, net-new, ≥2 slices with ≥2 layers, or >8 files.

## 3. Light/Extended-Light EXECUTE (inline — full never reaches this step)

- Present the plan WITH the approval question: print the spec in the final message AND attach it as the `preview` of the approval options in AskUserQuestion — "Approve and implement?" / "Adjust (give feedback)" / "Save for later (stop)". Never ask about a plan the user has not seen.
- On approve: emit `pipeline.stage: Execute` → `exec-rewave-check` (decomposed → use wave-1 spec) → `dependency-precheck` (block on missing externals) → dispatch via `agent-prompt-render --emit ref` — the 2-line stub stdout IS the Task prompt, passed verbatim; all agents of a wave in one message; each with its role's `subagent_type` → per-wave validate + `memory agent` → REVIEW per subproject (`review-result` emit, max 2 fix loops) → QA (`qa-run`: pass → CLOSE; fail → return the failing AC; skip → warn + allow CLOSE; max 3 iterations).
- Escalations: internal dispatch error → re-dispatch once; still failing → STOP (resume via `/spec`, `../../../refs/spec/resume-loop.md`). CONCERN/BLOCKED/PARTIAL/DEFERRED → same ref, `§ Escalation`. AC cross-shell quirks → `../../../refs/feature/ac-cross-shell.md`.

## Inviolable rules (all scopes)

- Research via the digest — never read the repo or `grain.model.json` whole; reading the pointed files is yours.
- Read only the anchors (~12); follow an anchor's references for composition questions. Settle existence/duplication by Grep enumeration BEFORE any subagent — sampled reading never proves absence (existence gate: `../../../refs/feature/existence-gate.md`).
- Trust each subagent's briefing as the answer (sub-agent contract). Re-read directly ONLY when a conclusion contradicts the user or claims absence — never to re-ground a `file:line` finding.
- The scaffold is materialised only by `spec-draft`. Never hand-write `spec.md`; never Read back a spec/scaffold/`meta.json` you just wrote.
- Prompts only via `agent-prompt-render`; dispatch with the recommended `subagent_type` (`explore`→Explore, `review`/`qa`→mustard-review, `guards`→mustard-guards; writing roles→general-purpose).
- Never skip `analyze-validation` or `dependency-precheck`.
- Emit at each transition — exact commands, there is NO `run emit`: scope → `mustard-rt run emit-pipeline --kind pipeline.scope --spec {slug} --payload <json>`; stage → `mustard-rt run emit-phase --spec {slug} --to {Phase}`.
- Full-scope rules (stops-at-PLAN, the `scope_guard` hard-gate, wave-body authoring, `scan spec` for net-new units): `../../../refs/feature/full-plan.md`.
