# /task-analyze - Code Analysis

> Analyzes code in a **separate Task context** (L0 Universal Delegation).

## Trigger

`/task-analyze <scope>`

## Description

Analyzes code in a separate Task(Explore) context.
Use for any code exploration that doesn't fit feature/bugfix pipelines.

## L0 Enforcement

**CRITICAL**: This command enforces L0 Universal Delegation:
- Parent context does NOT read code
- Parent context ONLY coordinates and presents results
- ALL analysis happens in Task(Explore) context

## Flow

1. **DELEGATE** 🔍
   - Create Task(Explore) with analysis scope
   - Never analyze directly in parent context

2. **REPORT**
   - Present findings to user

## Implementation

```javascript
Task({
  subagent_type: "Explore",
  model: "haiku",
  description: `🔍 Analyze: ${scope}`,
  prompt: `
    # 🔍 CODE ANALYSIS TASK
    ## Scope: ${scope}
    ## Instructions
    1. Use grepai_search for semantic search
    2. Read relevant files
    3. Document patterns found
    4. Report findings clearly
  `
})
```

## Examples

```bash
/task-analyze authentication flow
/task-analyze "database schema"
/task-analyze error handling patterns
```
