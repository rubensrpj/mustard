# /approve - Approve Spec

> Approves the current spec and enables the implementation phase.
> **v2.6** - Context Reset integration

## Usage

```
/approve
/approve [comment]
/approve --no-reset    # Skip context reset suggestion
```

## What It Does

1. **Searches** for active pipeline via memory MCP
2. **Validates** that there is a spec pending approval
3. **Saves** exploration checkpoint to memory (auto)
4. **Updates** phase to "implement"
5. **Suggests** context reset for clean implementation
6. **Enables** code edits

## Implementation (Memory MCP)

```javascript
// 1. Search for active pipeline
const result = await mcp__memory__search_nodes({
  query: "pipeline phase explore"
});

if (!result.entities.length) {
  return "⚠️ No active pipeline found. Use /feature or /bugfix first.";
}

const pipeline = result.entities[0];

// 2. Check phase
if (!pipeline.observations.includes("phase: explore")) {
  return "⚠️ Pipeline is not in explore phase. Use /resume to check status.";
}

// 3. AUTO: Save exploration checkpoint
const checkpointEntity = `Checkpoint:${pipeline.name}:explore:${Date.now()}`;

await mcp__memory__create_entities({
  entities: [{
    name: checkpointEntity,
    entityType: "Checkpoint",
    observations: [
      `pipeline: ${pipeline.name}`,
      `phase: explore`,
      `created: ${new Date().toISOString()}`,
      `type: auto-approve`,
      // Extract key insights from exploration:
      `discovered_files: ${explorationFindings.files.join(', ')}`,
      `patterns: ${explorationFindings.patterns.join('; ')}`,
      `dependencies: ${explorationFindings.dependencies.join('; ')}`
    ]
  }]
});

await mcp__memory__create_relations({
  relations: [{
    from: pipeline.name,
    to: checkpointEntity,
    relationType: "has_checkpoint"
  }]
});

// 4. Update to implement
await mcp__memory__add_observations({
  observations: [{
    entityName: pipeline.name,
    contents: [
      `phase: implement`,
      `approved: ${new Date().toISOString()}`,
      `approved_by: user`,
      `exploration_checkpoint: ${checkpointEntity}`
    ]
  }]
});
```

### Return Message

```javascript
return `✅ Spec approved!

Pipeline: ${pipeline.name}
Phase: implement (edits enabled)

## 💾 Exploration Checkpoint Saved

Your exploration findings have been saved to memory:
- Discovered files: ${explorationFindings.files.length}
- Patterns identified: ${explorationFindings.patterns.length}
- Dependencies mapped: ${explorationFindings.dependencies.length}

Checkpoint: ${checkpointEntity}

---

## 🧹 CONTEXT RESET RECOMMENDED

The exploration phase accumulated context in this conversation.
For optimal implementation performance, consider resetting:

**Option 1: Reset now (recommended)**
Reply with: "reset" or use \`/checkpoint --reset\`

**Option 2: Continue in current context**
Reply with: "continue"

---

## ⚠️ CRITICAL: Delegation Required

You MUST delegate implementation via Task tool. DO NOT implement in this context.

### Analyze Spec Dependencies

1. Read the spec file to identify layers: Backend, Frontend, Database
2. Check if Frontend depends on NEW Backend types (sequential) or existing (parallel)

### Execute Implementation

**If NO dependencies between layers (parallel):**
Call MULTIPLE Tasks in ONE message:
- Task({ description: "⚙️ Backend {name}" })
- Task({ description: "🎨 Frontend {name}" })

**If Frontend needs NEW Backend types (sequential):**
1. Task({ description: "⚙️ Backend {name}" }) → WAIT for completion
2. Task({ description: "🎨 Frontend {name}" }) → After backend done

### After All Tasks Complete
1. Use /validate to verify
2. Use /complete to finalize`;
```

## Flow

```text
/feature name
    ↓
EXPLORE (grepai + analysis)
    ↓
SPEC (create spec.md)
    ↓
"Spec ready. Approve?"
    ↓
/approve  ← YOU ARE HERE
    │
    ├── AUTO: Save exploration checkpoint
    ├── SUGGEST: Context reset
    │
    ↓
IMPLEMENT (clean context)
    ↓
/validate
    ↓
/complete
```

## Checks

| Condition | Result |
| --------- | ------ |
| No active pipeline | ⚠️ Error - use /feature |
| Pipeline in "implement" | ⚠️ Already approved |
| Pipeline in "explore" | ✅ Approve + checkpoint |

## Output

### Success

```text
✅ Spec approved!

Pipeline: add-email-partner
Phase: implement (edits enabled)

💾 Exploration Checkpoint Saved
- Discovered files: 12
- Patterns identified: 3
- Dependencies mapped: 5

🧹 CONTEXT RESET RECOMMENDED
Reply "reset" to clear context, or "continue" to proceed.

⚠️ CRITICAL: Delegation Required
You MUST delegate via Task tool. DO NOT implement in this context.
```

### Error - No Pipeline

```text
⚠️ No active pipeline found.

Use /feature <name> or /bugfix <error> to start.
```

### Error - Already Approved

```text
⚠️ Pipeline already in implement phase.

Use /resume to check current status.
Use /validate to validate implementation.
Use /complete to finalize.
```

## Context Reset Behavior

When user replies "reset" after approval:

1. **Saves** final exploration summary to checkpoint
2. **Clears** conversation context (new turn starts fresh)
3. **User types** `/resume` to continue with clean context
4. **Resume loads** checkpoint + spec + checklist

### Why Reset?

| Benefit | Description |
| ------- | ----------- |
| Clean context | Implementation starts without exploration noise |
| Better focus | Agent sees only spec + patterns, not exploration history |
| Token efficiency | More room for implementation details |
| Reduced confusion | No old exploration paths to confuse implementation |

## Notes

- Only **one active pipeline** at a time
- After approval, code edits are allowed
- The `enforce-pipeline.js` hook checks phase via memory MCP
- To reject spec and return to exploration, use a negative comment
- **NEW**: Exploration checkpoint saved automatically
- **NEW**: Context reset recommended for clean implementation

## See Also

- [/feature](./feature.md) - Start feature pipeline
- [/resume](./resume.md) - Resume pipeline (loads from checkpoint)
- [/complete](./complete.md) - Finalize pipeline
- [/checkpoint](./checkpoint.md) - Manual checkpoint (optional)
