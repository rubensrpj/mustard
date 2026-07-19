---
id: wave.field-report-fix-package-sialia.6-prose
---

# wave-6-prose

## Summary

plugin prose synced to the new behaviors: honest r shortcut, namespaced agent tables, Expect: in the AC schema, trace gate and boundary docs

## Network

- Parent: [[spec.field-report-fix-package-sialia]]
- Depends on: [[wave.field-report-fix-package-sialia.1-approval]], [[wave.field-report-fix-package-sialia.2-agents]], [[wave.field-report-fix-package-sialia.3-evidence]], [[wave.field-report-fix-package-sialia.4-trace]], [[wave.field-report-fix-package-sialia.5-boundary]]

## Tasks

- [ ] plugin/commands/spec.md: reword every `r` shortcut promise (lines announcing approve + execute inline / skip the confirm) to: `r` = execute immediately AFTER the real approval — the plan-mode accept (ExitPlanMode) or the approval AskUserQuestion still happens; on a Full spec clarify (.clarified) precedes approval; the picker never bypasses either marker
- [ ] plugin/refs/spec/resume-loop.md §A: align the same wording — `r` pre-answers the EXECUTE continuation, not the approval itself
- [ ] Namespace the agent-type tables: plugin/refs/agent-prompt/agent-prompt.md, plugin/pipeline-config.md, plugin/commands/task.md, plugin/commands/scan.md, plugin/commands/review.md, plugin/commands/qa.md, plugin/commands/feature.md, plugin/commands/bugfix.md — review/qa -> mustard:mustard-review, guards -> mustard:mustard-guards, patterns -> mustard:mustard-patterns; builtins stay bare; where a table repeats the full mapping, prefer pointing at the canonical table in pipeline-config.md
- [ ] plugin/refs/feature/full-plan.md: document the optional `Expect:` evidence line in the AC schema (regex over stdout+stderr, opt-in) and the MUSTARD_TRACE_GATE_MODE coverage gate; note that wave plans must cover every parent AC via satisfies/acceptance
- [ ] Do NOT touch behavior code in this wave — prose only

## Files

- `plugin/commands/spec.md`
- `plugin/refs/spec/resume-loop.md`
- `plugin/refs/agent-prompt/agent-prompt.md`
- `plugin/pipeline-config.md`
- `plugin/commands/task.md`
- `plugin/commands/scan.md`
- `plugin/commands/review.md`
- `plugin/commands/qa.md`
- `plugin/commands/feature.md`
- `plugin/commands/bugfix.md`
- `plugin/refs/feature/full-plan.md`
