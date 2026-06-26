# Concern-split judge — shared step (the single AI step in the locator)

Loaded on demand by **every** flow that researches through the scan digest and can face a **multi-concern** request: `/feature` (DECOMPOSE), `/task` (before it splits actions), `/bugfix` (when more than one symptom is reported), and any future digest consumer. **Single source of truth — never copy this prose into a SKILL; reference this file.** (SOLID/DRY: one definition, many references.)

## Why
A free-text request often bundles several independent units of work ("ordenar a datatable; permitir N contatos no parceiro; regras de senha"). The scan digest returns a FLAT anchor list that mixes them, and the deterministic co-occurrence split collapses common terms into one giant blob. The judge separates the request into real **concerns** so each unit is decomposed/dispatched on its own anchors instead of one mixed bag.

## When to run it (gate)
Run the judge ONLY when the digest answer signals MORE THAN ONE unit of work:
- `report.reason: "strong"` AND the answer carries `concerns` with `≥2` entries (the scan's deterministic connected-components split), OR
- the matched concepts plainly span ≥2 unrelated areas.

Single-concern signal (`concerns` absent / one entry, concepts clearly one area) → **SKIP**. One concern needs no partition; running the judge there only burns a dispatch.

## Steps
1. **Render the prompt (deterministic).** `mustard-rt run concern-judge-render --intent "<INTENT>" --model .claude/grain.model.json`. Pure deterministic — no model call inside the rt: it REUSES the feature digest's retrieval, so the concepts + per-concept anchors the judge sees are EXACTLY the ones the digest surfaced, and rides the scan's own split along as the judge's starting point. Stdout = the raw judge prompt; **empty stdout = nothing to partition → skip the judge and fall through to the flat anchors** (never dispatch on an empty prompt).
2. **Dispatch the judge (Haiku).** Pass the rendered prompt VERBATIM as the Task prompt to `subagent_type: general-purpose` with `model: haiku` (the cheap model — a bounded partition, not implementation; this is the ONLY place these flows select a model instead of inheriting the session's). The judge replies with ONLY a JSON array `[{label, concepts, anchors}]`.
3. **Parse + split by concern.** Each object is one concern (`label`, the `concepts` it groups, the `anchors` to read for it). Treat each returned concern as its OWN unit of work — its `anchors` are the files that unit reads, never the whole flat list:
   - `/feature` → each concern is a unit in the three-natures split (a net-new concept ⇒ `scan spec`; an enhancement ⇒ its own anchors) and, in Full scope, maps to a wave.
   - `/task` → each concern is its OWN dispatched action, scoped to its anchors (one render+dispatch per concern), instead of one mixed dispatch.
   - `/bugfix` → each concern is its OWN diagnose+fix, scoped to its anchors.
4. **Deterministic fallback (never block the flow).** If the judge is unavailable, the dispatch errors, or its reply does not parse into a non-empty `[{label,concepts,anchors}]` array (malformed / empty partition), DROP the judge and proceed from the flat pruned `anchors` exactly as you would without it. The judge only ever SHARPENS the partition; its absence degrades to the deterministic anchors, never to a stall — the scan already gave a usable answer.

## INTENT hygiene — the rule that makes or breaks the judge
**`<INTENT>` MUST be the user's actual REQUEST — their own words / phrasing of WHAT they want — the SAME rich intent you passed to the digest (the user's content words + your code-vocabulary translation), NOT a stripped list of isolated code terms.** The judge groups concepts by the user's SEMANTIC meaning; given only bare terms it loses that context and groups by file co-location, which OVER-SPLITS. Field-proven on a real 3-concern request: the user's request → **3 correct concerns**; the bare term soup → **6 fragmented ones**. So never lapidate the user's request away before the judge — the digest's term-matching wants code terms, the judge wants the human request.

## Invariant
The scan digest is **100% deterministic**; the judge is the **ONLY AI step** and lives in the **ORCHESTRATION**, never inside the scan. `concern-judge-render` does pure deterministic assembly (reuses the digest retrieval, emits a byte-stable prompt, calls no model); the JUDGEMENT is the dispatched Haiku agent's, run one layer above the scan. Never push the partition down into the scan tool — the scan's connected-components split is a deterministic FACT the judge refines, not a model call. (Mirrors the `.memory-approved` direction: the judge is one layer above, not within.)
