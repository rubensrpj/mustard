# /mtd-task-refactor - Code Refactoring

> Refactors code in **separate Task contexts** (L0 Universal Delegation).
> Uses Plan → Approve → Implement flow to ensure safe refactoring.

## Usage

```
/mtd-task-refactor <scope>
/mtd-task-refactor "extract auth service"
/mtd-task-refactor "rename User to Account"
```

## What It Does

1. **Plans** via Task(Plan) - analyzes scope and proposes strategy
2. **Awaits approval** - presents plan to user
3. **Implements** via Task(general-purpose) - executes refactoring
4. **Validates** - runs build/tests

## Pipeline

```
/mtd-task-refactor <scope>
     │
     ▼
┌────────────────────────────────┐
│  Task(Plan)                    │
│  model: sonnet                 │
│  description: 📋 Plan...       │
└──────────────┬─────────────────┘
               │
               ▼
         AWAIT APPROVAL
               │
               ▼
┌────────────────────────────────┐
│  Task(general-purpose)         │
│  model: opus                   │
│  description: ⚙️ Execute...    │
└──────────────┬─────────────────┘
               │
               ▼
         /mtd-validate-build
```

## Implementation

### Phase 1: Plan

```javascript
// CRITICAL: Always delegate planning
Task({
  subagent_type: "Plan",
  model: "sonnet",
  description: `📋 Plan refactor: ${scope}`,
  prompt: `
# 📋 REFACTORING PLAN TASK

## Scope
${scope}

## Analyze
1. Use grepai_search to find related code
2. Use grepai_trace_* to map dependencies
3. Identify all affected files

## Deliverable
Create a detailed refactoring plan:

### Overview
Brief description of the refactoring

### Current State
How the code is structured now

### Target State
How it should be structured after

### Files to Modify
- file1.ts - what changes
- file2.ts - what changes

### Steps (in order)
1. First change
2. Second change
3. ...

### Risks
- Potential breaking change 1
- Potential breaking change 2

### Validation
How to verify the refactoring worked
  `
})
```

### Phase 2: Execute (after approval)

```javascript
// CRITICAL: Always delegate execution
Task({
  subagent_type: "general-purpose",
  model: "opus",
  description: `⚙️ Execute refactor: ${scope}`,
  prompt: `
# ⚙️ REFACTORING EXECUTION TASK

## Approved Plan
${approvedPlan}

## Instructions
1. Follow the approved plan exactly
2. Make changes incrementally
3. Preserve existing functionality
4. Update imports/references
5. Keep tests passing

## Rules
- Do NOT deviate from the plan
- If you find issues, report them instead of improvising
- Preserve all existing behavior
  `
})
```

## Arguments

| Argument | Description | Example |
|----------|-------------|---------|
| `<scope>` | What to refactor | `"extract service"`, `"rename entity"` |

## Examples

```bash
# Extract a service
/mtd-task-refactor "extract PaymentService from OrderController"

# Rename across codebase
/mtd-task-refactor "rename User entity to Account"

# Split a large file
/mtd-task-refactor "split utils.ts into focused modules"

# Move to new pattern
/mtd-task-refactor "move validation logic to domain layer"
```

## Output

### Planning Phase

```
📋 Planning refactor: extract PaymentService

Task(Plan): Analyzing codebase...

## Refactoring Plan

### Overview
Extract payment logic from OrderController into dedicated PaymentService

### Current State
- OrderController handles orders AND payments
- Payment logic mixed with order logic
- Hard to test payment in isolation

### Target State
- PaymentService handles all payment operations
- OrderController delegates to PaymentService
- Clear separation of concerns

### Files to Modify
- src/controllers/order.ts - Remove payment logic
- src/services/payment.ts - NEW FILE
- src/routes/order.ts - Update DI

### Steps
1. Create PaymentService with interface
2. Move payment methods from OrderController
3. Update OrderController to use PaymentService
4. Update dependency injection
5. Update tests

### Risks
- OrderController tests may need updates
- Any direct payment calls need updating

Awaiting approval... Reply with "approve" or provide feedback.
```

### Execution Phase

```
⚙️ Executing refactor: extract PaymentService

Task(general-purpose): Implementing plan...
  ✓ Created src/services/payment.ts
  ✓ Moved processPayment()
  ✓ Moved refundPayment()
  ✓ Updated OrderController
  ✓ Updated DI container

Running validation...
  ✓ Build passed
  ✓ Type-check passed

✅ Refactoring complete!
```

## L0 Enforcement

**CRITICAL**: This command enforces L0 Universal Delegation:
- Parent context does NOT read code
- Parent context does NOT plan refactoring
- Parent context does NOT execute changes
- Parent context ONLY coordinates and presents results
- ALL work happens in Task(Plan) and Task(general-purpose) contexts

## Related Commands

| Command | Description |
|---------|-------------|
| `/mtd-task-analyze` | Analyze before refactoring |
| `/mtd-task-review` | Review after refactoring |
| `/mtd-validate-build` | Validate changes |
| `/mtd-pipeline-feature` | For larger changes, use full pipeline |

## See Also

- [enforcement.md](../../core/enforcement.md) - L0 Universal Delegation rule
- [orchestrator.md](../../prompts/orchestrator.md) - Orchestrator delegation rules
