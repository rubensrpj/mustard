---
description: An internal flow — dispatched by the orchestrator router (CLAUDE.md § Intent Routing), not chosen directly by the user. Feature pipeline for a new entity or a change spanning ≥2 layers: ANALYZE → scope gate → inline EXECUTE (Light) or PLAN via the full-plan ref (Full). Weak fallback only: use when the router did not engage and the user asks to add, create, or implement a feature.
user-invocable: false
source: manual
---
<!-- mustard:generated -->
# /feature — Feature Pipeline

This file is the LIGHT path (most runs) plus the shared ANALYZE. Full-scope PLAN machinery lives in `${CLAUDE_PLUGIN_ROOT}/refs/feature/full-plan.md` — open it ONLY when scope detection returns `full`.

Law: no code before the approved spec — `scope_guard` refuses it anyway. Full stops at PLAN; only `/spec` unlocks EXECUTE; urgency never changes scope. Full CLARIFIES before approval: the clarify-finalize records WHAT was settled into `<spec>/.clarified` — the terms the grill captured, or the stated reason no grill applied — and `approve-spec` REFUSES a Full plan whose marker recorded neither. The spec dir (`spec.md` + `meta.json`) is born at §2 via `spec-draft` — never reference it during research, and never before the conversation material is assembled (§2.2). Red flags to stop on: "spec after the code works"; "scope says full but feels light"; "the gate blocked me, work around it".

## When

Router dispatched a `feature` kind, or (fallback) the user asks to create / add / implement across ≥2 layers or a new entity. The one fork: single-layer, already-located work is a `/mustard:task`, not a feature — route there and stop.

## 1. ANALYZE — understand + research

No stage emit here; the slug is born at §2 (`spec-draft` backfills the ANALYZE marker). First, audit stale specs: `${CLAUDE_PLUGIN_ROOT}/refs/feature/spec-hygiene.md`.

1. Note the intent in your own words plus every concrete critique.
2. `mustard-rt run scan` when `grain.model.json` is absent or materially stale.
3. **Read the lapidation kit FIRST: `mustard-rt run scan-lapidation`.** It prints how THIS project names things — the mined roles (what a thing is called and where that kind lives), the shapes (roles that recur together, i.e. what a new entity here usually needs) and the units. Lapidating from memory is guessing at a vocabulary the asker had no way to know: measured on a real request, the raw prompt scored 14/32 terms and had its planning fields withheld, while the same request in the project's own words scored 5/5 and pointed straight at the implementing modules. Map the request onto that menu — never copy the menu into the query wholesale, and never invent a term that is not in it. THEN call ONCE: `mustard-rt run feature --intent "<lapidated terms + the request content words>"` (deterministic, no model call). Query-shaping rules: `${CLAUDE_PLUGIN_ROOT}/refs/locating-code.md`.

| Digest field | Rule |
|---|---|
| stdout | compact payload — read ONCE, never redirect |
| long tail | already written to `.claude/feature-digest.json` — Read it sliced (`offset`/`limit`); NEVER re-run the command |
| `strong` | SELECT the 5-10 files a developer would open from `candidates` by their evidence lines — never the whole published list (~12 on a strong report), never the repo or `grain.model.json`; prefer production code over migrations/seeds/skeletons; keep frontend AND backend when the request spans layers. The anchor rows carry no `terms` here: the candidate evidence already does |
| `weak`/`none` | planning fields withheld — read the `vocabulary` menu, sharpen terms, re-call. A `miss` is NOT absent; true net-new is DESIGN |
| `uncovered` (absence radar) | request concepts with NO candidate — settle EACH with one Grep/Glob (existence gate) BEFORE planning; never conclude it does not exist from the pool alone |
| confirmed bridge | after a settled re-query or `uncovered` row: `mustard-rt run equivalence-learn --term <missed> --tokens <code-terms>` (learned overlay, survives re-scans; explicit, never automatic) |

4. Read the survivors (Explore READS the §1.3 anchors, never re-maps): ONE consolidated `Task(Explore)` (≤40 lines each) when they fit one subagent; one per subproject only when anchors span ≥2 subprojects with volume in each; direct sliced parent reads for a single-subproject feature too small for a subagent. Composition/enhancement → the `slices` lead (each names the pattern and carries `exemplarFiles`); net-new entity → the anchors of a sibling lead.
5. Glossary grill: `${CLAUDE_PLUGIN_ROOT}/refs/feature/glossary-grill.md`. RUNNING it stays optional and never blocks — its OUTCOME does not. The outcome is mandatory in exactly one of two shapes: **it ran**, and the terms it settled are named; or **it declined**, and the reason is stated (`glossary-coverage` hands you that sentence verbatim on `verdict:"declined"`). Both shapes are recorded by the same clarify-finalize on the Full path (`refs/feature/full-plan.md` step 6). Silence is not a third shape — a marker that records nothing is what `approve-spec` now refuses.
6. Specification grill (selective, EARLY — before any §2 ceremony): digest still `weak`/`none` after the re-query, or the request names an outcome/symptom without the mechanism → ONE batched AskUserQuestion (2-3 targeted questions, options inferred from the anchors); fold answers into the intent. A concrete, well-covered request skips this.
7. Project memory (judgement, NEVER automatic): before drafting, consult what earlier CLOSED specs recorded — `search_knowledge` / `find_similar_specs` on the `mustard-memory` MCP server (substring match over the `decision`/`lesson` events those specs left behind). Carry a memory into THIS spec only when you judge it relevant to THIS spec, and carry it as ordinary material with its ORIGIN named (`— from spec <slug>`), through the same channel as everything else (§2.2). Nothing relevant → carry nothing; the step is silent, never padded. What makes this safe is that a human-authored inclusion cites where it came from: the automatic injection this project once had was removed for confabulating provenance. Distinguish it from PROCESS memory (`<spec>/memory/*.md`, written by `wave-done` as each wave closes, steering the waves that follow) — that one is intra-run, materialised by the pipeline, and never authored here.

## 2. Route + scope (deterministic — never your eye alone)

1. Routing economy: pruned anchors show single-layer work, no new entity → run it as `/mustard:task` on those anchors and STOP.
2. **Assemble the conversation material FIRST — then materialize. Never the other way round.** A flow that drafts first invites the retype-by-hand this channel exists to remove: what the hand does not retype is simply lost. **Order, said out loud: the base gate — and therefore the unit's `{base}_{slug}` branch — comes BEFORE this write.** The gate is what NAMES the unit, the branch is cut from that name, and this step is an ordinary write, so it belongs inside the unit's branch and not on an integration base. Saying it costs nothing when the auto-branch hook already cuts the branch on this very write (which is what happens when the gate ran first); when it has not, a write from an integration base is refused and the flow dead-ends here with nothing materialised. Write everything §1 established into one JSON file (`.claude/.cache/spec-material.json` — a scratch path; the material's permanent home is the spec `spec-draft` is about to write):
   ```json
   { "definitions": [{"term": "wave", "meaning": "one level of the plan"}],
     "decisions":   [{"decision": "everything branches off dev", "reason": "the release train cuts from it"}],
     "findings":    [{"statement": "the marker is minted unconditionally", "file": "apps/rt/src/commands/grill_capture.rs", "line": 88}] }
   ```
   | Kind | What goes in | Refused |
   |---|---|---|
   | `definitions` | a term the conversation settled + what it means HERE (the grill's captures land here too) | a term with no meaning |
   | `decisions` | a choice + the REASON it was taken — including a project memory you judged relevant (§1.7), with its origin named in the reason | a decision with no reason |
   | `findings` | a verified statement + the `file` (and `line`) it was checked at — a refuted hypothesis is a finding | a statement with no file |

   A FILE, not a flag: the payload carries newlines, quotes and non-ASCII a shell argument would mangle. The channel is FAIL-CLOSED (unlike most of this pipeline) — an unknown key or a half-entry ABORTS the draft with the offending index rather than degrading to an empty channel, because a silent drop is the defect itself. Nothing established → omit `--material` entirely; the draft is then byte-identical to one written before this channel existed.
3. `mustard-rt run spec-draft --intent "<request>" --scope <your light/full read> --lang <bcp47> [--material .claude/.cache/spec-material.json] [--query-terms "<repo terms when raw words were weak/none>"]` — the ONLY scaffold writer; its auto-downgrade gate is the deterministic backstop. Each kind lands in a section of its OWN (`## Definitions` / `## Decisions` / `## Evidence`), never crammed into the prose-only opening — which is why a finding keeps its `file:line` where `## Context` would reject it. The report echoes `material:{definitions,decisions,findings}` counts, so a channel that carried nothing is visible.

   **A `full` scope — the one the report above just recorded in `meta.json`, not your own read (the auto-downgrade gate may have overruled it) — leaves this file HERE: the full path continues in `${CLAUDE_PLUGIN_ROOT}/refs/feature/full-plan.md`, BEFORE step 4.** Its step 2 is the FIRST materialisation and it is ONE call — `spec-draft --plan plan.json` writes `spec.md` + `meta.json` + `wave-plan.md` + every wave directory in the same pass and takes the negative proof there; `plan-materialize` is the RE-materialisation door for a plan that was EDITED, never the first one. Step 4 below cannot help a Full spec, and that is why the fork sits above it and not under it: the census it reads is authored later on this path, out of the lapidated wave bodies folded into the plan JSON, so a call made here reads an empty `## Files` and can only answer `scope:"abstain"` with `filesSectionEmpty:true` — every time, whatever the spec says. Steps 4-7 are the LIGHT path's; on Full the same engines (`analyze-validation`, the negative proof, the `pipeline.scope` + PLAN emits) run in-process inside that one call.
4. `mustard-rt run plan-prepare --from-spec .claude/spec/{slug}/spec.md --slice-match-count <sliceMatchCount from the digest>` — the authority for `scope` (plus decompose/waves) on a populated census. On `filesSectionEmpty:true` it returns `scope:"abstain"` — keep the `meta.json#scope` `spec-draft` wrote; an empty-census read never overrides `full`.
5. `mustard-rt run analyze-validation --spec .claude/spec/{slug}/spec.md` → append `issues[]` to `## Concerns`. It WARNs weak/tautological ACs (a bare `cargo build`/`grep` verifies nothing): ACs are EARS — `when/then` + a behaviour-asserting `Command:`, never a lone build-green.
6. **Prove each criterion can FAIL — `mustard-rt run ac-negative-check --spec .claude/spec/{slug}/spec.md`.** Runs right after the structural validation, and this is the LIGHT path's own step (on Full the materialising call runs the same engine in-process — `spec-draft --plan` on the first pass, `plan-materialize` on a re-materialisation). Each criterion's own `Command:` is executed against the tree AS IT IS NOW — before the work exists — and **clears only by coming back RED**. Green, killed by its deadline, never attempted, or still carrying an unfilled `<…>` placeholder is UNPROVEN. Where the linter above reads command SHAPES and warns, this one runs them and decides: **a criterion that does not fail now does not enter the plan** (exit 2 lists each unproven id with the one action that clears it), and `approve-spec` refuses the spec later on the same ledger. The trailing criterion is exempt — it is the build-green safety net, green by design. The verdict lands in `<spec>/ac-proof.json`, which is what the approval reads instead of re-running anything. To fix an unproven criterion, rewrite its command so it asserts the new behaviour and re-run this; after the artefacts are frozen use `mustard-rt run ac-amend` instead (§3).
7. Emit the transitions (exact commands — there is NO `run emit`): scope → `mustard-rt run emit-pipeline --kind pipeline.scope --spec {slug} --payload <json>`; stage → `mustard-rt run emit-phase --spec {slug} --to Plan`.
8. Route on the effective scope (`meta.json#scope` on `abstain`): `light` → §3; `full` → open `${CLAUDE_PLUGIN_ROOT}/refs/feature/full-plan.md` and stop reading this file. This is the BACKSTOP for a light read the census upgraded — a spec drafted `full` already took the fork under step 3 and never reached here.
9. Digest `concerns` ≥2 → each is its own unit, scoped to its anchors (Full: a wave; light/task: its own dispatch).

Orientation labels (plan-prepare decides on a populated census): light = 1-2 layers, ≤5 files, mirrors a slice · extended-light (internal flow label — emits the canonical scope `light`) = matched slice + modifies existing, 6-8 files · full = 3+ layers, net-new, ≥2 slices with ≥2 layers, or >8 files.

## 3. Light / Extended-Light EXECUTE (inline — Full never reaches here)

- Present the spec WITH the approval question: print it in the final message AND attach it as the `preview` of the AskUserQuestion options — "Approve and implement?" / "Adjust (give feedback)" / "Save for later (stop)". Never ask about a plan the user has not seen.
- On approve: `emit-phase --to Execute` → `exec-rewave-check` (decomposed → use the wave-1 spec) → `dependency-precheck` (block on missing externals) → dispatch via `agent-prompt-render --emit ref` — never hand-craft (stub stdout passed verbatim as the Task prompt; all agents of a wave in one message; each with its role subagent_type) → per-wave validate → REVIEW per subproject (`review-result`, max 2 fix loops) → QA (`qa-run`: pass → CLOSE; fail → return the failing AC; skip → warn + allow CLOSE).
- Prompt render + subagent_type mapping: `${CLAUDE_PLUGIN_ROOT}/refs/agent-prompt/agent-prompt.md`. The dispatch loop itself: `${CLAUDE_PLUGIN_ROOT}/refs/spec/resume-loop.md § B`.

## Inviolable (all scopes)

- Research via the digest; read only the selected anchors (~12), never the repo or `grain.model.json` whole. Settle existence/duplication by Grep enumeration BEFORE any subagent — sampled reading never proves absence: `${CLAUDE_PLUGIN_ROOT}/refs/feature/existence-gate.md`.
- Trust each subagent briefing as the answer; re-read directly ONLY when a conclusion contradicts the user or claims absence.
- The scaffold is materialised ONLY by `spec-draft`; never hand-write `spec.md`; never Read back a spec / `meta.json` you just wrote. What the conversation established rides IN through `--material` (§2.2), assembled before the draft — never retyped into the spec afterwards.
- Prompts only via `agent-prompt-render`; dispatch with the recommended `subagent_type` (`explore`→Explore, `review`/`qa`→`mustard:mustard-review`, `guards`→`mustard:mustard-guards`; writing roles→`general-purpose` — plugin agents namespaced, builtins bare; canonical map: `refs/agent-prompt/agent-prompt.md`).
- Never skip `analyze-validation`, `ac-negative-check` or `dependency-precheck`. Skipping the proof does not postpone it — `approve-spec` refuses on the missing ledger, so the cost is paid at the approval gesture instead of here.
- Flat `.claude/spec/{name}/` layout, lifecycle in `meta.json`, escalation statuses: `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md`.

## Refs

- Full-scope DECOMPOSE + PLAN (stops-at-PLAN, the `scope_guard` hard-gate, wave-body authoring, `scan spec` for net-new units): `${CLAUDE_PLUGIN_ROOT}/refs/feature/full-plan.md`
- Spec headings + narrative language: `${CLAUDE_PLUGIN_ROOT}/refs/feature/spec-language.md`
- AC cross-shell quirks: `${CLAUDE_PLUGIN_ROOT}/refs/feature/ac-cross-shell.md`

## Escalate

Internal dispatch error → re-dispatch once; still failing → STOP (resume via `/spec`). CONCERN / BLOCKED / PARTIAL / DEFERRED → `${CLAUDE_PLUGIN_ROOT}/refs/spec/resume-loop.md § Escalation` (statuses defined in `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Escalation Statuses`).
