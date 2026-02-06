# /task-review - Code Review

> Performs code review in a **separate Task context** (L0 Universal Delegation).

## Trigger

`/task-review <scope>`

## Description

Reviews code quality in a separate Task(general-purpose) context.
Use for QA, SOLID validation, security checks.

## L0 Enforcement

**CRITICAL**: This command enforces L0 Universal Delegation:
- Parent context does NOT read code
- Parent context ONLY coordinates and presents results
- ALL review happens in Task(general-purpose) context

## Flow

1. **DELEGATE** 🔎
   - Create Task(general-purpose) with review prompt
   - Never review directly in parent context

2. **REPORT**
   - Present findings with severity levels

## Implementation

```javascript
Task({
  subagent_type: "general-purpose",
  model: "opus",
  description: `🔎 Review: ${scope}`,
  prompt: `
    # 🔎 CODE REVIEW TASK
    ## Scope: ${scope}
    ## Checklist
    - [ ] SOLID principles
    - [ ] Error handling
    - [ ] Security concerns
    - [ ] Performance issues
    ## Output: [Severity] File:Line - Issue - Suggestion
  `
})
```

## Examples

```bash
/task-review src/services/payment
/task-review "Contract entity"
/task-review "security in auth module"
```
