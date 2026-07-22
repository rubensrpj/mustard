---
id: wave.btw-plan-rework-fixes.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.btw-plan-rework-fixes.1-rt]] | rt | — | Harness engine: reconcile-vs-freeze scaffold writes, spec-dir flag aliases plus normalisation, schema-teaching plan errors, and the resume-gate token rename. |
| 2 | [[wave.btw-plan-rework-fixes.2-docs]] | docs | [[wave.btw-plan-rework-fixes.1-rt]] | Instruction surfaces: stop telling readers to run the absorbed command, and lock that with a guard test derived from the published CLI surface. |
