# /resume - Resume Pipeline

## Trigger

`/resume`

## Description

Resumes a pipeline that was interrupted.

## Action

1. Finds active pipeline
2. Identifies last completed phase
3. Continues from where it stopped

## When to Use

- After restarting Claude session
- After accidental interruption
- To continue work from another session

```
User: /resume

Claude: 🔄 Resuming pipeline "add-email-partner"
        Last phase: IMPLEMENT
        Continuing with Backend...
```
