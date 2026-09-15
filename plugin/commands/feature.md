---
description: An internal flow — dispatched by the orchestrator router (CLAUDE.md § Intent Routing), not chosen directly by the user. Feature pipeline for a new entity or a change spanning ≥2 layers: ANALYZE → scope gate → inline EXECUTE (Light) or PLAN via the full-plan ref (Full). Weak fallback only: use when the router did not engage and the user asks to add, create, or implement a feature.
user-invocable: false
---
<!-- mustard:generated -->
# /feature — Feature Pipeline

This file is the LIGHT path (most runs) plus the shared ANALYZE. Full-scope PLAN machinery lives in `${CLAUDE_PLUGIN_ROOT}/refs/feature/full-plan.md` — open it ONLY when scope detection returns `full`.

Law: no code before the approved spec — `write_gate` refuses it anyway. Full stops at PLAN; only `/spec` unlocks EXECUTE; urgency never changes scope. The spec dir (`spec.md` + `meta.json`) is born at §2 via `spec-draft` — never reference it during research, and never before the conversation material is assembled (§2.2). Red flags to stop on: "spec after the code works"; "scope says full but feels light"; "the gate blocked me, work around it".

## When

Router dispatched a `feature` kind, or (fallback) the user asks to create / add / implement across ≥2 layers or a new entity. The one fork: single-layer, already-located work is a `/mustard:task`, not a feature — route there and stop.

## 1. ANALYZE — understand + research

No stage emit here; the unit's name was minted at the base gate, BEFORE this flow, and §2's `spec-draft` consumes it (and backfills the ANALYZE marker). First, audit stale specs: `${CLAUDE_PLUGIN_ROOT}/refs/feature/spec-hygiene.md`.

1. Note the intent in your own words plus every concrete critique.
2. `mustard-rt run scan` when `grain.model.json` is absent or materially stale.
3. **Call the digest ONCE:** `mustard-rt run feature --intent "<the request content words, in the code's own vocabulary>"` (deterministic, no model call). Query-shaping rules: `${CLAUDE_PLUGIN_ROOT}/refs/locating-code.md`.

| Digest field | Rule |
|---|---|
| stdout | compact payload — read ONCE, never redirect |
| long tail | already written to `.claude/feature-digest.json` — Read it sliced (`offset`/`limit`); NEVER re-run the command |
| `strong` | SELECT the 5-10 files a developer would open from `candidates` by their evidence lines — never the whole published list (~12 on a strong report), never the repo or `grain.model.json`; prefer production code over migrations/seeds/skeletons; keep frontend AND backend when the request spans layers. The anchor rows carry no `terms` here: the candidate evidence already does |
| `weak`/`none` | planning fields withheld — read the `vocabulary` menu, sharpen terms, re-call. A `miss` is NOT absent; true net-new is DESIGN |
| `uncovered` (absence radar) | request concepts with NO candidate — settle EACH with one Grep/Glob (existence gate) BEFORE planning; never conclude it does not exist from the pool alone |

4. Read the survivors (Explore READS the §1.3 anchors, never re-maps): ONE consolidated `Task(Explore)` (≤30 lines each — the cap the rendered explore contract states and the return gate cuts at) when they fit one subagent; one per subproject only when anchors span ≥2 subprojects with volume in each; direct sliced parent reads for a single-subproject feature too small for a subagent. Composition/enhancement → the `slices` lead (each names the pattern and carries `exemplarFiles`); net-new entity → the anchors of a sibling lead.
5. Specification grill (selective, EARLY — before any §2 ceremony): digest still `weak`/`none` after the re-query, or the request names an outcome/symptom without the mechanism → ONE batched AskUserQuestion (2-3 targeted questions, options inferred from the anchors); fold answers into the intent. A concrete, well-covered request skips this.

## 2. Route + scope (deterministic — never your eye alone)

1. Routing economy: pruned anchors show single-layer work, no new entity → run it as `/mustard:task` on those anchors and STOP.
2. **Assemble the conversation material FIRST — then materialize. Never the other way round.** A flow that drafts first invites the retype-by-hand this channel exists to remove: what the hand does not retype is simply lost. **The base gate comes first, and the order lives in one place — `.claude/mustard/dispatch.md`, which states it for every flow.** The gate is what NAMES the unit, the branch is cut from that name, and this step is an ordinary write, so it belongs inside the unit's branch and not on an integration base. No hook cuts a branch on a write any more: with the unit's branch already out this write lands, and from an integration base the `git.flow` declares the write gate refuses it and the flow dead-ends here with nothing materialised. Write everything §1 established into one JSON file (`.claude/.cache/spec-material.json` — a scratch path; the material's permanent home is the spec `spec-draft` is about to write):
   ```json
   { "definitions": [{"term": "wave", "meaning": "one level of the plan"}],
     "decisions":   [{"decision": "everything branches off dev", "reason": "the release train cuts from it"}],
     "findings":    [{"statement": "the marker is minted unconditionally", "file": "apps/rt/src/commands/grill_capture.rs", "line": 88}] }
   ```
   | Kind | What goes in | Refused |
   |---|---|---|
   | `definitions` | a term the conversation settled + what it means HERE (the grill's captures land here too) | a term with no meaning |
   | `decisions` | a choice + the REASON it was taken | a decision with no reason |
   | `findings` | a verified statement + the `file` (and `line`) it was checked at — a refuted hypothesis is a finding | a statement with no file |

   A FILE, not a flag: the payload carries newlines, quotes and non-ASCII a shell argument would mangle. The channel is FAIL-CLOSED (unlike most of this pipeline) — an unknown key or a half-entry ABORTS the draft with the offending index rather than degrading to an empty channel, because a silent drop is the defect itself. Nothing established → omit `--material` entirely; the draft is then byte-identical to one written before this channel existed.
3. `mustard-rt run spec-draft --intent "<request>" --slug <the unit name the base gate reported> --scope <your light/full read> [--material .claude/.cache/spec-material.json | --no-material-reason "<why nothing was established>"] [--query-terms "<repo terms when raw words were weak/none>"]` — the ONLY scaffold writer; its auto-downgrade gate is the deterministic backstop. **`--slug` is the unit's name and it is NOT yours to choose:** copy the `spec` field out of the base gate's own JSON — it may differ from the `--spec` that call was given, and it says so through `renamedFrom`. Omitting it is not fatal (the draft then reads the slug half of the unit's branch, which carries that same name) but passing it puts the one name in the call a reader audits. Each kind lands in a section of its OWN (`## Definitions` / `## Decisions` / `## Evidence`), never crammed into the prose-only opening — which is why a finding keeps its `file:line` where `## Context` would reject it. The report echoes `material:{definitions,decisions,findings}` counts, so a channel that carried nothing is visible. **An empty channel must be a stated choice.** When the conversation established nothing — a re-draft, a mechanical rename — pass `--no-material-reason "<why>"`; the draft REFUSES without either it or `--material`, because a spec that silently lost a conversation's decisions used to look exactly like one that never had any. The reason rides the report as `noMaterialReason`, and the material counts are always echoed, zeroes included.

   **A `full` scope — the one the report above just recorded in `meta.json`, not your own read (the auto-downgrade gate may have overruled it) — leaves this file HERE: the full path continues in `${CLAUDE_PLUGIN_ROOT}/refs/feature/full-plan.md`, BEFORE step 4.** Its step 2 is the FIRST materialisation and it is ONE call — `spec-draft --plan plan.json` writes `spec.md` + `meta.json` + `wave-plan.md` + every wave directory in the same pass and takes the negative proof there; `plan-materialize` is the RE-materialisation door for a plan that was EDITED, never the first one. Step 4 below cannot help a Full spec, and that is why the fork sits above it and not under it: the census it reads is authored later on this path, out of the lapidated wave bodies folded into the plan JSON, so a call made here reads an empty `## Files` and can only answer `scope:"abstain"` with `filesSectionEmpty:true` — every time, whatever the spec says. Steps 4-7 are the LIGHT path's; on Full the same engines (`analyze-validation`, the negative proof, the `pipeline.scope` + PLAN emits) run in-process inside that one call.
4. `mustard-rt run plan-prepare --from-spec .claude/spec/{slug}/spec.md --slice-match-count <sliceMatchCount from the digest>` — the authority for `scope` (plus decompose/waves) on a populated census. On `filesSectionEmpty:true` it returns `scope:"abstain"` — keep the `meta.json#scope` `spec-draft` wrote; an empty-census read never overrides `full`.
5. `mustard-rt run analyze-validation --spec .claude/spec/{slug}/spec.md` → append `issues[]` to `## Concerns`. It WARNs weak/tautological ACs (a bare `cargo build`/`grep` verifies nothing): ACs are EARS — `when/then` + a behaviour-asserting `Command:`, never a lone build-green.
6. Emit the transitions (exact commands — there is NO `run emit`): scope → `mustard-rt run emit-pipeline --kind pipeline.scope --spec {slug} --payload <json>`; stage → `mustard-rt run emit-phase --spec {slug} --to Plan`.
7. Route on the effective scope (`meta.json#scope` on `abstain`): `light` → §3; `full` → open `${CLAUDE_PLUGIN_ROOT}/refs/feature/full-plan.md` and stop reading this file. This is the BACKSTOP for a light read the census upgraded — a spec drafted `full` already took the fork under step 3 and never reached here.
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
- Never skip `analyze-validation` or `dependency-precheck`.
- Flat `.claude/spec/{name}/` layout, lifecycle in `meta.json`, escalation statuses: `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md`.

## Refs

- Full-scope DECOMPOSE + PLAN (stops-at-PLAN, the `write_gate` approval rule, wave-body authoring, `scan spec` for net-new units): `${CLAUDE_PLUGIN_ROOT}/refs/feature/full-plan.md`
- Spec headings + narrative language: `${CLAUDE_PLUGIN_ROOT}/refs/feature/spec-language.md`
- AC cross-shell quirks: `${CLAUDE_PLUGIN_ROOT}/refs/feature/ac-cross-shell.md`

## Escalate

Internal dispatch error → re-dispatch once; still failing → STOP (resume via `/spec`). CONCERN / BLOCKED / PARTIAL / DEFERRED → `${CLAUDE_PLUGIN_ROOT}/refs/spec/resume-loop.md § Escalation` (statuses defined in `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Escalation Statuses`).
