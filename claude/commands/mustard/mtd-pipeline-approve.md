# /mtd-pipeline-approve - Approve Spec

> Approves the current spec and enables the implementation phase.

## Usage

```
/mtd-pipeline-approve
/mtd-pipeline-approve [comment]
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
  return "⚠️ No active pipeline found. Use /mtd-pipeline-feature or /mtd-pipeline-bugfix first.";
}

const pipeline = result.entities[0];

// 2. Check phase
if (!pipeline.observations.includes("phase: explore")) {
  return "⚠️ Pipeline is not in explore phase. Use /mtd-pipeline-resume to check status.";
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

Next steps:
1. Implement according to spec
2. Use /mtd-validate-build when done
3. Use /mtd-pipeline-complete to finalize`;
```

## Flow

```
/mtd-pipeline-feature name
    ↓
EXPLORE (grepai + analysis)
    ↓
SPEC (create spec.md)
    ↓
"Spec ready. Approve?"
    ↓
/mtd-pipeline-approve  ← YOU ARE HERE
    ↓
IMPLEMENT (edits enabled)
    ↓
/mtd-validate-build
    ↓
/mtd-pipeline-complete
```

## Checks

| Condition | Result |
|-----------|--------|
| No active pipeline | ⚠️ Error - use /mtd-pipeline-feature |
| Pipeline in "implement" | ⚠️ Already approved |
| Pipeline in "explore" | ✅ Approve |

## Output

### Success

```
✅ Spec approved!

Pipeline: add-email-partner
Phase: implement

Code edits now allowed.
Next steps:
1. Implement according to spec
2. Use /mtd-validate-build when done
```

### Error - No Pipeline

```
⚠️ No active pipeline found.

Use /mtd-pipeline-feature <name> or /mtd-pipeline-bugfix <error> to start.
```

### Error - Already Approved

```
⚠️ Pipeline already in implement phase.

Use /mtd-pipeline-resume to check current status.
Use /mtd-validate-build to validate implementation.
Use /mtd-pipeline-complete to finalize.
```

## Notes

- Only **one active pipeline** at a time
- After approval, code edits are allowed
- The `enforce-pipeline.js` hook checks phase via memory MCP
- To reject spec and return to exploration, use a negative comment

## See Also

- [/mtd-pipeline-feature](./mtd-pipeline-feature.md) - Start feature pipeline
- [/mtd-pipeline-resume](./mtd-pipeline-resume.md) - Resume pipeline
- [/mtd-pipeline-complete](./mtd-pipeline-complete.md) - Finalize pipeline
