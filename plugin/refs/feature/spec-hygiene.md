# Spec Hygiene

> Detail for `/feature` — automatic spec audit before ANALYZE. Silent when there is nothing to audit.

Before starting a new pipeline, audit `.claude/spec/*/spec.md` (flat layout + `meta.json` lifecycle: `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Spec Layout`).

1. Scan every spec's `meta.json` for `stage`/`outcome`/`flags`, and `spec.md` for checkbox completion (`[x]` vs `[ ]`). `Completed`/`Abandoned` specs are verified in step 2, skipped in step 3.
2. Completed/Abandoned specs — verify before trusting:
   - Analyze first: ALL checklist items `[x]`, no unresolved `BLOCKED` in `## Concerns`, build/type-check references satisfied.
   - Confirmed done → `mustard-rt run complete-spec {name} --archive` (emits `pipeline.outcome`, removes any `.diff.md`; the dir stays at `.claude/spec/{name}/` — no move). Log `[HYGIENE] Verified and archived {name}`.
   - Incomplete → set `meta.json` `stage: Execute` + `outcome: Active` via `mustard-rt run emit-pipeline`, log `[HYGIENE] {name} marked Completed but has N unchecked items — reverted to Execute`, then treat as in-progress (step 3).
3. In-progress specs (`outcome: Active`, stage ≠ `Close`) → one `AskUserQuestion`: "Found spec in progress: {name} (stage {stage}, {done}/{total} done). Continue it before starting a new one?"
   - yes → stop, suggest `/mustard:spec`.
   - no → proceed to ANALYZE (the existing spec stays at `.claude/spec/{name}/`).
4. No active specs → proceed to ANALYZE normally.
