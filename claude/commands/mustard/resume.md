# /resume - Resume Pipeline

> Resumes an active pipeline, loading complete context.
> **v2.3** - Context refresh if stale.

## Usage

```
/resume
/resume [pipeline-name]
```

## What It Does

1. **Checks** context (refresh if > 24h old)
2. **Searches** for active pipeline via memory MCP
3. **Loads** spec and checklist
4. **Shows** current state
5. **Asks** how to continue

## Implementation (Memory MCP)

```javascript
// 0. Check and update context if needed
const context = await mcp__memory__search_nodes({
  query: "ProjectContext loaded"
});

const isStale = !context.entities?.length || context.entities[0]?.observations?.some(obs => {
  if (obs.startsWith('loaded:')) {
    const loadedDate = new Date(obs.replace('loaded:', '').trim());
    const now = new Date();
    const hoursDiff = (now - loadedDate) / (1000 * 60 * 60);
    return hoursDiff > 24;
  }
  return false;
});

if (isStale) {
  console.log("📚 Context outdated, reloading...");
  // Auto-refresh context (see /sync-context)
}

// 1. Search for active pipeline
const result = await mcp__memory__search_nodes({
  query: "pipeline phase"
});

if (!result.entities.length) {
  return `ℹ️ No active pipeline.

Use /feature <name> to start a new pipeline.
Use /bugfix <error> to fix a bug.`;
}

// 2. Load details
const pipeline = result.entities[0];
const specName = pipeline.name.replace('Pipeline:', 'Spec:');

const details = await mcp__memory__open_nodes({
  names: [pipeline.name, specName]
});

// 3. Extract information
const phase = extractObservation(pipeline, 'phase');
const objective = extractObservation(pipeline, 'objective');
const started = extractObservation(pipeline, 'started');
const checklist = extractChecklist(details.spec);

// 4. Present summary
return `📋 Active Pipeline: ${pipeline.name}

**Phase:** ${phase}
**Started:** ${started}
**Objective:** ${objective}

**Checklist:**
${checklist}

**Next steps (phase ${phase}):**
${getNextSteps(phase)}

Continue from where you left off?`;
```

## Resume Flow

```
[New session/conversation]
        ↓
/resume
        ↓
memory MCP: search_nodes
        ↓
Pipeline found?
    ├─ NO → "No active pipeline"
    └─ YES → open_nodes (load details)
              ↓
          Show state
              ↓
          "Continue?"
```

## Output by Phase

### Phase: explore

```
📋 Active Pipeline: Pipeline:add-email-customer

**Phase:** explore
**Started:** 2026-02-05T10:00:00Z
**Objective:** Add email field to Customer

**Identified files:**
- [ ] {DatabaseProject}/src/schema/customer.ts
- [ ] {BackendProject}/Modules/Customer/...

**Next steps:**
1. Finish analysis with grepai
2. Create spec.md
3. Present for approval

Continue analysis?
```

### Phase: implement

```
📋 Active Pipeline: Pipeline:add-email-customer

**Phase:** implement
**Started:** 2026-02-05T10:00:00Z
**Approved:** 2026-02-05T11:30:00Z
**Objective:** Add email field to Customer

**Checklist:**
- [x] Database: schema updated
- [ ] Backend: endpoints pending
- [ ] Frontend: CRUD pending

**Next steps:**
1. Implement backend
2. Implement frontend
3. Run /validate

Continue implementation?
```

### No Pipeline

```
ℹ️ No active pipeline.

Recently completed pipelines:
- 2026-02-04: solid-user (completed)
- 2026-02-03: add-validation (completed)

Use /feature <name> to start a new pipeline.
Use /bugfix <error> to fix a bug.
```

## Arguments

| Argument | Description | Optional |
|----------|-------------|----------|
| `[name]` | Specific pipeline name | Yes |

If name not specified, searches for any active pipeline.

## Use Cases

### 1. Resume after break

```
User: /resume
Claude: [Loads Pipeline:add-email from memory]
        "Resuming add-email pipeline.
         Phase: implement. Backend pending.
         Continue?"
```

### 2. Resume in new session

```
[New conversation]
User: "Continue what you were doing"
Claude: [automatic /resume]
        "Active pipeline found: solid-refactor
         Phase: explore. Spec in progress."
```

### 3. Check for active pipeline

```
User: /resume
Claude: "No active pipeline.
         Use /feature to start."
```

## Auto Loading

Claude can automatically call `/resume` when detecting:
- Messages like "continue", "resume", "where were we"
- New session start when there's an active pipeline

## Notes

- **Context auto-refresh** if > 24h old (before resuming)
- Context loaded via **memory MCP**, not files
- Checklist extracted from spec observations
- Phase determines which actions are allowed
- Multiple simultaneous pipelines not supported

## See Also

- [/feature](./feature.md) - Start feature pipeline
- [/approve](./approve.md) - Approve spec
- [/complete](./complete.md) - Finalize pipeline
- [/sync-context](./sync-context.md) - Manually load context
