# Digest validation — the shared AI step after the scan digest

Loaded on demand by **every** flow that researches through the scan digest: `/feature`, `/task`, `/bugfix`. **Single source of truth — never copy this prose into a SKILL; reference this file.** (SOLID/DRY: one definition, many references.)

## Why
The scan is deterministic: it LOCATES, it cannot JUDGE. Two judgements it cannot make, but a model can from the digest answer alone:
1. **Is an anchor a REAL target or an incidental lexical match?** A UI request matching a backend credit-card file on the bare word `card` is noise — and no deterministic score removes it (field-proven: a `stratum_floor` A/B left the anchors identical, because the noise is *meaning*, not rank).
2. **Does the work actually need the feature pipeline, or is it a lean `/task`? And if a feature — light or full?** The deterministic `layerCount` OVER-counts: it spans every project an anchor touches, including the incidental ones.

So one model step, ONE layer above the scan, validates the answer before any flow acts on it.

## When to run it (gate)
Run it after the digest returns a usable answer (`report.reason` ∈ {`strong`, `weak`}). **SKIP** when the render is empty (no concept matched — nothing to validate) or the digest `miss`ed entirely after the repo-vocabulary re-query (true net-new → treat as DESIGN, not recomposition).

## Steps
1. **Render the prompt (deterministic).** `mustard-rt run digest-validate-render --intent "<INTENT>" --model .claude/grain.model.json`. Pure deterministic — no model call inside the rt: it REUSES the feature digest's retrieval AND tags each anchor with its project (`read_projects`), so the validator sees EXACTLY the concepts/anchors the digest surfaced, each with its layer. Stdout = the raw prompt; **empty stdout = skip the validation and fall through to the flat pruned anchors** (never dispatch on an empty prompt).
2. **Dispatch the validator (Sonnet).** Pass the rendered prompt VERBATIM as the Task prompt to `subagent_type: general-purpose` with `model: sonnet`. **Sonnet, not Haiku:** this is ONE routing-critical call per pipeline entry (not a fan-out), so accuracy outweighs the negligible per-call cost — a wrong `full` is the most expensive routing error. The validator replies with ONLY a JSON object:
   `{"route":"task|feature","scope":"light|full|","dropped":["<file>"],"concerns":[{"label":"<short>","concepts":["<concept>"],"anchors":["<file>"]}]}`
3. **Act on the verdict.**
   - **`dropped`** → remove those anchors from what you read; they are incidental (a far-layer / ambiguous-term match). Never open them.
   - **`route` = `"task"`** → the real work is single-layer and small: it does NOT need the pipeline. Run it as **`/mustard:task`** (lean — no spec, no wave, no QA gate) on the KEPT anchors, and STOP the heavier flow. *(This is the −81%-turns lever: most enhancements land here.)*
   - **`route` = `"feature"`** → continue the feature pipeline. **`scope` = `"light"`** → inline EXECUTE; **`scope` = `"full"`** → open `feature/full-plan.md` and follow PLAN.
   - **`concerns`** → each object is its OWN unit of work, scoped to its `anchors` (never the flat list): `/feature` → a unit in the three-natures split (Full: a wave); `/task` → its own dispatched action; `/bugfix` → its own diagnose+fix.
4. **Deterministic fallback (never block the flow).** If the validator is unavailable, the dispatch errors, or its reply does not parse into an object with a `route`, DROP the verdict and proceed from the flat pruned `anchors` + the DETERMINISTIC scope (fill the census → `spec-draft` → `plan-prepare`) exactly as you would without it. The validator only ever SHARPENS — its absence degrades to the deterministic path, never to a stall.

## INTENT hygiene — the rule that makes or breaks the validator
**`<INTENT>` MUST be the user's actual REQUEST — their own words / phrasing of WHAT they want — the SAME rich intent you passed to the digest (the user's content words + your code-vocabulary translation), NOT a stripped list of isolated code terms.** The validator judges meaning (is this `card` the UI card or the credit card?); given only bare terms it loses the context that disambiguates, and prunes/routes wrong. Never lapidate the request away before the validator.

## Invariant
The scan digest is **100% deterministic**; the validator is the **ONLY AI step** and lives in the **ORCHESTRATION**, never inside the scan. `digest-validate-render` does pure deterministic assembly (reuses the digest retrieval + project span, emits a byte-stable prompt, calls no model); the JUDGEMENT is the dispatched Sonnet agent's, run one layer above the scan. It SUBSUMES the older concern-split judge — its `concerns` field carries the same partition, so flows run THIS step, not a second judge.
