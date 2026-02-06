# /sync-context - Carregar Contexto

> Discovers and caches project context for faster implementations.

## Usage

```
/sync-context
/sync-context --refresh
```

## What It Does

1. Reads `.claude/context/*.md` for user-provided context
2. Reads `.claude/CLAUDE.md` for project rules and conventions
3. Reads `.claude/entity-registry.json` for entity mappings
4. Uses grepai to discover code patterns (services, repos, components)
5. Stores all in memory MCP as entities

## When It Runs

- **Automatically** at the start of `/feature` or `/bugfix` (if context is missing or stale)
- **Manually** when you run `/sync-context`
- **Force refresh** with `/sync-context --refresh`

## Context Sources

| Source | Entity Type | Content |
|--------|-------------|---------|
| `.claude/context/*.md` | `UserContext:*` | User-provided specs, tips, code examples |
| `.claude/CLAUDE.md` | `ProjectContext` | Stacks, naming, conventions |
| `.claude/entity-registry.json` | `EntityRegistry` | Entity mappings |
| `.claude/core/enforcement.md` | `EnforcementRules` | L0-L9 rules |
| grepai discovery | `CodePattern:*` | Services, repos, components |

## Implementation

### Step 1: Check Existing Context

```javascript
// Check if context already loaded
const result = await mcp__memory__search_nodes({
  query: "ProjectContext loaded"
});

// Check freshness (24h threshold)
const isStale = result.entities?.[0]?.observations?.some(obs => {
  if (obs.startsWith('loaded:')) {
    const loadedDate = new Date(obs.replace('loaded:', '').trim());
    const now = new Date();
    const hoursDiff = (now - loadedDate) / (1000 * 60 * 60);
    return hoursDiff > 24;
  }
  return false;
}) ?? true;

// If --refresh or stale, proceed with loading
```

### Step 2: Delete Existing Context (if --refresh)

```javascript
// Clean up existing context entities
await mcp__memory__delete_entities({
  entityNames: [
    "ProjectContext:current",
    "EntityRegistry:current",
    "EnforcementRules:current"
  ]
});

// Also delete UserContext and CodePattern entities
const existing = await mcp__memory__search_nodes({ query: "UserContext CodePattern" });
if (existing.entities?.length) {
  await mcp__memory__delete_entities({
    entityNames: existing.entities.map(e => e.name)
  });
}
```

### Step 3: Load User Context Files

```javascript
// Scan .claude/context/ folder
const contextFiles = await Glob({ pattern: ".claude/context/**/*.md" });

// Load each file (max 20 files, max 500 lines each)
for (const file of contextFiles.slice(0, 20)) {
  const content = await Read({ file_path: file });
  const filename = file.replace('.claude/context/', '').replace('.md', '');
  const title = extractTitle(content) || filename;

  // Truncate to 500 lines
  const truncatedContent = content.split('\n').slice(0, 500).join('\n');

  await mcp__memory__create_entities({
    entities: [{
      name: `UserContext:${filename}`,
      entityType: "user-context",
      observations: [
        `file: ${file}`,
        `title: ${title}`,
        `content: ${truncatedContent}`
      ]
    }]
  });
}
```

### Step 4: Load Project Context

```javascript
// Read CLAUDE.md for project info
const claudeMd = await Read({ file_path: ".claude/CLAUDE.md" });

// Extract key information
const projectType = extractProjectType(claudeMd);
const stacks = extractStacks(claudeMd);
const naming = extractNaming(claudeMd);

await mcp__memory__create_entities({
  entities: [{
    name: "ProjectContext:current",
    entityType: "project-context",
    observations: [
      `type: ${projectType}`,
      `stacks: ${stacks.join(', ')}`,
      `naming.classes: ${naming.classes}`,
      `naming.files: ${JSON.stringify(naming.files)}`,
      `naming.folders: ${naming.folders}`,
      `loaded: ${new Date().toISOString()}`
    ]
  }]
});
```

### Step 5: Load Entity Registry

```javascript
// Read entity registry
const registry = await Read({ file_path: ".claude/entity-registry.json" });
const parsed = JSON.parse(registry);

await mcp__memory__create_entities({
  entities: [{
    name: "EntityRegistry:current",
    entityType: "entity-registry",
    observations: [
      `version: ${parsed._meta?.version || '1.0'}`,
      ...Object.entries(parsed._p || {}).map(([key, val]) => `patterns.${key}: ${val}`),
      `entities: ${Object.keys(parsed.e || {}).join(',')}`
    ]
  }]
});
```

### Step 6: Load Enforcement Rules

```javascript
// Read enforcement rules
const enforcement = await Read({ file_path: ".claude/core/enforcement.md" });

// Extract rules
const rules = extractRules(enforcement);

await mcp__memory__create_entities({
  entities: [{
    name: "EnforcementRules:current",
    entityType: "enforcement",
    observations: rules.map(r => `L${r.level}: ${r.name} - ${r.description}`)
  }]
});
```

### Step 7: Discover Code Patterns (grepai)

```javascript
// Service pattern
const services = await grepai_search({ query: "service layer business logic dependency injection" });
if (services.results?.length) {
  const sample = await Read({ file_path: services.results[0].file, limit: 50 });
  await mcp__memory__create_entities({
    entities: [{
      name: "CodePattern:service",
      entityType: "code-pattern",
      observations: [
        "type: service",
        `file: ${services.results[0].file}`,
        `sample: ${sample.slice(0, 2000)}`
      ]
    }]
  });
}

// Repository pattern
const repos = await grepai_search({ query: "repository pattern data access CRUD" });
// ... similar for repository

// Component pattern (frontend)
const components = await grepai_search({ query: "React component hooks state management" });
// ... similar for component
```

### Step 8: Create Relations

```javascript
// Link context entities to project
await mcp__memory__create_relations({
  relations: [
    { from: "ProjectContext:current", to: "EntityRegistry:current", relationType: "has_registry" },
    { from: "ProjectContext:current", to: "EnforcementRules:current", relationType: "has_rules" },
    // UserContext relations
    ...userContextNames.map(name => ({
      from: "ProjectContext:current",
      to: name,
      relationType: "has_context"
    })),
    // CodePattern relations
    ...codePatternNames.map(name => ({
      from: "ProjectContext:current",
      to: name,
      relationType: "has_pattern"
    }))
  ]
});
```

## Output

### Success

```
✅ Context loaded successfully

Project Context:
- Type: monorepo
- Stacks: dotnet:9.0, react:19.x, drizzle:0.44.x

User Context (5 files):
- architecture.md
- business-rules.md
- tips.md
- service-example.md
- component-example.md

Code Patterns:
- service (Backend/Modules/Contract/Services/ContractService.cs)
- repository (Backend/Modules/Contract/Repositories/ContractRepository.cs)
- component (Frontend/src/features/contract/components/ContractForm.tsx)

Entity Registry:
- 15 entities registered
- Patterns: db, be, fe

Enforcement Rules:
- L0-L9 loaded
```

### Already Loaded

```
ℹ️ Context already loaded (2h ago)

Use /sync-context --refresh to force reload.
```

## Using Context During Implementation

```javascript
// Search for context
const context = await mcp__memory__search_nodes({
  query: "UserContext architecture CodePattern service"
});

// Open specific entities
const details = await mcp__memory__open_nodes({
  names: ["UserContext:architecture", "CodePattern:service"]
});

// Access in agent prompts
// Context is instantly available without re-reading files
```

## Memory Size

| Entity | Typical Size |
|--------|-------------|
| ProjectContext | ~500 bytes |
| EnforcementRules | ~800 bytes |
| EntityRegistry | ~2KB |
| UserContext (up to 20 files) | ~20KB total |
| CodePattern (5x) | ~5KB total |

**Total: ~30KB** - manageable memory footprint

## Context Refresh Strategy

| Trigger | Action |
|---------|--------|
| Context > 24h old | Auto-refresh on /feature or /bugfix |
| `/sync-context --refresh` | Force full refresh |
| `/sync-registry` | Refresh only EntityRegistry |

## Notes

- Context is **lazy loaded** - only loads when needed
- **Max 20 files** from context folder
- **Max 500 lines** per file (truncated)
- Refresh automatically when older than **24 hours**
- grepai patterns are **discovered**, not hardcoded

## See Also

- [/feature](./feature.md) - Feature pipeline (auto-loads context)
- [/bugfix](./bugfix.md) - Bugfix pipeline (auto-loads context)
- [context/README.md](../context/README.md) - How to create context files
- [pipeline.md](../core/pipeline.md) - Pipeline documentation
