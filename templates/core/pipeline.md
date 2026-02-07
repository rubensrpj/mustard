# Development Pipeline

## Flow

```
User request
      │
      ▼
Intent Classification
(feature / bugfix / simple)
      │
      ▼
AUTO-SYNC (Phase 0)
      │
      ▼
EXPLORE (analysis)
      │
      ▼
SPEC (approval)
      │
      ▼
IMPLEMENT (delegation)
      │
      ▼
REVIEW (conditional)
      │
      ▼
COMPLETED
```

## Entry Points

- `/feature <name>` — Pipeline feature
- `/bugfix <error>` — Pipeline bugfix
- Direct request — Claude classifies intent automatically

## Reference

Full pipeline details: `context/orchestrator.context.md`
