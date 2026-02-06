# Project Context

Place markdown files here to provide context to Claude during implementations.

## Purpose

Files in this folder are loaded into memory MCP at the start of `/feature` or `/bugfix` pipelines.
This gives Claude instant access to project specifications, architecture decisions, and patterns.

## Supported Files

Any `.md` file placed in this folder will be automatically loaded.

**Suggested files:**
- `project-spec.md` - Project overview and specifications
- `architecture.md` - Architecture decisions and patterns
- `business-rules.md` - Domain-specific rules and logic
- `api-guidelines.md` - API design guidelines
- `tips.md` - Project-specific tips for Claude
- `service-example.md` - Code example for services
- `component-example.md` - Code example for components

## Rules

1. **Markdown only** - Only `.md` files are loaded
2. **Keep files focused** - One topic per file
3. **Use headers** - Claude uses headers to understand structure
4. **Max 500 lines** - Longer files are truncated
5. **Max 20 files** - Total limit for loaded files

## How It Works

Files are automatically loaded at the start of `/feature` or `/bugfix` pipelines.
Each file is stored as a `UserContext:{filename}` entity in memory MCP.

## Example: architecture.md

```markdown
# Architecture

## Layers
- Database: Drizzle ORM with PostgreSQL
- Backend: .NET 9 with FastEndpoints
- Frontend: React 19 with TanStack Query

## Patterns
- Repository pattern for data access
- Services for business logic
- DTOs for API contracts
```

## Manual Refresh

To force a context refresh, use:

```
/sync-context --refresh
```

## See Also

- [/sync-context](../commands/mustard/sync-context.md) - Manual context loading
- [/feature](../commands/mustard/feature.md) - Feature pipeline
- [pipeline.md](../core/pipeline.md) - Pipeline documentation
