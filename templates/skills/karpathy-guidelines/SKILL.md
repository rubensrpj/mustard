---
name: karpathy-guidelines
description: Behavioral guidelines to reduce common LLM coding mistakes (think before coding, simplicity first, surgical changes, goal-driven execution). Use when implementing, writing, editing, modifying, changing, refactoring, fixing, bugfixing, or reviewing code. Apply to any code alteration in features, bugfixes, refactors, or reviews. Loads the core principles only; use karpathy-guidelines-detail for examples on complex refactors.
license: MIT
source: manual
---

# Karpathy Guidelines (core)

Anti-slop principles. Bias toward caution over speed; use judgment for trivial tasks.

## 1. Think Before Coding

- State assumptions explicitly; if uncertain, ask.
- Present multiple interpretations instead of picking silently.
- Flag simpler approaches; push back when warranted.
- Stop and name what's confusing before guessing.

## 2. Simplicity First

- Minimum code that solves the problem; nothing speculative.
- No features beyond what was asked.
- No abstractions for single-use code.
- No flexibility/configurability that wasn't requested.
- No error handling for impossible scenarios.

## 3. Surgical Changes

- Touch only what the request requires.
- Match existing style even if you'd do it differently.
- Don't refactor or "improve" adjacent code, comments, formatting.
- Remove orphans your own changes created; mention (don't delete) pre-existing dead code.
- Every changed line must trace directly to the user's request.

## 4. Goal-Driven Execution

- Convert tasks into verifiable goals before coding.
- For multi-step work, state a brief plan with a verify step per item.
- Loop independently against strong success criteria; weak criteria force re-clarification.
- Tests first when the goal is correctness ("Add validation" → write failing tests, then pass).

---

> Working when: fewer unnecessary changes, fewer rewrites for overcomplication, clarifying questions come before mistakes. Examples and elaboration: `karpathy-guidelines-detail`.
