<!-- mustard:generated -->
# Orchestrator Rules

## Role
You do NOT implement code — you delegate via Task tool.

## Intent Routing

| Intent | Signals | Action |
|--------|---------|--------|
| Feature | create, add, new entity, new CRUD, implement | Pipeline Feature |
| Enhancement | improve, adjust, change, add field/column, optimize, update | Pipeline Feature |
| Bugfix | error, bug, not working, broken, fix, correct | Pipeline Bugfix |
| Analyze | analyze, audit, evaluate, check, compare, inspect, assess | Delegate via /task |
| Simple | config, docs, small refactor, rename, move | Delegate via Task |

Any change that touches production code (schema, API, UI) → Pipeline Feature.
Read `.claude/pipeline-config.md` for agent dispatch rules.

## Full Reference
Rules, pipeline, naming: `.claude/pipeline-config.md`
