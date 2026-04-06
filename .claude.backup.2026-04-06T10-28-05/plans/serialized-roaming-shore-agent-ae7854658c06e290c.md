# Plan: Improve Pause/Resume with Handoff Document

## Objective
Improve the resume command and pipeline-config with structured handoff information for pause/resume continuity.

## Files to Modify

### 1. `templates/commands/mustard/resume/SKILL.md`

**Change A — Expand Step 5 summary into full handoff format**

Location: Replace the current "Present summary and ASK before continuing" block (lines 26-37, starting with `5. **Present summary and ASK before continuing:**`)

Current Step 5 block:
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

Replace with full handoff format:
```
5. **Present handoff summary and ASK before continuing:**

   ```
   Pipeline: {spec-name}
   Scope:    {light|full}
   Status:   {status} | Phase: {phase}
   Branch:   {current git branch}
   Progress: {completed}/{total} tasks

   Completed:
   - {task title} ✓  (one line per [x] task from spec)

   Pending:
   - {task title}    (one line per [ ] task from spec)

   Concerns:
   - {concern text}  (extracted from <!-- CONCERN: --> comments in spec; omit section if none)

   Context:
   - Files changed: {output from diff-context.js, summarized as file count + paths}
   - Last agent: {agent type from most recent memory-write entry, if available}

   Next action: {ONE specific next step — e.g., "Dispatch Backend Agent (Wave 2)"}

   Continue?
   ```
```

**Change B — Add `### Pause Handoff` section before INVIOLABLE RULES**

Insert after line 122 (after the `5. **Max 2 retries per agent**` line of Granular Retry Protocol, before `## INVIOLABLE RULES`):

```markdown
### Pause Handoff

When the user pauses the pipeline (explicit request or session end signal):

1. **Update pipeline state** — write the following fields to `.claude/.pipeline-states/{spec-name}.json`:
   - `pausedAt`: ISO timestamp
   - `pauseReason`: user-provided reason or `"user_requested"`
   - `nextAction`: ONE sentence — what to do first on resume (e.g., `"Dispatch Backend Agent (Wave 2) — schema is ready"`)
2. **Write agent memory:**
   ```bash
   echo '{"agent_type":"orchestrator","wave":0,"pipeline":"{spec-name}","summary":"Paused at {phase}. Next: {nextAction}","details":{"pausedAt":"{ISO}","pendingTasks":[...]}}' | node .claude/scripts/memory-write.js
   ```
3. **Confirm to user:**
   ```
   Pipeline paused. State saved.
   Next action on resume: {nextAction}
   ```
```

**Change C — Add `### Next Action Rule` section**

Insert immediately after the `### Pause Handoff` section (before `## INVIOLABLE RULES`):

```markdown
### Next Action Rule

The "Next action" field in both the handoff summary and pause state MUST be exactly ONE specific, actionable sentence. It is the first thing the resumed session will do — no ambiguity allowed.

**Wrong (multiple / vague):**
> "Continue with remaining agents and then run review."
> "Work on backend and frontend tasks."

**Right (single / specific):**
> "Dispatch Backend Agent (Wave 2) — API schema compiled, types ready."
> "Re-run review for frontend — previous review returned 1 CRITICAL issue."
> "Run `node .claude/scripts/sync-registry.js` — schema was modified in Wave 1."
```

---

### 2. `templates/pipeline-config.md`

**Change — Add `## Session Handoff` section after `## Compact Guidance`**

Location: After line 42 (the closing line of Compact Guidance: `> "Heavy analysis complete..."`), before `## Parallel Rules`.

Insert:

```markdown
## Session Handoff

When a pipeline session ends (pause, /compact, or context limit), the runner MUST preserve enough state for the next session to resume without re-reading the entire spec.

| Field | Source | Written to |
|-------|--------|------------|
| `pausedAt` | ISO timestamp | pipeline state JSON |
| `pauseReason` | user message or `"user_requested"` | pipeline state JSON |
| `nextAction` | ONE sentence — first action on resume | pipeline state JSON + agent memory |
| `filesChanged` | diff-context.js output | agent memory summary |
| `lastAgent` | most recent memory-write entry | readable from memory-write log |

**Contract:** On resume, `/resume` reads `nextAction` from pipeline state and presents it as the single next step. If `nextAction` is missing, fall back to inferring from the last `[x]` checkpoint in the spec.

**nextAction must be one sentence.** See `resume/SKILL.md` → Next Action Rule.
```

---

## Insertion Points (exact strings to match)

### SKILL.md

- Change A old_string starts with: `5. **Present summary and ASK before continuing:**`
- Change B+C insertion: before `## INVIOLABLE RULES` (after Granular Retry Protocol block ending `5. **Max 2 retries per agent** — exhausted → STOP + report`)
  - Old string anchor: `\n5. **Max 2 retries per agent** — exhausted → STOP + report\n\n## INVIOLABLE RULES`

### pipeline-config.md

- Change insertion anchor: after the Compact Guidance advisory suggestion line ending `Then use \`/resume\` to continue."\n`
  - Old string: ends with the closing blockquote line and the next section header `## Parallel Rules`

## Validation

After edits: `node --test hooks/__tests__/hooks.test.js` (text-only changes, tests should pass unchanged)
