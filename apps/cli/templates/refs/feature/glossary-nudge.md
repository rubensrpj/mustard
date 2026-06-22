# Glossary Grill (ANALYZE, optional, non-blocking)

> Detail for the `/feature` ANALYZE "glossary grill" (Selo 1). A deterministic,
> zero-token coverage check decides whether to run a LIGHT inline grill **before**
> planning: when the glossary does not yet define the domain terms this feature
> touches, ask the user for a one-line definition of each and persist the
> confirmed pairs with `grill-capture`. It never blocks (any term the user skips
> is dropped) and the Rust side stays zero-token — the only AI work is asking the
> user, in chat, for words they already hold.

## When

Right after the `mustard-rt run feature` digest (which produces the matched
repo-vocabulary terms), once per request. Skip entirely on Light requests where
you already have full precedent — the nudge is most valuable on net-new / wide
Full features that touch domain terms the glossary doesn't define.

## Run

```bash
mustard-rt run glossary-coverage --intent "<the request>" \
  --context {root}/CONTEXT.md
  # repeat --context for each subproject CONTEXT.md / a CONTEXT-MAP.md
```

It is **deterministic and zero-token**: pure Rust over `grain.model.json` +
`CONTEXT.md`, reusing the exact term matcher `context-slice` uses. Output is
byte-stable JSON:

```json
{ "verdict": "missing|weak|ok|na", "present": false, "termsTotal": 3,
  "termsCovered": 0, "coveragePct": 0, "uncovered": ["spec","wave","pipeline"],
  "contextFile": "CONTEXT.md" }
```

- **N (`termsTotal`)** = the digest's MATCHED terms (repo vocabulary the intent
  maps to), not raw intent tokens — stopwords never inflate it.
- **`uncovered`** = the actionable payload: the weak/missing domain terms to
  grill, in declaration order.
- **`contextFile`** = the resolved glossary `grill-capture` writes into (the
  authored `CONTEXT.md`, or the first requested path when none exists yet, so a
  `missing` glossary still names a destination). Empty when no `--context` given.
- **`verdict`**: `missing` (no `CONTEXT.md` authored) · `weak` (authored but
  coverage `< 50%` OR `≥ 3` uncovered matched terms) · `ok` (covered, or no
  domain terms touched) · `na` (scan model unavailable — fail-open).

## React

- `verdict ∈ {missing, weak}` → run a **LIGHT inline grill**. Take the ≤3 most
  central terms from `uncovered` (drop the tangential ones — a seeder term, a
  stats-DTO term). In ONE batched AskUserQuestion, ask the user for a one-line
  definition of each:
  > "Your glossary doesn't define these domain terms yet (`{uncovered}`). A
  > one-line definition each sharpens the spec and every dispatched agent's
  > shared language. (Skip any you'd rather not.)"
  Then persist EACH confirmed pair (skip the ones the user left blank):
  ```bash
  mustard-rt run grill-capture --term "<term>" \
    --definition "<the user's one-line answer>" \
    --context <contextFile from the coverage output>
  ```
  `grill-capture` is glossary-only + update-not-duplicate (re-grilling a term
  replaces its block in place); it resolves the same CONTEXT-MAP-aware target.
  Continue to PLAN on any answer — a fully-skipped grill is a no-op.
- `verdict ∈ {ok, na}`, or `glossary-coverage` is missing/errors → **stay silent
  and continue**. The lean path is byte-identical to a run without this step.

## Hard rules

- **Never block.** Every term is optional; a skipped or empty answer is dropped,
  never gated on. The grill grounds the spec's language — it does not interrogate.
- **Keep it light — ≤3 terms.** This is the inline grill, not the full
  `grill-with-docs` skill (that one challenges a plan against the whole domain
  model and writes ADRs). Here you only capture the one-line definitions of the
  most central uncovered terms.
- **Only `grill-capture` writes.** The orchestrator never hand-edits `CONTEXT.md`
  — `grill-capture` owns the **English-only** `CONTEXT.md` contract
  (`spec-language.md`); only the live AskUserQuestion text localises to the
  user's language. A confirmed definition is written verbatim as the user's
  words (translate to English yourself if they answered in another language).
- **Fail-open to OFF.** If `glossary-coverage` / `grill-capture` is absent
  (binary not rebuilt) or errors, treat it as `na` and continue — the grill is
  an enhancement, never a gate.

## Why it pays

The glossary the user authors here is not wasted: it flows downstream for FREE
through the already-wired `context-slice → {context_md}` cache, so a sharpened
`CONTEXT.md` reaches every wave-1 subagent with zero new per-dispatch wiring.
Deliberate friction lands only where it amortises (wide/Full features with
uncovered domain terms); everywhere else the step is silent and free.
