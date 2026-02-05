# /mtd-task-analyze - Code Analysis

> Analyzes code in a **separate Task context** (L0 Universal Delegation).
> Use for any code exploration that doesn't fit feature/bugfix pipelines.

## Usage

```
/mtd-task-analyze <scope>
/mtd-task-analyze authentication flow
/mtd-task-analyze "database schema"
```

## What It Does

1. **Delegates** to Task(Explore) - NEVER analyzes in parent context
2. **Explores** the codebase with grepai and file reads
3. **Reports** findings to user

## Pipeline

```
/mtd-task-analyze <scope>
     │
     ▼
┌────────────────────────────────┐
│  Task(Explore)                 │
│  model: haiku                  │
│  description: 🔍 Analyze...    │
└──────────────┬─────────────────┘
               │
               ▼
         Report findings
```

## Implementation

```javascript
// CRITICAL: Always delegate - never analyze in parent context
Task({
  subagent_type: "Explore",
  model: "haiku",
  description: `🔍 Analyze: ${scope}`,
  prompt: `
# 🔍 CODE ANALYSIS TASK

## Scope
${scope}

## Instructions
1. Use grepai_search for semantic search
2. Use grepai_trace_callers/callees for dependencies
3. Read relevant files
4. Document patterns found
5. Identify key components

## Output Format
- **Overview**: Brief summary
- **Key Files**: List with descriptions
- **Patterns Found**: Code patterns identified
- **Dependencies**: How components connect
- **Suggestions**: Next steps if applicable
  `
})
```

## Arguments

| Argument | Description | Example |
|----------|-------------|---------|
| `<scope>` | What to analyze | `auth flow`, `"payment module"` |

## Examples

```bash
# Analyze a flow
/mtd-task-analyze authentication flow

# Analyze a module
/mtd-task-analyze "payment processing"

# Analyze patterns
/mtd-task-analyze error handling patterns

# Analyze dependencies
/mtd-task-analyze Contract entity dependencies
```

## Output

```
🔍 Analyzing: authentication flow

Task(Explore): Searching codebase...
  - Found: src/services/auth.ts
  - Found: src/middleware/jwt.ts
  - Tracing callers of AuthService...

Analysis Complete:

## Overview
The authentication system uses JWT tokens with refresh token rotation...

## Key Files
- src/services/auth.ts - Main auth service
- src/middleware/jwt.ts - JWT validation middleware
- src/routes/auth.ts - Auth endpoints

## Patterns
- Token refresh rotation
- Role-based access control
- Session management via Redis

## Dependencies
AuthService → JwtService → RedisCache
```

## L0 Enforcement

**CRITICAL**: This command enforces L0 Universal Delegation:
- Parent context does NOT read code
- Parent context does NOT analyze patterns
- Parent context ONLY coordinates and presents results
- ALL analysis happens in the Task(Explore) context

## Related Commands

| Command | Description |
|---------|-------------|
| `/mtd-task-review` | Code review with quality checks |
| `/mtd-task-refactor` | Refactoring with plan |
| `/mtd-task-docs` | Documentation generation |
| `/mtd-pipeline-feature` | Full feature implementation |

## See Also

- [enforcement.md](../../core/enforcement.md) - L0 Universal Delegation rule
- [orchestrator.md](../../prompts/orchestrator.md) - Orchestrator delegation rules
