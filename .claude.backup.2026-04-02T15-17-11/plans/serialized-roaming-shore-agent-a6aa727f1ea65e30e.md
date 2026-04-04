# Plan: Improve Pause/Resume with Handoff Document (P4)

## Objective

Enhance `/mustard:resume` to produce a structured handoff document on resume, add a pause handoff procedure, and enforce the single-next-action rule. Also add a Session Handoff section to `pipeline-config.md`.

## Files to Modify

1. `templates/commands/mustard/resume/SKILL.md` — three additions
2. `templates/pipeline-config.md` — one new section

---

## File 1: `templates/commands/mustard/resume/SKILL.md`

### Change A — Handoff Document Generation (insert after Step 5 "Present summary and ASK before continuing", before Step 2: Bootstrap)

Insert a new subsection **### Handoff Document** between the summary block and the "Continue?" prompt.

Current Step 5 ends with presenting the summary and asking "Continue?". We want to:
- Expand that summary into the richer handoff format
- Pull in additional context: git diff, agent memory
- Show exactly ONE next action

The current summary block in Step 5 is:
```
Pipeline: {spec-name}
Scope:    {light|full}
Status:   {status} | Phase: {phase}
Progress: {completed}/{total} tasks
Next:     {next agent/wave}
Decisions: {decisions[] if present}

Continue?
```

We will replace it with the full handoff format plus the same "Continue?" prompt.

### Change B — Pause Handoff (new subsection before INVIOLABLE RULES)

Insert a **### Pause Handoff** section describing what to do when a pipeline is paused.

### Change C — Next Action Rule (new subsection before INVIOLABLE RULES, after Pause Handoff)

Insert a **### Next Action Rule** section emphasising that handoff MUST end with exactly ONE next action.

---

## File 2: `templates/pipeline-config.md`

### Change — New section "Session Handoff" after "Compact Guidance"

Add a concise `## Session Handoff` section describing how handoff works at a high level.

---

## Detailed Diff Plan

### SKILL.md Step 5 block replacement

**Old** (lines 26–37 — the summary block):
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

**New** — expand into handoff format with additional data-gathering steps before the display:

```
5. **Compile and present handoff, then ASK before continuing:**

   a. Run `node .claude/scripts/diff-context.js` — capture files-changed count and current branch
   b. Read `.claude/.agent-memory/_index.json` if it exists — take last 5 entries for last-agent and last-action
   c. Compile into handoff format and present:

   ```
   === PIPELINE HANDOFF ===

   Pipeline: {spec-name}
   Phase:    {ANALYZE|PLAN|EXECUTE|CLOSE}
   Started:  {startedAt from pipeline state or spec header}
   Duration: {elapsed since startedAt}

   ## Completed
   - [x] {each completed task from spec}

   ## Pending
   - [ ] {each pending task from spec}

   ## Concerns
   {DONE_WITH_CONCERNS items if any — omit section if none}

   ## Context
   - Branch:      {current git branch from diff-context output}
   - Files changed: {N files from diff-context output}
   - Last agent:  {most recent agent_type from _index.json}
   - Last action: {summary field from most recent _index.json entry}

   ## Next Action
   → {ONE specific next step, e.g. "Dispatch backend agent for task 3 (add /api/users endpoint)"}

   ===
   ```

   Continue from next action, or review first?
```

### SKILL.md — Pause Handoff section (insert before INVIOLABLE RULES)

```markdown
### Pause Handoff

When a pipeline is paused (user ends the session or explicitly pauses):

1. Update `.claude/.pipeline-states/{spec-name}.json`:
   - Set `pausedAt` to current ISO timestamp
   - Set `pauseReason` if the user provided one
   - Set `nextAction` to the specific next step (ONE sentence)
2. Write agent memory for context carry-over:
   ```bash
   echo '{"agent_type":"orchestrator","wave":0,"pipeline":"{spec-name}","summary":"Pipeline paused at {phase}. Next: {action}"}' | node .claude/scripts/memory-write.js
   ```

This ensures `/resume` can compile the handoff without re-analysing.
```

### SKILL.md — Next Action Rule section (insert after Pause Handoff, before INVIOLABLE RULES)

```markdown
### Next Action Rule

The handoff MUST end with exactly ONE next action:
- NOT: "you could do A, B, or C"
- INSTEAD: "→ Dispatch backend agent for task 3 (add /api/users endpoint)"

The orchestrator decides the next step from state — the user should be able to say "go" without re-orienting. The user may override the next action, but the default must be a single clear step.
```

### pipeline-config.md — Session Handoff section (insert after Compact Guidance section, before Parallel Rules)

```markdown
## Session Handoff

When a pipeline pauses or a session ends mid-pipeline:
- Pipeline state is auto-saved (pipeline state JSON + metrics-tracker if enabled)
- On `/resume`: compile handoff from state + spec + agent memory + git diff
- Handoff includes: completed tasks, pending tasks, concerns, context, ONE next action
- Goal: user can resume in <30 seconds without re-reading the spec
- If `pausedAt` and `nextAction` fields are present in pipeline state, use them directly
```

---

## Constraints Check

- No new code — all changes are text/template in SKILL.md and pipeline-config.md
- Handoff is compilable from existing data sources (pipeline state JSON, spec.md, _index.json, diff-context.js)
- Exactly ONE next action — enforced by the Next Action Rule section
- Format is scannable (headers, bullet lists, short lines)
- SKILL.md will stay under 200 lines (adding ~50 lines to a 133-line file = ~183 lines — acceptable)
- pipeline-config.md adding ~8 lines — fine
- Both files already have `<!-- mustard:generated -->` header in the original? Check: SKILL.md does NOT have that header (it's a command SKILL.md, not a generated file) — correct, SKILL.md files are not generated files
- guards.md rule: commands must end with ULTRATHINK — SKILL.md currently ends with INVIOLABLE RULES section (no ULTRATHINK). Checking...

Wait — the guards rule says "DO end every command SKILL.md with `ULTRATHINK`". The current SKILL.md does NOT end with ULTRATHINK. I should NOT add it unless fixing a pre-existing violation is in scope. The task only asks for specific changes — I will leave the ULTRATHINK question as-is (pre-existing state) and focus only on the requested changes.

---

## Execution Order

1. Edit `templates/commands/mustard/resume/SKILL.md`:
   - Replace the Step 5 summary block with the expanded handoff format
   - Insert Pause Handoff section before INVIOLABLE RULES
   - Insert Next Action Rule section after Pause Handoff
2. Edit `templates/pipeline-config.md`:
   - Insert Session Handoff section after Compact Guidance, before Parallel Rules
