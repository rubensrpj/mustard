# Why the router ships as two injectables

Background for a maintainer editing `templates/mustard/{orchestrator,dispatch}.md`. The
templates themselves carry rules only; the reasoning lives here, loaded on demand.

## The delivery channel

An injectable is a file declared in `mustard.json#inject`. A hook reads it and returns it
as `additionalContext`, which Claude Code splices into the model's window. Two SIBLING
HOOKS carry the router today — one per injectable — and both ride the same event,
`userPromptSubmit`.

**A hook response is capped at 10,000 characters.** Past that, the overflow is not cut
mid-sentence: Claude Code saves it to a file and hands the window a preview plus that
path. That is the real danger. A router that becomes a pointer stops being *in force*, and
this text has to be in force at the moment a unit opens.

**The cap is per hook response, not per event.** Measured 2026-08-25 on this repository:
two sibling hooks registered on the same `UserPromptSubmit`, emitting 6,000 characters
each (12,000 combined), both arrived intact, in separate blocks, each with its own header
and end marker. Nothing was truncated. The official guide states the same rule: *"Text
from `additionalContext` is kept from every hook and passed to Claude together."*

So the way to give a document its own ceiling is **one sibling hook per injectable**, not
one hook per event. There is no composite budget between siblings.

## Why two files rather than one

The router was a single file until 2026-08-20. It measured 9,543 characters, under the
cap. That day a commit improved the opening question (three correctable fields, base asked
before type) and pushed the file to 12,177 — over the cap. Thirty-six minutes later a
second commit split it across two events to fit.

The split was the right call for the ceiling and the wrong call for delivery. It moved the
half that carries the opening question to `sessionStart`, which has paths that never
re-inject: `fork` matched no matcher, `startup` never cleared the per-session markers, and
a project installed before the split kept declaring only the first half. Every one of
those failures was silent.

Both halves now ride `userPromptSubmit`, one sibling hook each. That event is
**self-healing**: the `once` markers live under `.claude/.session/<session_id>/`, so any
path that loses the window (a new session, a fork, a resume) also loses the marker, and
the next prompt re-delivers on its own. `sessionStart` can never have that property — it
only fires on openings.

## Why the internal fold is not the ceiling

`prompt_submit_inject.rs` folds every injectable of one event into a single
`Verdict::Inject`, because Mustard's own dispatcher is last-writer-wins: two Injects on the
same invocation would drop one. **That limit is Mustard's, not Claude Code's.** Registering
one sibling hook per injectable sidesteps it without rewriting the dispatcher — each hook
is its own invocation, with its own verdict and its own ceiling.

Do not write in a shipped template that Claude Code overwrites sibling context. It does
not; it combines.

## The house style for an injectable

The 2026-08-20 overflow was caused by prose that argued with itself, not by too many rules.
Rewriting both files rule-first cut `orchestrator.md` from 6,592 to 5,332 characters and
`dispatch.md` from 7,995 to 5,728, with zero operational tokens lost (audited token by
token against the previous revision).

- **Rule, trigger, command.** State what to do, when, and with which flag.
- **Keep the *why* only when it decides a doubtful case.** "hotfix is pinned *since that
  row exists precisely so an emergency can be named*" earns its place; the story of how
  the pin was chosen does not.
- **History and migration notes belong here, not there.** "There is no `.claude/spec/`
  carve-out *any more*" reads as archaeology to someone who never saw the old behaviour.
- **Em dashes hide run-on sentences.** The pre-rewrite `orchestrator.md` had 16 of them
  across 21 paragraphs. Ending the sentence is shorter and clearer.
- **Bold only marks a prohibition or an inviolable rule.** When everything is bold, nothing
  is.

A first pass at this cut too far and lost four real instructions (`never hand-write it`;
why `ac-negative-check` accepts either `--spec` form; `trust its thresholds`; `never enter
it just for guidance`). All were restored. **The safe cut is justification, and it runs out
well before any aggressive character target** — which is why there is no per-file budget
below the real 10,000 ceiling. A budget that forces a rule out is a guard that lies: it
stays green while the product gets worse.

## Language

Templates, refs, code and comments are EN-only; specs follow `mustard.json#specLang`.
`mustard-rt run language-audit` enforces it with a diacritic-seed heuristic.
