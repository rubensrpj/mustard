# Plan: Resume SKILL.md + pipeline-config.md Handoff Edits

## Edit 1 — Replace Step 5 summary block in SKILL.md

**File:** `templates/commands/mustard/resume/SKILL.md`

**Target (lines 26–37):**
```
5. **Present summary and ASK before continuing:**

   ```
   Pipeline: {spec-name}
   Scope:    {light|full}
   Status:   {status} | Phase: {phase}
   Progress: {completed}/{total} tasks
   Next:     {next agent/wave}
   Decisions: {decisions[] if present}

   Continue?
   ```
```

**Replace with expanded handoff format:**
- Pipeline name, scope, phase, started (from spec header Checkpoint or state JSON), elapsed (now − started)
- Completed tasks: extracted from `[x]` checkboxes in spec
- Pending tasks: extracted from `[ ]` checkboxes in spec
- Concerns: from `<!-- CONCERN: -->` comments in spec (section omitted if none found)
- Context block: branch (from `git branch --show-current`), files changed (from diff-context.js output), last agent + last action (from `.claude/.agent-memory/_index.json` if present)
- ONE `→ Next action:` line (the first pending task)
- Prompt: "Continue from next action, or review spec first?"

---

## Edit 2 — Add `### Pause Handoff` section before `## INVIOLABLE RULES`

**File:** `templates/commands/mustard/resume/SKILL.md`

**Insert before `## INVIOLABLE RULES` (line 134):**

```markdown
### Pause Handoff

When the user asks to pause or `/resume` detects a mid-session interrupt:

1. Write to pipeline state JSON: `pausedAt` (ISO timestamp), `pauseReason` (user message or "interrupted"), `nextAction` (first pending `[ ]` task title)
2. Write agent memory:
   ```bash
   echo '{"agent_type":"orchestrator","pipeline":"{spec-name}","summary":"Paused at {phase}: {nextAction}","details":{"pausedAt":"{iso}","pauseReason":"{reason}","nextAction":"{nextAction}"}}' | node .claude/scripts/memory-write.js
   ```
3. Confirm to user: "Pipeline paused. Next action saved: {nextAction}"
```

---

## Edit 3 — Add `### Next Action Rule` section after Pause Handoff, before INVIOLABLE RULES

**File:** `templates/commands/mustard/resume/SKILL.md`

**Insert after Pause Handoff, still before `## INVIOLABLE RULES`:**

```markdown
### Next Action Rule

The handoff summary MUST include exactly ONE next action.

**Wrong:**
> Next: Wave 2 — Backend + Frontend agents

**Right:**
> → Dispatch Backend agent (Wave 2): implement POST /contracts endpoint

Eliminates decision fatigue on resume — user or runner knows exactly where to start without re-reading the spec.
```

---

## Edit 4 — Add `## Session Handoff` section in pipeline-config.md

**File:** `templates/pipeline-config.md`

**Insert after `## Compact Guidance` section (after line 41), before `## Diagnostic Failure Routing`:**

```markdown
## Session Handoff

Fields persisted to pipeline state JSON when a session pauses or is interrupted. Compiled on `/resume` to reconstruct context without re-reading the full spec.

| Field | Source | Purpose |
|-------|--------|---------|
| `pausedAt` | ISO timestamp at pause | Calculates elapsed time on resume |
| `pauseReason` | User message or "interrupted" | Explains why the session ended |
| `nextAction` | First pending `[ ]` task title from spec | Single entry point — no ambiguity on resume |
| `filesChanged` | Output of `diff-context.js` | Shows what already changed so agents avoid re-doing work |
| `lastAgent` | Last entry in `.claude/.agent-memory/_index.json` | Provides continuity — downstream agents know what ran before |

On `/resume`: compile all five fields into the handoff summary before asking the user to confirm. Goal: user resumes in <30 seconds.
```

---

## Execution Order

1. Edit 1: Replace Step 5 block (old_string is unique — spans the exact summary block)
2. Edit 2: Insert Pause Handoff before `## INVIOLABLE RULES`
3. Edit 3: Insert Next Action Rule after Pause Handoff (between Edit 2 insertion and `## INVIOLABLE RULES`)
4. Edit 4: Insert Session Handoff in pipeline-config.md after Compact Guidance block

All edits are independent of each other except Edits 2 and 3 share an insertion point — Edit 3 must be done after Edit 2 so the anchor text exists.
