# Project Context

Place markdown files here to provide context to Claude during implementations.

## Purpose

Files in this folder are loaded into memory MCP at the start of `/mtd-pipeline-feature` or `/mtd-pipeline-bugfix` pipelines.
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

Files are automatically loaded at the start of `/mtd-pipeline-feature` or `/mtd-pipeline-bugfix` pipelines.
Each file is stored as a `UserContext:{filename}` entity in memory MCP.

## Memory MCP Structure

Each file is stored as a `UserContext:{filename}` entity:

```javascript
{
  name: "UserContext:architecture",
  entityType: "user-context",
  observations: [
    "file: .claude/context/architecture.md",
    "title: Architecture",
    "content: ## Layers\n- Database (Drizzle)\n..."
  ]
}
```

## Example Files

### architecture.md

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

## Rules
- Services do NOT access DbContext directly
- Use UnitOfWork for transactions
```

### business-rules.md

```markdown
# Business Rules

## Orders
- Order must have at least one item
- Order total = sum of item values
- Status transitions: Draft -> Active -> Completed

## Customers
- Customer code must be unique
- Customer cannot be deleted if has orders
```

### tips.md

```markdown
# Project Tips

## Common Patterns
- Use `useOptimistic` for form submissions
- Always validate TenantId in services
- Use `[FromRoute]` for ID parameters

## Gotchas
- Don't forget to add new entities to Registry
- Run migrations after schema changes
```

## Manual Refresh

To force a context refresh, use:

```bash
/mtd-sync-context --refresh
```

## See Also

- [/mtd-sync-context](../commands/mtd-sync-context.md) - Manual context loading
- [/mtd-pipeline-feature](../commands/mtd-pipeline-feature.md) - Feature pipeline
- [pipeline.md](../core/pipeline.md) - Pipeline documentation
