# Glossary Grill

> Detail for `/feature` ANALYZE — an optional, non-blocking, zero-token coverage check that grills undefined domain terms before planning. It never blocks (any term the user skips is dropped); the only AI work is asking the user, in chat, for words they already hold.
>
> ASKING is optional. The OUTCOME is not: it ran and here are the terms it settled, or it declined and here is the stated reason. Three outcomes to react to — grill, decline, stay silent — and on a Full spec the first two are what the clarify-finalize records. Staying silent there leaves a marker with nothing in it, which `approve-spec` refuses.

## When
Right after the `mustard-rt run feature` digest (which produces the matched repo-vocabulary terms), once per request. Skip on Light requests with full precedent — the grill pays off on net-new / wide Full features that touch domain terms the glossary does not define.

## Run
```bash
mustard-rt run glossary-coverage --intent "<the request>" --context {root}/CONTEXT.md
# repeat --context per subproject CONTEXT.md / a CONTEXT-MAP.md
```
Deterministic + zero-token (pure Rust over `grain.model.json` + `CONTEXT.md`, the same term matcher `context-slice` uses). Byte-stable JSON:
```json
{ "verdict":"weak", "present":true, "termsTotal":4,
  "termsCovered":1, "coveragePct":25, "uncovered":["spec","wave","pipeline"],
  "contextFile":"CONTEXT.md", "statedReason":"" }
```
- `termsTotal` = the digest's MATCHED terms (repo vocabulary the intent maps to), never raw intent tokens — stopwords never inflate it.
- `uncovered` = the actionable payload: the weak domain terms to grill, in declaration order. **It is populated only when a glossary EXISTS (`present:true`).** With none authored (`present:false` ⇒ `verdict:"missing"`) the key keeps its place and arrives **empty** — deliberately: a list of words to interrogate answers "which entries are thin", and with no file there is no entry to be thin. The question there is a different one, so the report declines to answer it with the wrong payload.
- `seed` = the answer to the question an ABSENT glossary raises: which words would a first one be worth opening with. Always present, non-empty ONLY on `missing`. Derived from the corpus — the terms this request touches that the repository's index reports as most CONCENTRATED (said in few places, not everywhere), the same arithmetic `declined` uses read from the other end. Never a hand-written list, and never a definition: it names words to ask a human about. Empty means the corpus judges nothing here worth defining — offer to move on, do not name terms by eye.
- `contextFile` = where `grill-capture` writes (the authored `CONTEXT.md`, or the first requested path when none exists yet). Empty when no `--context` is given. It is a real destination even before the file exists, so the capture creates it — the bootstrap needs no separate step.
- `statedReason` = always present, non-empty ONLY on `declined`: the sentence to pass VERBATIM to `grill-capture --finalize --reason`. It is what turns a decline into a recorded outcome instead of a skip nobody wrote down.
- `verdict`: `missing` (no `CONTEXT.md` authored) · `weak` (authored but coverage < 50% OR >= 3 uncovered matched terms) · `declined` (terms matched, but the CORPUS reports them as repository-wide vocabulary — a definition would restate the code) · `ok` (covered, or no domain terms touched) · `na` (scan model unavailable — fail-open).

Why `declined` exists beside the others and not above them: the check was designed for a business domain, where a matched term like `payable` has a definition worth capturing. In a harness the domain vocabulary IS technical vocabulary, so the matcher answers with the words the repository says everywhere and the grill would ask low-value questions. `declined` is that outcome said out loud. It is decided by the corpus's own arithmetic — a word's rarity against the median rarity of the repo's published term index — so no list of words is written down anywhere to rot or encode one person's taste. It is NOT an error and NOT a skip.

## React
- `missing` (no `CONTEXT.md` authored) → **do NOT grill: `uncovered` is empty here, by design (above), so there are no terms to take.** There is no glossary to extend — the answer is to author a first one. ONE `AskUserQuestion`: offer to start `{contextFile}` with a one-line definition of the terms in **`seed`** — the corpus's own answer to "which words would a first glossary open with", published on this verdict only. Take the ≤3 most central of them; never from `uncovered`, which is empty here by design. **`seed` empty is an answer, not a failure**: it means the corpus judges every term this request touches to be repository-wide vocabulary (or never published it), so there is nothing worth defining — offer to move on rather than naming terms by eye, and record the stated reason below. Answers → `grill-capture --term/--definition --context {contextFile}` exactly as below. The user declines, or the request does not warrant a first glossary → **record the stated reason** rather than staying silent, because on a Full spec silence leaves a marker with nothing in it:
  ```bash
  mustard-rt run grill-capture --finalize --spec {slug} --reason "no glossary is authored yet and this request did not warrant starting one"
  ```
- `weak` (authored but thin) → run a LIGHT inline grill. Take the <=3 most central `uncovered` terms (drop tangential ones — a seeder term, a stats-DTO term). ONE batched `AskUserQuestion` asks the user for a one-line definition of each: "Your glossary doesn't define these domain terms yet ({uncovered}). A one-line definition each sharpens the spec and every dispatched agent's shared language. (Skip any you'd rather not.)" Persist EACH confirmed pair (skip blanks):
  ```bash
  mustard-rt run grill-capture --term "<term>" --definition "<the user's answer>" --context <contextFile from the coverage output>
  ```
  `grill-capture` is glossary-only + update-not-duplicate (re-grilling a term replaces its block in place). Continue to PLAN on any answer. Then RECORD what it settled — naming the terms — so the outcome is a fact and not a memory:
  ```bash
  mustard-rt run grill-capture --finalize --spec {slug} --term "<term>" --term "<term>"
  ```
- `declined` → do NOT ask. RECORD the decline, verbatim, as a first-class outcome next to the others:
  ```bash
  mustard-rt run grill-capture --finalize --spec {slug} --reason "<statedReason from the coverage output>"
  ```
  This is the honest command: the reason is the corpus's sentence, not yours, and `approve-spec` accepts it as substance exactly like a list of captured terms. Declining is a decision the reader can audit; a silent skip is not.
- `ok`/`na`, or the tool is missing/errors → stay silent and continue. The lean path is byte-identical to a run without this step. On a FULL spec the clarify-finalize still has to record something — state the reason there (`--reason "the glossary already defines every matched term"`), because `approve-spec` refuses a marker that recorded nothing.

## Hard rules
- Never block. Every term is optional; a skipped or empty answer is dropped, never gated. What is NOT optional is saying which outcome happened — the finalize takes `--term` per settled term OR `--reason`, and refuses with neither.
- A definition the user gives you is conversation material too: carry the same term/meaning pairs into `spec-draft --material` (`/feature` §2.2) so they reach the spec and every wave, not just `CONTEXT.md`.
- Keep it light — <=3 terms. This is the inline grill, not the `grill-with-docs` skill (that one challenges a whole plan against the domain model and writes ADRs).
- Only `grill-capture` writes, and `CONTEXT.md` is English-only: write the definition verbatim as the user's words, translating to English yourself if they answered in another language. Only the live `AskUserQuestion` text localises. (Contract: `${CLAUDE_PLUGIN_ROOT}/refs/feature/spec-language.md`.)
- Fail-open to OFF. Absent binary or any error → treat as `na` and continue. The captured glossary flows downstream for free through the `context-slice -> {context_md}` cache, reaching every wave-1 subagent with no new per-dispatch wiring.
