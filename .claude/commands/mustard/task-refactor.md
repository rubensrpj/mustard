# /task-refactor - Code Refactoring

> Refactors code in **separate Task contexts** (L0 Universal Delegation).

## Trigger

`/task-refactor <scope>`

## Description

Refactors code using Plan → Approve → Implement flow.
Uses separate Task contexts for planning and execution.

## L0 Enforcement

**CRITICAL**: This command enforces L0 Universal Delegation:
- Parent context does NOT read code
- Parent context does NOT plan or implement
- ALL work happens in Task(Plan) and Task(general-purpose) contexts

## Flow

1. **PLAN** 📋
   - Create Task(Plan) to analyze scope
   - Propose refactoring strategy

2. **APPROVE**
   - Present plan to user
   - Wait for approval

3. **IMPLEMENT** ⚙️
   - Create Task(general-purpose) to execute
   - Apply changes incrementally

4. **VALIDATE**
   - Run build/tests

## Implementation

```javascript
// Phase 1: Plan
Task({
  subagent_type: "Plan",
  model: "sonnet",
  description: `📋 Plan refactor: ${scope}`,
  prompt: `# Plan refactoring for ${scope}...`
})

// Phase 2: Execute (after approval)
Task({
  subagent_type: "general-purpose",
  model: "opus",
  description: `⚙️ Execute refactor: ${scope}`,
  prompt: `# Execute approved plan...`
})
```

## Examples

```bash
/task-refactor "extract PaymentService"
/task-refactor "rename User to Account"
/task-refactor "split large component"
```
