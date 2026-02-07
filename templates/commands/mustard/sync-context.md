# /sync-context - Load Project Context

## Trigger

`/sync-context`
`/sync-context --refresh`

## Description

Discovers and caches project context for faster implementations.

## What It Does

1. Reads `.claude/context/*.md` for user-provided context
2. Reads `.claude/CLAUDE.md` for project rules and conventions
3. Reads `.claude/entity-registry.json` for entity mappings
4. Uses grepai to discover code patterns (services, repos, components)
5. Stores all in memory MCP as entities

## When It Runs

- **Automatically** at the start of /feature or /bugfix (if context is missing or stale)
- **Manually** when you run /sync-context
- **Force refresh** with /sync-context --refresh

## Context Sources

| Source | Entity Type | Content |
|--------|-------------|---------|
| `.claude/context/*.md` | `UserContext:*` | User-provided specs, tips, examples |
| `.claude/CLAUDE.md` | `ProjectContext` | Stacks, naming, conventions |
| `.claude/entity-registry.json` | `EntityRegistry` | Entity mappings |
| `.claude/core/enforcement.md` | `EnforcementRules` | L0-L9 rules |
| grepai discovery | `CodePattern:*` | Services, repos, components |

## Output

```
✅ Context loaded successfully

Project Context:
- Type: monorepo
- Stacks: dotnet:9.0, react:19.x

User Context (3 files):
- architecture.md
- business-rules.md
- tips.md

Code Patterns:
- service (Backend/Services/ContractService.cs)
- repository (Backend/Repositories/ContractRepository.cs)
```

## Context Refresh Strategy

| Trigger | Action |
|---------|--------|
| Context > 24h old | Auto-refresh on /feature or /bugfix |
| `/sync-context --refresh` | Force full refresh |
| `/sync-registry` | Refresh only EntityRegistry |

## See Also

- [context/README.md](../context/README.md) - How to create context files
- [/feature](./feature.md) - Feature pipeline (auto-loads context)
- [/bugfix](./bugfix.md) - Bugfix pipeline (auto-loads context)
