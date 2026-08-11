# Spec Hygiene

> Loaded by `/feature` + `/bugfix` — automatic spec audit before ANALYZE. Silent when there is nothing to audit.

Before starting a new pipeline, audit `.claude/spec/*/spec.md` (flat layout + `meta.json` lifecycle: `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Spec Layout`).

1. Scan every spec's `meta.json` for `stage`/`outcome`/`flags`, and `spec.md` for checkbox completion (`[x]` vs `[ ]`). `Completed`/`Abandoned` specs are verified in step 2, skipped in step 3.
2. Completed/Abandoned specs — verify before trusting:
   - Analyze first: ALL checklist items `[x]`, no unresolved `BLOCKED` in `## Concerns`, build/type-check references satisfied.
   - Confirmed done → `mustard-rt run complete-spec {name} --archive` (emits `pipeline.outcome`, removes any `.diff.md`; the dir stays at `.claude/spec/{name}/` — no move). Log `[HYGIENE] Verified and archived {name}`. **It can refuse (exit 2), and that is not a bug:** the archive is a no-op only for a spec whose EVENT LOG already reads `completed`/`cancelled`. A spec marked done in `meta.json` alone — a meta-only close, or one whose `.events/` was pruned — would really be closed by this call, so it needs a recorded `qa.result overall=pass` like any other close. Record one externally (`mustard-rt run qa-run --spec {name}`) or leave the spec alone; never reach for a switch, there is none.
   - Incomplete → set `meta.json` `stage: Execute` + `outcome: Active` via `mustard-rt run emit-pipeline`, log `[HYGIENE] {name} marked Completed but has N unchecked items — reverted to Execute`, then treat as in-progress (step 3).
3. In-progress specs (`outcome: Active`, stage ≠ `Close`) → **ask ONLY when the answer is not already given.** Two conditions, either one alone is enough to ask:
   - **overlap** — the new intent collides with the active spec (same files, same subproject, same mechanism), so continuing it and starting this one are candidates for the same work; or
   - **not explicitly requested** — this work was inferred by the pipeline rather than asked for in the same message, so the user never chose between the two.

   Neither holds — the user asked for THIS work, in this message, and it touches something else → do not ask. Record one line, `[HYGIENE] spec {name} remains parked`, and proceed to ANALYZE (the parked spec stays at `.claude/spec/{name}/` and is resumable via `/mustard:spec` whenever the user wants it).

   Asking → one `AskUserQuestion`: "Found spec in progress: {name} (stage {stage}, {done}/{total} done). Continue it before starting a new one?"
   - yes → stop, suggest `/mustard:spec`.
   - no → proceed to ANALYZE (the existing spec stays at `.claude/spec/{name}/`).

   **Why it is conditional and not merely tolerated as skippable:** unconditional, this question is asked most often in the one situation where its answer is already in the message that triggered the flow, and it is answered "no" every time. A step that is routinely skipped without consequence teaches the reader to judge EVERY step of this protocol case by case — including the steps that must never be skipped, like step 2's verification before trusting a `Completed` marker. Making the question fire only when it can change the outcome is what keeps the rest of the protocol binding.
4. No active specs → proceed to ANALYZE normally.
