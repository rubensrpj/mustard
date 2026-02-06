# /resume - Resume Pipeline

> Resumes an active pipeline, loading context from checkpoints.
> **v2.6** - Checkpoint-aware resume with compact context.

## Usage

```bash
/resume
/resume [pipeline-name]
/resume --full    # Load full checkpoint details (not just summary)
```

## What It Does

1. **Checks** context (refresh if > 24h old)
2. **Searches** for active pipeline via memory MCP
3. **Loads** latest checkpoint summary (compact)
4. **Loads** spec and checklist
5. **Loads** past learnings (if relevant)
6. **Shows** current state with focused context
7. **Asks** how to continue

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

// 2. Load pipeline details
const pipeline = result.entities[0];
const specName = pipeline.name.replace('Pipeline:', 'Spec:');

// 3. Load latest checkpoint (NEW v2.6)
const checkpoints = await mcp__memory__search_nodes({
  query: `Checkpoint ${pipeline.name}`
});

let checkpointSummary = null;
if (checkpoints.entities.length) {
  // Sort by timestamp, get most recent
  const latest = checkpoints.entities
    .sort((a, b) => b.name.localeCompare(a.name))[0];

  const checkpointDetails = await mcp__memory__open_nodes({
    names: [latest.name]
  });

  checkpointSummary = {
    phase: extractObservation(latest, 'phase'),
    summary: extractObservation(latest, 'summary'),
    keyFiles: extractObservation(latest, 'key_files'),
    decisions: extractObservation(latest, 'decisions'),
    nextSteps: extractObservation(latest, 'next_steps')
  };
}

// 4. Load relevant past learnings (NEW v2.6)
const objective = extractObservation(pipeline, 'objective');
const learnings = await mcp__memory__search_nodes({
  query: `Learning ${objective.split(' ').slice(0, 3).join(' ')}`
});

let relevantLearnings = [];
if (learnings.entities.length) {
  relevantLearnings = learnings.entities.slice(0, 3).map(l => ({
    name: l.name,
    gotchas: extractObservation(l, 'gotchas')
  }));
}

// 5. Load spec and checklist
const details = await mcp__memory__open_nodes({
  names: [pipeline.name, specName]
});

const phase = extractObservation(pipeline, 'phase');
const started = extractObservation(pipeline, 'started');
const checklist = extractChecklist(details.spec);
```

### Return Message

```javascript
return `📋 Resuming Pipeline: ${pipeline.name}

**Phase:** ${phase}
**Started:** ${started}
**Objective:** ${objective}

${checkpointSummary ? `
## 💾 From Last Checkpoint (${checkpointSummary.phase})

${checkpointSummary.summary}

**Key Files:** ${checkpointSummary.keyFiles}

**Decisions Made:** ${checkpointSummary.decisions}

**Next Steps:** ${checkpointSummary.nextSteps}
` : ''}

${relevantLearnings.length ? `
## 📚 Relevant Past Learnings

${relevantLearnings.map(l => `- **${l.name}**: ${l.gotchas}`).join('\n')}
` : ''}

## Checklist
${checklist}

## Next Actions (phase: ${phase})
${getNextSteps(phase)}

Ready to continue?`;
```

## Resume Flow

```text
[New session/conversation or after context reset]
        ↓
/resume
        ↓
memory MCP: search_nodes (pipeline)
        ↓
Pipeline found?
    ├─ NO → "No active pipeline"
    └─ YES ─┬─→ Load latest checkpoint summary
            ├─→ Search relevant learnings
            ├─→ Load spec + checklist
            └─→ Present compact context
                    ↓
                "Ready to continue?"
```

## Output by Phase

### Phase: explore

```text
📋 Resuming Pipeline: Pipeline:add-email-customer

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

### Phase: implement (after context reset)

```text
📋 Resuming Pipeline: Pipeline:add-email-customer

**Phase:** implement
**Started:** 2026-02-05T10:00:00Z
**Objective:** Add email field to Customer

## 💾 From Last Checkpoint (explore)

Discovered Customer entity in Drizzle schema with existing fields.
Backend follows service-repository pattern. Frontend uses React Hook Form.

**Key Files:** schema/customer.ts, CustomerService.cs, CustomerForm.tsx

**Decisions Made:** Add nullable email; Validate format on frontend

**Next Steps:** Implement DB migration; Add backend validation; Update form

## 📚 Relevant Past Learnings

- **Learning:add-phone-customer:1707000000**: Migration needed for nullable fields

## Checklist
- [x] Database: schema updated
- [ ] Backend: endpoints pending
- [ ] Frontend: CRUD pending

## Next Actions (phase: implement)
1. Delegate backend via Task
2. Delegate frontend via Task
3. Run /validate

Ready to continue?
```

### No Pipeline

```text
ℹ️ No active pipeline.

Recently completed pipelines:
- 2026-02-04: solid-user (completed)
- 2026-02-03: add-validation (completed)

Use /feature <name> to start a new pipeline.
Use /bugfix <error> to fix a bug.
```

## Use Cases

### 1. Resume after context reset (NEW v2.6)

```text
[After /approve → user replied "reset"]
[New conversation turn]

User: /resume
Claude: [Loads checkpoint + spec + learnings]
        "Resuming add-email pipeline.

         From checkpoint: Found 12 files, using service pattern...
         Past learning: Watch for migration issues.

         Phase: implement. Ready to delegate.
         Continue?"
```

### 2. Resume after break

```text
User: /resume
Claude: [Loads Pipeline:add-email from memory]
        "Resuming add-email pipeline.
         Phase: implement. Backend pending.
         Continue?"
```

### 3. Resume in new session

```text
[New conversation]
User: "Continue what you were doing"
Claude: [automatic /resume]
        "Active pipeline found: solid-refactor
         Phase: explore. Spec in progress."
```

### 4. Check for active pipeline

```text
User: /resume
Claude: "No active pipeline.
         Use /feature to start."
```

## Auto Loading

Claude can automatically call `/resume` when detecting:
- Messages like "continue", "resume", "where were we"
- New session start when there's an active pipeline

## Arguments

| Argument | Description | Default |
| -------- | ----------- | ------- |
| `[name]` | Specific pipeline name | auto-detect |
| `--full` | Load full checkpoint details | summary only |

## Notes

- **Context auto-refresh** if > 24h old (before resuming)
- Context loaded via **memory MCP**, not files
- Checklist extracted from spec observations
- Phase determines which actions are allowed
- Multiple simultaneous pipelines not supported
- **NEW**: Loads checkpoint summary for compact context
- **NEW**: Shows relevant past learnings (gotchas)
- **NEW**: Ideal for continuing after context reset

## See Also

- [/feature](./feature.md) - Start feature pipeline
- [/approve](./approve.md) - Approve spec (suggests reset)
- [/complete](./complete.md) - Finalize pipeline (saves learnings)
- [/checkpoint](./checkpoint.md) - Manual checkpoint
- [/sync-context](./sync-context.md) - Manually load context
