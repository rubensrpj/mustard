<!-- mustard:generated -->
# Canonical Pipeline Phases

Single source of truth for Mustard phase vocabulary. Every consumer (hooks,
docs, dashboard, metrics) MUST use these names. Loaded on demand.

Canonical sequence: `ANALYZE → PLAN → EXECUTE → REVIEW → QA → CLOSE`
(+ `COORDINATE` for roadmaps / multi-spec parents).

| Phase | What it represents | Entry trigger |
|-------|--------------------|---------------|
| `ANALYZE` | Research the codebase: locate entities, read relevant files, map the change surface. | Pipeline starts (`/feature`, `/bugfix`) — pipeline-state created with `phase=ANALYZE`. |
| `PLAN` | Write the spec: scope, waves, Acceptance Criteria. Full scope only; Light scope skips it. | Spec PLAN file written / pipeline-state `phase=PLAN`. |
| `EXECUTE` | Implement the change across delegated agents. | `/approve` accepted, or Light scope after ANALYZE — pipeline-state `phase=EXECUTE`. |
| `REVIEW` | Inspect produced code for correctness, conventions, regressions before QA. | `/review` invoked or review agents dispatched — emits `review.*` events. |
| `QA` | Run the spec's Acceptance Criteria commands and record pass/fail (Wave 10). | `/mustard:qa` runs — emits `qa.result`. |
| `CLOSE` | Finalize: sync registry, move spec to done, commit. | pipeline-state `phase=CLOSE`; gated by `close-gate.js`. |
| `COORDINATE` | Parent-level orchestration of a roadmap with multiple child specs. | A spec with `children[]` enters coordination — pipeline-state `phase=COORDINATE`. |

## Notes

- `REVIEW` is a recognized phase: it already emits real events but was invisible
  because earlier vocabularies omitted it.
- The pipeline-phase hook records phases descriptively; it does not reject
  unknown values. This list defines the names every emitter should use.
- Light scope sequence: `ANALYZE → EXECUTE → REVIEW → QA → CLOSE` (PLAN skipped).
