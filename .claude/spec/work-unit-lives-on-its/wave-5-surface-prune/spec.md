---
id: wave.work-unit-lives-on-its.5-surface-prune
---

# wave-5-surface-prune

## Summary

The six read-only and legacy doors are removed: mustard, status, stats, knowledge, maint and skills.

## Network

- Parent: [[spec.work-unit-lives-on-its]]
- Depends on: [[wave.work-unit-lives-on-its.3-pr-door]], [[wave.work-unit-lives-on-its.4-unit-tools]]

## Tasks

- [ ] Remove the mustard door — the natural-language routing is injected on every prompt via mustard.json#inject and does not depend on the command.
- [ ] Remove the status and stats doors; the active-spec listing the operator actually needs is already what the spec door prints with no argument.
- [ ] Remove the knowledge door while keeping the decision and lesson capture and the search that the analysis consults — only the manual reading door goes.
- [ ] Remove the skills door: create, optimize and eval are inert without a Python tool this project does not bundle, and what remains is listing and deleting a folder.
- [ ] Remove the maint door, keeping only its doctor, which wave 6 folds into upsert.

## Files

- `plugin/commands/mustard.md`
- `plugin/commands/status.md`
- `plugin/commands/stats.md`
- `plugin/commands/knowledge.md`
- `plugin/commands/maint.md`
- `plugin/commands/skills.md`
