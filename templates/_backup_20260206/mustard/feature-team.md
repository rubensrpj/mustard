# /feature-team - Feature Pipeline (Agent Teams)

> Uses Agent Teams for parallel implementation with peer-to-peer coordination.
> **Experimental** - Requires Agent Teams to be enabled in settings.

## Usage

```bash
/feature-team <name>
/feature-team Invoice
/feature-team "Stripe Integration"
```

## What It Does

1. **Compiles contexts** for all agents (mandatory first step)
2. **Verifies** Agent Teams is enabled
3. **Creates team** with you as Team Lead
4. **Spawns teammates** based on detected stacks:
   - Database teammate (if ORM detected)
   - Backend teammate (always)
   - Frontend teammate (if UI framework detected)
5. **Creates task list** with dependencies
6. **Coordinates implementation** via shared task list
7. **Spawns review teammate** after implementation
8. **Cleans up team** when complete

## Prerequisites

Enable Agent Teams in `.claude/settings.json`:

```json
{
  "env": {
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1"
  }
}
```

## Pipeline (Agent Teams)

```text
/feature-team <name>
     │
     ▼
┌────────────────────────────────┐
│  YOU as TEAM LEAD              │
│  + team-lead.md prompt         │
│  + delegate mode (Shift+Tab)   │
└──────────────┬─────────────────┘
               │
     ┌─────────┼─────────┐
     ▼         ▼         ▼
 Database   Backend   Frontend
 Teammate   Teammate  Teammate
 (spawn)    (spawn)   (spawn)
     │         │         │
     │    ┌────┴────┐    │
     │    ▼         ▼    │
     └──► SHARED    ◄────┘
          TASK LIST
               │
               ▼
         Review Teammate
               │
               ▼
         TEAM CLEANUP
```

## Implementation

### Phase 0: Compile Contexts (MANDATORY FIRST STEP)

**BEFORE doing anything else, you MUST compile all agent contexts:**

#### Step 0.1: Get current commit hash

```bash
git rev-parse --short HEAD
```

Save the result as `currentHash`.

#### Step 0.2: For each agent, check and compile

For each agent in: `backend`, `frontend`, `database`, `review`, `team-lead`:

1. Use Glob to check if `.claude/prompts/{agent}.context.md` exists
2. If exists, Read the file and check if `compiled-from-commit: {hash}` matches `currentHash`
3. If missing OR hash differs:
   - Use Glob to find all `.md` files in `.claude/context/shared/` (exclude README)
   - Use Glob to find all `.md` files in `.claude/context/{agent}/` (exclude README)
   - Read each file's content
   - Synthesize into a single compiled context (remove duplicates, consolidate, optimize)
   - Write to `.claude/prompts/{agent}.context.md` with format:

```markdown
<!-- compiled-from-commit: {currentHash} -->
<!-- sources: {list of source files} -->
<!-- compiled-at: {ISO timestamp} -->

# {Agent} Context

{synthesized content}
```

#### Step 0.3: Report compilation status

```text
✅ Context compiled for all agents (commit: {hash})
```

> ⚠️ **DO NOT SKIP THIS STEP.** All teammates depend on compiled contexts.

### Phase 1: Verify Agent Teams

Check that Agent Teams is enabled before proceeding.

### Phase 2: Create Task List

Create structured task list with dependencies:

```text
## Task List: {name}

### Task 1: [Database] Create schema for {entity}
- Status: pending
- Dependencies: none

### Task 2: [Backend] Implement {entity} endpoints
- Status: pending
- Dependencies: Task 1

### Task 3: [Frontend] Create {entity} components
- Status: pending
- Dependencies: Task 2

### Task 4: [Review] Validate implementation
- Status: pending
- Dependencies: Task 2, Task 3
```

### Phase 3: Spawn Team

Enable delegate mode (Shift+Tab), then spawn teammates:

```text
Spawn a Database Specialist teammate with prompt:
"You are the Database Specialist.
Your tasks: Task 1. Message Backend when schema is ready."

Spawn a Backend Specialist teammate with prompt:
"You are the Backend Specialist.
Your tasks: Task 2. Wait for Database. Message Frontend when types ready."

Spawn a Frontend Specialist teammate with prompt:
"You are the Frontend Specialist.
Your tasks: Task 3. Wait for Backend. Message Team Lead when done."
```

### Phase 4: Coordinate

Monitor task completion:

- Handle messages from teammates
- Resolve blockers
- Track progress in task list
- Notify dependents when tasks complete

### Phase 5: Review

When Tasks 1-3 are complete:

```text
Spawn a Review Specialist teammate with prompt:
"You are the Review Specialist.
Your tasks: Task 4. Review all files. Report findings to Team Lead."
```

### Phase 6: Cleanup

After review is complete:

```text
Ask all teammates to shut down.
Clean up the team.
Report final status.
```

## Comparison: /feature vs /feature-team

| Aspect | /feature (Task) | /feature-team (Teams) |
|--------|-----------------|----------------------|
| Parallelism | Sequential Tasks | True parallel contexts |
| Communication | Report to parent | Peer-to-peer messaging |
| Context | Shared session | Independent per teammate |
| Token Cost | Lower | Higher |
| Coordination | Orchestrator prompt | Shared task list |
| Best For | Simple features | Complex, multi-layer |

## Arguments

| Argument | Description | Example |
|----------|-------------|---------|
| `<name>` | Feature name | `Invoice`, `"User Auth"` |

## Examples

```bash
# New entity
/feature-team Invoice

# Feature with description
/feature-team "Add email field to Person"

# Integration
/feature-team "Payment gateway integration"
```

## Output

### During Execution

```text
Team Lead: Creating task list for Invoice
Team Lead: Spawning Database Specialist...
Team Lead: Spawning Backend Specialist...
Team Lead: Spawning Frontend Specialist...

Database Teammate: Starting Task 1...
Backend Teammate: Waiting for Database...
Frontend Teammate: Waiting for Backend...

Database Teammate: Task 1 complete. Messaging Backend.
Backend Teammate: Starting Task 2...
...
```

### After Completion

```text
Team Lead: All tasks complete.

## Team Complete: Invoice

### Task Summary
| Task | Assignee | Status |
|------|----------|--------|
| Task 1: Database | Database Teammate | completed |
| Task 2: Backend | Backend Teammate | completed |
| Task 3: Frontend | Frontend Teammate | completed |
| Task 4: Review | Review Teammate | completed |

### Review: APPROVED

### Files Created
- src/schema/invoice.ts
- Modules/Invoice/...
- src/features/invoice/...

Team cleanup complete.
```

## Notes

- **Experimental**: Agent Teams has known limitations
- **Token cost**: Significantly higher than Task subagents
- **Best for**: Complex features requiring multi-layer coordination
- **Avoid for**: Simple single-layer changes (use `/feature` instead)
- **Delegate mode**: Press Shift+Tab to prevent Team Lead from coding

## Limitations

- No session resumption with in-process teammates
- Task status can lag (manually update if needed)
- Slow shutdown (teammates finish current work first)
- One team per session
- Higher token cost

## See Also

- [/feature](./feature.md) - Task-based pipeline (lower cost)
- [/bugfix-team](./bugfix-team.md) - Team-based bugfix
- [team-lead.md](../../prompts/team-lead.md) - Team Lead prompt
- [Agent Teams docs](https://code.claude.com/docs/en/agent-teams) - Official documentation
