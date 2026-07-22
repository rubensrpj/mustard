---
id: wave.qa-gate-and-settle-multirepo.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.qa-gate-and-settle-multirepo.1-qa]] | qa | — | Stop the QA gate from reporting success over criteria it never ran: drain the child's pipes, tell a timeout apart from a benign skip, and never record a verdict a self-invoked run did not earn. |
| 2 | [[wave.qa-gate-and-settle-multirepo.2-git]] | git | [[wave.qa-gate-and-settle-multirepo.1-qa]] | Make the exit ritual honest in a monorepo with a submodule: resolve bases from the superproject, say what was resolved when refusing, and report one entry per repository instead of a half-'settled'. |
