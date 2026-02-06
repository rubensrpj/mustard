# Project Context

Place markdown files here to provide context to Claude during implementations.

## Purpose

Files in this folder are loaded into memory MCP at the start of `/feature` or `/bugfix` pipelines.
This gives Claude instant access to project specifications, architecture decisions, and patterns.

## Structure

```text
.claude/context/
+-- shared/           # Loaded by ALL agents
|   +-- conventions.md
+-- backend/          # Loaded by Backend Specialist
|   +-- patterns.md
+-- frontend/         # Loaded by Frontend Specialist
|   +-- patterns.md
+-- database/         # Loaded by Database Specialist
|   +-- patterns.md
+-- orchestrator/     # Loaded by Orchestrator (optional)
+-- review/           # Loaded by Review Specialist (optional)
+-- bugfix/           # Loaded by Bugfix Specialist (optional)
+-- team-lead/        # Loaded by Team Lead (Agent Teams mode)
|   +-- coordination.md
|   +-- task-list.md
```

## How Agents Load Context

Each agent loads:
1. All files from `context/shared/`
2. All files from `context/{agent}/`

```javascript
const sharedFiles = Glob(".claude/context/shared/*.md");
const agentFiles = Glob(".claude/context/{agent}/*.md");
```

## Included Files (Templates)

### shared/conventions.md
Common naming conventions across all layers (entities, tables, components, hooks).

### backend/patterns.md
Stack-specific backend patterns (.NET, Node.js, Python, etc.).

### frontend/patterns.md
Stack-specific frontend patterns (React, Vue, Angular, etc.).

### database/patterns.md
ORM-specific patterns (Drizzle, Prisma, TypeORM, Entity Framework, etc.).

## Custom Files

You can add custom files to any folder:

- `shared/architecture.md` - Architecture decisions
- `shared/business-rules.md` - Domain-specific rules
- `backend/service-example.md` - Service code example
- `frontend/component-example.md` - Component code example
- `database/schema-example.md` - Schema code example

## Rules

1. **Markdown only** - Only `.md` files are loaded
2. **Keep files focused** - One topic per file
3. **Use headers** - Claude uses headers to understand structure
4. **Max 500 lines** - Longer files are truncated
5. **Max 20 files** - Total limit for loaded files per agent

## Memory MCP Structure

Each file is stored as an `AgentContext:{agent}:{filename}` entity:

```javascript
{
  name: "AgentContext:backend:patterns",
  entityType: "agent-context",
  observations: [content]
}
```

## Manual Refresh

To force a context refresh, use:

```bash
/sync-context --refresh
```

## See Also

- [/sync-context](../commands/mustard/sync-context.md) - Manual context loading
- [/feature](../commands/mustard/feature.md) - Feature pipeline
- [pipeline.md](../core/pipeline.md) - Pipeline documentation
