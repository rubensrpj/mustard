# Glossary Grill

> Detail for `/feature` ANALYZE — an optional, non-blocking, zero-token coverage check that grills undefined domain terms before planning. It never blocks (any term the user skips is dropped); the only AI work is asking the user, in chat, for words they already hold.

## When
Right after the `mustard-rt run feature` digest (which produces the matched repo-vocabulary terms), once per request. Skip on Light requests with full precedent — the grill pays off on net-new / wide Full features that touch domain terms the glossary does not define.

## Run
```bash
mustard-rt run glossary-coverage --intent "<the request>" --context {root}/CONTEXT.md
# repeat --context per subproject CONTEXT.md / a CONTEXT-MAP.md
```
Deterministic + zero-token (pure Rust over `grain.model.json` + `CONTEXT.md`, the same term matcher `context-slice` uses). Byte-stable JSON:
```json
{ "verdict":"missing|weak|ok|na", "present":false, "termsTotal":3,
  "termsCovered":0, "coveragePct":0, "uncovered":["spec","wave","pipeline"],
  "contextFile":"CONTEXT.md" }
```
- `termsTotal` = the digest's MATCHED terms (repo vocabulary the intent maps to), never raw intent tokens — stopwords never inflate it.
- `uncovered` = the actionable payload: the weak/missing domain terms to grill, in declaration order.
- `contextFile` = where `grill-capture` writes (the authored `CONTEXT.md`, or the first requested path when none exists yet). Empty when no `--context` is given.
- `verdict`: `missing` (no `CONTEXT.md` authored) · `weak` (authored but coverage < 50% OR >= 3 uncovered matched terms) · `ok` (covered, or no domain terms touched) · `na` (scan model unavailable — fail-open).

## React
- `missing`/`weak` → run a LIGHT inline grill. Take the <=3 most central `uncovered` terms (drop tangential ones — a seeder term, a stats-DTO term). ONE batched `AskUserQuestion` asks the user for a one-line definition of each: "Your glossary doesn't define these domain terms yet ({uncovered}). A one-line definition each sharpens the spec and every dispatched agent's shared language. (Skip any you'd rather not.)" Persist EACH confirmed pair (skip blanks):
  ```bash
  mustard-rt run grill-capture --term "<term>" --definition "<the user's answer>" --context <contextFile from the coverage output>
  ```
  `grill-capture` is glossary-only + update-not-duplicate (re-grilling a term replaces its block in place). Continue to PLAN on any answer — a fully-skipped grill is a no-op.
- `ok`/`na`, or the tool is missing/errors → stay silent and continue. The lean path is byte-identical to a run without this step.

## Hard rules
- Never block. Every term is optional; a skipped or empty answer is dropped, never gated.
- Keep it light — <=3 terms. This is the inline grill, not the `grill-with-docs` skill (that one challenges a whole plan against the domain model and writes ADRs).
- Only `grill-capture` writes, and `CONTEXT.md` is English-only: write the definition verbatim as the user's words, translating to English yourself if they answered in another language. Only the live `AskUserQuestion` text localises. (Contract: `${CLAUDE_PLUGIN_ROOT}/refs/feature/spec-language.md`.)
- Fail-open to OFF. Absent binary or any error → treat as `na` and continue. The captured glossary flows downstream for free through the `context-slice -> {context_md}` cache, reaching every wave-1 subagent with no new per-dispatch wiring.
