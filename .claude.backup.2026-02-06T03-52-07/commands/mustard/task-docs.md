# /task-docs - Documentation Generation

> Generates documentation in a **separate Task context** (L0 Universal Delegation).

## Trigger

`/task-docs <scope>`

## Description

Generates documentation in a separate Task(general-purpose) context.
Use for API docs, README updates, or technical documentation.

## L0 Enforcement

**CRITICAL**: This command enforces L0 Universal Delegation:
- Parent context does NOT read code
- Parent context does NOT generate documentation
- ALL work happens in Task(general-purpose) context

## Flow

1. **DELEGATE** 📊
   - Create Task(general-purpose) with docs prompt
   - Never generate docs in parent context

2. **PRESENT**
   - Show generated documentation
   - Ask for approval before saving

## Implementation

```javascript
Task({
  subagent_type: "general-purpose",
  model: "sonnet",
  description: `📊 Docs: ${scope}`,
  prompt: `
    # 📊 DOCUMENTATION TASK
    ## Scope: ${scope}
    ## Instructions
    1. Use grepai to find relevant code
    2. Generate appropriate documentation
    3. Indicate where to save
  `
})
```

## Examples

```bash
/task-docs "API endpoints"
/task-docs "Contract entity"
/task-docs "update README"
```
