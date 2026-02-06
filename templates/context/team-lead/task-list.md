# Task List Patterns

> Patterns for creating and managing the shared task list in Agent Teams mode.

## Task List Format

```text
## Task List: {Feature Name}

### Task 1: [Database] Create schema for {entity}
- Status: pending | in_progress | completed
- Assignee: Database Teammate
- Dependencies: none
- Files: src/schema/{entity}.ts

### Task 2: [Backend] Implement {entity} endpoints
- Status: pending
- Assignee: Backend Teammate
- Dependencies: Task 1
- Files: Modules/{Entity}/Endpoints/, Modules/{Entity}/Services/

### Task 3: [Frontend] Create {entity} components
- Status: pending
- Assignee: Frontend Teammate
- Dependencies: Task 2
- Files: src/features/{entity}/

### Task 4: [Review] Validate implementation
- Status: pending
- Assignee: Review Teammate
- Dependencies: Task 2, Task 3
- Files: All modified files
```

## Task States

| State | Meaning | Who Changes |
|-------|---------|-------------|
| `pending` | Not started, dependencies may not be met | Team Lead creates |
| `in_progress` | Teammate is working on it | Teammate claims |
| `completed` | Work is done and verified | Teammate marks |
| `blocked` | Cannot proceed, needs resolution | Teammate or Lead |

## Dependency Rules

### Sequential Dependencies

```text
Database → Backend → Frontend

Task 1 (Database): no dependencies
Task 2 (Backend): depends on Task 1
Task 3 (Frontend): depends on Task 2
```

### Parallel Work

When tasks have no dependencies on each other:

```text
Task 2 (Backend): depends on Task 1
Task 3 (Frontend - existing types): no dependencies

Both can run in parallel if Frontend uses existing types.
```

### Dependency Notation

```text
- Dependencies: none           # Can start immediately
- Dependencies: Task 1         # Must wait for Task 1
- Dependencies: Task 1, Task 2 # Must wait for both
```

## Task Breakdown Guidelines

### Good Task Size

- One schema file per task
- One module/feature per task
- One component group per task
- Estimated 15-30 minutes of work

### Task Examples

| Layer | Good Task | Bad Task |
|-------|-----------|----------|
| Database | "Create Invoice schema" | "Create all schemas" |
| Backend | "Implement Invoice endpoints" | "Implement entire backend" |
| Frontend | "Create Invoice list and form" | "Create all UI" |

## Claiming Tasks

Teammates claim tasks by:
1. Checking their assigned tasks in the list
2. Verifying dependencies are complete
3. Marking task as `in_progress`
4. Starting work

## Completing Tasks

When a task is done:
1. Mark as `completed`
2. Message dependent teammates
3. Claim next available task (if any)

```text
Message Frontend teammate:
"Task 2 (Backend Invoice endpoints) is complete.
Types are at src/types/invoice.ts.
You can proceed with Task 3."
```

## Handling Blocked Tasks

When blocked:
1. Mark task as `blocked`
2. Add blocker reason
3. Message Team Lead or blocking teammate

```text
Task 2: [Backend] Implement Invoice endpoints
- Status: blocked
- Blocker: Schema missing tenant_id column
- Needs: Database teammate to add column
```
