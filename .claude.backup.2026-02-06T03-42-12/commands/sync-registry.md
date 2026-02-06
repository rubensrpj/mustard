# /sync-registry - Update Entity Registry

## Trigger

`/sync-registry`

## Description

Scans the project and updates entity-registry.json.

## Action

1. Searches database schemas (Drizzle, Prisma, etc)
2. Searches backend entities (.NET, Node, etc)
3. Updates `.claude/entity-registry.json`

## When to Use

- After creating new entity
- After importing existing code
- To sync after manual changes
