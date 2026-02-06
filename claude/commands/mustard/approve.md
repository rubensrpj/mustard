# /approve - Approve Spec

> Approves the current spec and enables the implementation phase.

## Usage

```
/approve
/approve [comment]
```

## What It Does

1. **Searches** for active pipeline via memory MCP
2. **Validates** that there is a spec pending approval
3. **Updates** phase to "implement"
4. **Enables** code edits

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

// 3. Update to implement
await mcp__memory__add_observations({
  observations: [{
    entityName: pipeline.name,
    contents: [
      `phase: implement`,
      `approved: ${new Date().toISOString()}`,
      `approved_by: user`
    ]
  }]
});

return `✅ Spec approved!

Pipeline: ${pipeline.name}
Phase: implement (edits enabled)

## ⚠️ CRITICAL: Delegation Required

You MUST now delegate implementation via Task tool. DO NOT implement in this context.

### Analyze Spec Dependencies

1. Read the spec file to identify layers: Backend, Frontend, Database
2. Check if Frontend depends on NEW Backend types (sequential) or existing (parallel)

### Execute Implementation

**If NO dependencies between layers (parallel):**
\`\`\`
Call MULTIPLE Tasks in ONE message:

Task({ subagent_type: "general-purpose", model: "opus", description: "⚙️ Backend {name}", prompt: "..." })
Task({ subagent_type: "general-purpose", model: "opus", description: "🎨 Frontend {name}", prompt: "..." })
\`\`\`

**If Frontend needs NEW Backend types (sequential):**
\`\`\`
1. Task({ description: "⚙️ Backend {name}" }) → WAIT for completion
2. Task({ description: "🎨 Frontend {name}" }) → After backend done
\`\`\`

### After All Tasks Complete
1. Use /validate to verify
2. Use /complete to finalize`;
```

## Flow

```
/feature name
    ↓
EXPLORE (grepai + analysis)
    ↓
SPEC (create spec.md)
    ↓
"Spec ready. Approve?"
    ↓
/approve  ← YOU ARE HERE
    ↓
IMPLEMENT (edits enabled)
    ↓
/validate
    ↓
/complete
```

## Checks

| Condition | Result |
|-----------|--------|
| No active pipeline | ⚠️ Error - use /feature |
| Pipeline in "implement" | ⚠️ Already approved |
| Pipeline in "explore" | ✅ Approve |

## Output

### Success

```
✅ Spec approved!

Pipeline: add-email-partner
Phase: implement

⚠️ CRITICAL: Delegation Required
You MUST delegate via Task tool. DO NOT implement in this context.

Next:
1. Analyze spec for layer dependencies
2. Delegate via Task (parallel or sequential based on dependencies)
3. /validate after Tasks complete
```

### Error - No Pipeline

```
⚠️ No active pipeline found.

Use /feature <name> or /bugfix <error> to start.
```

### Error - Already Approved

```
⚠️ Pipeline already in implement phase.

Use /resume to check current status.
Use /validate to validate implementation.
Use /complete to finalize.
```

## Notes

- Only **one active pipeline** at a time
- After approval, code edits are allowed
- The `enforce-pipeline.js` hook checks phase via memory MCP
- To reject spec and return to exploration, use a negative comment

## See Also

- [/feature](./feature.md) - Start feature pipeline
- [/resume](./resume.md) - Resume pipeline
- [/complete](./complete.md) - Finalize pipeline
