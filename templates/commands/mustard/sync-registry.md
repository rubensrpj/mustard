# /sync-registry - Sync Registry

> Syncs the Entity Registry after creating/modifying entities.

## Usage

```
/sync-registry
```

## When to Use

- New entity created
- Entity renamed
- Sub-entity added
- Enum added
- Changes to existing layers

## What It Does

1. Scans Database schemas
2. Scans Backend entities
3. Scans Frontend features
4. Updates `.claude/entity-registry.json`
5. Increments counter and timestamp

## Registry Format

```json
{
  "_m": { "v": "2.1", "n": 35, "at": "..." },
  "_k": { /* legend */ },
  "_p": { /* path patterns */ },
  "_i": { /* integration points */ },
  "e": { /* entities */ }
}
```

## L6 Rule

This command implements L6 enforcement rule:

> After creating/modifying entities, sync `.claude/entity-registry.json`.

## See Also

- [entity-registry-spec.md](../core/entity-registry-spec.md)
- [enforcement.md](../core/enforcement.md) - L6 Rule
