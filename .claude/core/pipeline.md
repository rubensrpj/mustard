# Development Pipeline

## Flow

```
/feature or /bugfix
         │
         ▼
    EXPLORE (analysis)
         │
         ▼
      SPEC (approval)
         │
         ▼
    IMPLEMENT
    (delegation)
         │
         ▼
    REVIEW
         │
         ▼
    COMPLETED
```

## Commands

| Command | Description |
|---------|-------------|
| /feature <name> | Starts feature pipeline |
| /bugfix <error> | Starts bugfix pipeline |
| /approve | Approves spec for implementation |
| /complete | Finalizes pipeline |
| /resume | Resumes active pipeline |
