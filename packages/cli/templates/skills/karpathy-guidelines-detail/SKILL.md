---
name: karpathy-guidelines-detail
description: Extended examples and elaboration for karpathy-guidelines. Load when refactor crosses 3+ files, complex behavior change, or first attempt was rejected for slop. Skip for trivial edits — karpathy-guidelines core is sufficient.
license: MIT
source: manual
---

# Karpathy Guidelines — Detail & Examples

Companion to `karpathy-guidelines` (core). The 4 principles live there; this skill carries the worked examples, elaboration, and edge-case discussion that justify them. Load this only when the core rules are insufficient: large refactors, ambiguous behavior changes, or after a rejected attempt.

Derived from [Andrej Karpathy's observations](https://x.com/karpathy/status/2015883857489522876) on LLM coding pitfalls.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

## 1. Think Before Coding — elaboration

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them — don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

The failure mode this counters: the LLM picks the first plausible reading, builds 200 lines around it, and only at review does the user discover the wrong interpretation was implemented. One clarifying question up front costs less than one rewrite at the end.

## 2. Simplicity First — elaboration

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

The failure mode this counters: gold-plating. Adding a `config` object "in case we need to vary this later", wrapping a single call site in a factory, exporting things that have no second consumer. Every speculative abstraction is dead weight until proven useful, and most never are.

## 3. Surgical Changes — elaboration

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it — don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

The failure mode this counters: the "while I'm here" rewrite. The diff balloons from 5 lines to 200, the reviewer can't tell what's the actual fix vs. cosmetic noise, and the merge gets blocked or — worse — silently introduces a regression in code that wasn't supposed to change.

## 4. Goal-Driven Execution — elaboration

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

The failure mode this counters: claiming "done" based on plausibility rather than evidence. With weak criteria, the model can convince itself any output is acceptable. With a runnable verification step per item, the loop terminates only when the check passes — no self-deception possible.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.

---

> Derivado de [forrestchang/andrej-karpathy-skills](https://github.com/forrestchang/andrej-karpathy-skills) (MIT).
