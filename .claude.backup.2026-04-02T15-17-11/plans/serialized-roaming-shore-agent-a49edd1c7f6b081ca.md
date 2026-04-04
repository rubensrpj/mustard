# Plan: Add Escalation Statuses to Agent Return Format

## Objective
Add 4 escalation statuses (DONE, DONE_WITH_CONCERNS, NEEDS_CONTEXT, BLOCKED) to pipeline instructions across 5 template files. Text/template changes only — no code.

## Findings

### `templates/context/shared/`
Directory does not exist. The task description says to find the shared context and add escalation statuses to the return format section. Since there are no shared context files, this sub-task is skipped — will note it.

### Files to modify
1. `templates/pipeline-config.md` — add new `## Escalation Statuses` section at the end
2. `templates/commands/mustard/feature/SKILL.md` — add escalation handling to EXECUTE phase (step 8)
3. `templates/commands/mustard/bugfix/SKILL.md` — add escalation handling in EXECUTE section
4. `templates/commands/mustard/complete/SKILL.md` — add concern surfacing step before finalization
5. `templates/commands/mustard/resume/SKILL.md` — add escalation handling in Step 3 (dispatch/return handling, step 17)

Note: `/resume` is the canonical EXECUTE runner for Full scope pipelines, so it also needs escalation handling — this is implied by "same change as feature".

## Insertion Points

### 1. `templates/pipeline-config.md`
Append a new `## Escalation Statuses` section at the end of the file (after line 106).

### 2. `templates/commands/mustard/feature/SKILL.md`
Insert after step 8 / step 8b (around line 120–121), before step 9 (REVIEW). Add `#### Escalation Status Handling` subsection within EXECUTE Phase.

### 3. `templates/commands/mustard/bugfix/SKILL.md`
Insert after the **Validate** block in EXECUTE, before the Retry Compact Advisory. Add `#### Escalation Status Handling` subsection.

### 4. `templates/commands/mustard/complete/SKILL.md`
Insert a new step between the Verification Gate and the Action steps (before step 1 "Locate active spec"). Add `#### Surface Accumulated Concerns`.

### 5. `templates/commands/mustard/resume/SKILL.md`
Insert after step 17 (dispatch/return handling) before step 17b (Agent Memory). Add `#### Escalation Status Handling`.

## Status Vocabulary (consistent across all files)
- **DONE** — completed, no concerns
- **DONE_WITH_CONCERNS** — completed, flagged doubts
- **NEEDS_CONTEXT** — cannot complete, missing information
- **BLOCKED** — cannot complete, structural impediment

## Tasks
- [ ] Add `## Escalation Statuses` to `pipeline-config.md`
- [ ] Add escalation handling to `feature/SKILL.md` EXECUTE phase
- [ ] Add escalation handling to `bugfix/SKILL.md` EXECUTE phase
- [ ] Add escalation handling to `resume/SKILL.md` Step 3
- [ ] Add concern surfacing to `complete/SKILL.md` before finalization
