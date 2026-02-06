# /checkpoint - Save Context Checkpoint

> Saves current phase insights to memory and optionally clears conversation context.
> **v2.6** - Context Reset + Memory Persistence

## Usage

```
/checkpoint
/checkpoint --reset
/checkpoint --summary "custom summary"
```

## What It Does

1. **Extracts** key insights from current conversation
2. **Saves** phase summary to memory MCP
3. **Clears** conversation context (if --reset)
4. **Preserves** everything needed to continue

## Why Use This

| Problem | Solution |
|---------|----------|
| Context window filling up | Checkpoint saves insights, reset clears window |
| Long exploration phase | Save findings before approval |
| Implementation taking multiple sessions | Checkpoint progress incrementally |
| Want to preserve learnings | Insights saved to memory MCP permanently |

## Implementation

### Step 1: Extract Current Phase Insights

```javascript
// 1. Find active pipeline
const pipeline = await mcp__memory__search_nodes({
  query: "pipeline phase"
});

if (!pipeline.entities.length) {
  return "No active pipeline. Use /feature or /bugfix first.";
}

const pipelineName = pipeline.entities[0].name;
const phase = extractObservation(pipeline.entities[0], 'phase');
```

### Step 2: Generate Phase Summary

```javascript
// 2. Create summary based on current phase
const summary = await generatePhaseSummary(phase, conversation);

// Summary structure by phase:
// - explore: discovered files, patterns, dependencies
// - implement: completed tasks, modified files, decisions made
// - review: issues found, fixes applied

const checkpointEntity = `Checkpoint:${pipelineName}:${phase}:${Date.now()}`;

await mcp__memory__create_entities({
  entities: [{
    name: checkpointEntity,
    entityType: "Checkpoint",
    observations: [
      `pipeline: ${pipelineName}`,
      `phase: ${phase}`,
      `created: ${new Date().toISOString()}`,
      `summary: ${summary.brief}`,
      `key_files: ${summary.files.join(', ')}`,
      `decisions: ${summary.decisions.join('; ')}`,
      `next_steps: ${summary.nextSteps.join('; ')}`
    ]
  }]
});

// 3. Link checkpoint to pipeline
await mcp__memory__create_relations({
  relations: [{
    from: pipelineName,
    to: checkpointEntity,
    relationType: "has_checkpoint"
  }]
});
```

### Step 3: Context Reset (if requested)

```javascript
if (args.includes('--reset')) {
  return `## Checkpoint Saved

**Pipeline:** ${pipelineName}
**Phase:** ${phase}
**Checkpoint:** ${checkpointEntity}

### Summary Saved
${summary.brief}

### Key Files
${summary.files.map(f => `- ${f}`).join('\n')}

### Decisions Made
${summary.decisions.map(d => `- ${d}`).join('\n')}

### Next Steps
${summary.nextSteps.map(s => `- ${s}`).join('\n')}

---

## CONTEXT RESET REQUESTED

The conversation context will now be cleared.
Use \`/resume\` in a new message to continue from this checkpoint.

All progress has been saved to memory MCP.`;
}
```

## Checkpoint Structure

Each checkpoint saves:

| Field | Description | Example |
|-------|-------------|---------|
| `pipeline` | Active pipeline name | `Pipeline:add-login` |
| `phase` | Current phase | `explore`, `implement`, `review` |
| `summary` | Brief description of progress | "Discovered auth flow..." |
| `key_files` | Important files identified/modified | `auth.ts, login.tsx` |
| `decisions` | Architectural decisions made | "Use JWT over sessions" |
| `next_steps` | What remains to be done | "Implement refresh token" |

## Memory Structure

```
Pipeline:add-login
├── has_checkpoint → Checkpoint:Pipeline:add-login:explore:1707235200
├── has_checkpoint → Checkpoint:Pipeline:add-login:implement:1707238800
└── has_spec → Spec:add-login
```

## Output Examples

### Success (no reset)

```
## Checkpoint Saved

**Pipeline:** Pipeline:add-login
**Phase:** explore

### Summary
Discovered authentication flow using NextAuth. Main files:
- src/lib/auth.ts (NextAuth config)
- src/app/api/auth/[...nextauth]/route.ts (API route)

### Key Files
- src/lib/auth.ts
- src/components/LoginForm.tsx
- src/middleware.ts

### Decisions
- Use existing NextAuth setup
- Add email/password provider

### Next Steps
- Create spec for email login feature
- Define database schema changes

Checkpoint saved. Conversation context preserved.
Use `/checkpoint --reset` to clear context.
```

### Success (with reset)

```
## Checkpoint Saved

[same content as above]

---

## CONTEXT RESET REQUESTED

The conversation context will now be cleared.
Use `/resume` in a new message to continue from this checkpoint.
```

## When to Use

### Recommended Checkpoints

| Moment | Why |
|--------|-----|
| After exploration, before spec | Save discoveries before writing spec |
| After approval, before implement | Clear exploration context |
| After each layer implementation | Save progress incrementally |
| After review findings | Preserve review insights |
| Before long break | Ensure nothing is lost |

### Automatic Suggestions

The system suggests `/checkpoint --reset` when:
- Context exceeds 80% of window
- Phase transition (explore → implement)
- 30+ minutes since last checkpoint

## Integration with Pipeline

```
/feature add-login
    │
    ▼
EXPLORE (grepai + analysis)
    │
    ├─── /checkpoint (optional - save exploration insights)
    │
    ▼
SPEC created
    │
    ▼
/approve
    │
    ├─── AUTO: /checkpoint --reset (recommended)
    │
    ▼
IMPLEMENT (clean context)
    │
    ├─── /checkpoint (after each layer)
    │
    ▼
/complete
    │
    └─── AUTO: save learnings permanently
```

## Resume from Checkpoint

When resuming after reset:

```javascript
// /resume automatically loads:
// 1. Pipeline state
// 2. Latest checkpoint summary
// 3. Spec content
// 4. Checklist progress

// User sees:
`📋 Resuming Pipeline: add-login

**Phase:** implement
**Last Checkpoint:** 2 hours ago

### Context from Checkpoint
${checkpointSummary}

### Key Files to Reference
${keyFiles}

### Remaining Tasks
${nextSteps}

Ready to continue?`
```

## Arguments

| Argument | Description | Default |
|----------|-------------|---------|
| `--reset` | Clear context after saving | false |
| `--summary "text"` | Custom summary text | auto-generated |
| `--include-code` | Include code snippets in checkpoint | false |

## Notes

- Checkpoints are **cumulative** - multiple per phase allowed
- `--reset` triggers conversation clear (new turn starts fresh)
- Memory MCP persists across sessions
- Checkpoints can be reviewed with `/status`
- Old checkpoints auto-cleaned after pipeline completes (configurable)

## See Also

- [/approve](./approve.md) - Automatically suggests checkpoint
- [/resume](./resume.md) - Loads from checkpoint
- [/complete](./complete.md) - Saves permanent learnings
- [/status](./status.md) - View checkpoints
