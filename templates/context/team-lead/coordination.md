# Team Coordination Patterns

> Context for Team Lead when using Agent Teams mode.

## Spawning Teammates

### When to Spawn

| Layer | Spawn If | Prompt to Load |
|-------|----------|----------------|
| Database | Schema changes needed | `.claude/prompts/database.md` |
| Backend | API/services needed | `.claude/prompts/backend.md` |
| Frontend | UI components needed | `.claude/prompts/frontend.md` |
| Review | After implementation | `.claude/prompts/review.md` |

### Spawn Command Pattern

```text
Spawn a {Role} Specialist teammate with prompt:

"You are the {Role} Specialist for this project.

## Context
Read your compiled context file: .claude/prompts/{role}.context.md

## Your Tasks
{list of tasks from task list}

## Coordination
- Message {other teammate} when {dependency} is ready
- Mark tasks complete when done
- Message Team Lead if blocked"
```

### Spawn Order (Dependencies)

```text
1. Database (if needed) - No dependencies
2. Backend - Depends on Database schema
3. Frontend - Depends on Backend types/DTOs
4. Review - Depends on all implementation
```

## Messaging Patterns

### To Specific Teammate

Use when:
- Notifying about completed dependency
- Requesting information
- Coordinating handoffs

```text
Message Backend teammate:
"Database schema for Invoice is ready at src/schema/invoice.ts.
You can proceed with endpoints implementation."
```

### Broadcast to All

Use sparingly (token cost scales with team size):
- Critical blockers affecting everyone
- Project-wide decisions
- Final status updates

```text
Broadcast to team:
"Spec has been approved. All teammates can begin implementation."
```

## Delegate Mode

Enable delegate mode (Shift+Tab) to:
- Prevent Team Lead from implementing code directly
- Focus on coordination and task management
- Ensure all work is done by specialists

## Monitoring Progress

### Check Task Status

Regularly review the shared task list:
- Which tasks are in progress?
- Which tasks are blocked?
- Are dependencies being respected?

### Handle Blockers

When a teammate is blocked:
1. Identify the blocker
2. Message the teammate who can resolve it
3. Update task dependencies if needed

## Team Cleanup

After all tasks complete:

```text
1. Verify all tasks are marked complete
2. Ask each teammate to shut down
3. Clean up the team
4. Report final status to user
```
