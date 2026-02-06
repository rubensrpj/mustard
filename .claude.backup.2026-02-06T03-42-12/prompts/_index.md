# Prompts Index

This directory contains specialized prompts for agents.

## Available Prompts

- [orchestrator.md](./orchestrator.md)
- [bugfix.md](./bugfix.md)
- [review.md](./review.md)
- [backend.md](./backend.md)

## How to Use

Prompts are automatically loaded by the pipeline when needed.
To delegate tasks, use:

```javascript
Task({
  subagent_type: "general-purpose",
  model: "opus",
  prompt: `
    [appropriate prompt content]

    # TASK
    [task description]
  `
})
```
