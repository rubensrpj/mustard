# Prompts Index

This directory contains **reference pointers** to specialized agent prompts.

> Actual content lives in `context/{agent}.context.md` (compiled from `context/{agent}/{agent}.core.md`).

## Available Prompts

| Prompt | Context | Summary |
|--------|---------|---------|
| [orchestrator.md](./orchestrator.md) | `context/orchestrator.context.md` | Pipeline orchestration and Task delegation |
| [backend.md](./backend.md) | `context/backend.context.md` | .NET 9 + FastEndpoints implementation |
| [frontend.md](./frontend.md) | `context/frontend.context.md` | React 19 + Next.js 16 implementation |
| [database.md](./database.md) | `context/database.context.md` | Drizzle ORM + PostgreSQL schemas |
| [review.md](./review.md) | `context/review.context.md` | Code review and validation |
| [bugfix.md](./bugfix.md) | `context/bugfix.context.md` | Bug diagnosis and fix |

## How It Works

Prompts here are **reference only**. The orchestrator passes context inline via Task tool:

```javascript
Task({
  subagent_type: "general-purpose",
  model: "opus",
  prompt: `[context from context/{agent}.context.md] + [task]`
})
```
