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

## Implementation

1. **Search** for active pipeline in memory MCP
2. **Validate** pipeline is in "explore" phase
3. **Save** exploration checkpoint (auto)
4. **Update** phase to "implement"
5. **Suggest** context reset

→ Memory MCP operations: [pipeline.md#memory-mcp---entity-types](../../core/pipeline.md#memory-mcp---entity-types)

### After Approval

⚠️ **CRITICAL: Delegation Required**

You MUST delegate implementation via Task tool:

| Dependency | Action |
|------------|--------|
| Layers independent | Call multiple Tasks in ONE message (parallel) |
| Frontend needs Backend types | Backend first, then Frontend (sequential) |

After all Tasks complete: `/validate` then `/complete`

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
