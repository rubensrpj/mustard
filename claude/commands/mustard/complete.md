# /complete - Finalizar Pipeline

> Finalizes the active pipeline after successful validation.

## Usage

```
/complete
/complete [final notes]
```

## What It Does

1. **Executes** /validate (build + type-check)
2. **Verifies** validation passed
3. **Records** completion in memory MCP
4. **Cleans** pipeline entities
5. **Updates** entity-registry if needed

## Implementation (Memory MCP)

```javascript
// 1. Find active pipeline
const result = await mcp__memory__search_nodes({
  query: "pipeline phase implement"
});

if (!result.entities.length) {
  return "⚠️ No active pipeline in implement phase.";
}

const pipeline = result.entities[0];
const pipelineName = pipeline.name;

// 2. Run validation
const validateResult = await runValidation();
if (!validateResult.success) {
  return `❌ Validation failed:

${validateResult.errors.join('\n')}

Fix errors and try again.`;
}

// 3. Record completion
await mcp__memory__add_observations({
  observations: [{
    entityName: pipelineName,
    contents: [
      `phase: completed`,
      `finished: ${new Date().toISOString()}`,
      `validation: passed`
    ]
  }]
});

// 4. Find related spec
const specName = pipelineName.replace('Pipeline:', 'Spec:');

// 5. Clean from memory (optional - keep history)
await mcp__memory__delete_entities({
  entityNames: [pipelineName, specName]
});

return `✅ Pipeline completed successfully!

Pipeline: ${pipelineName}
Validation: ✅ Passed
Total time: ${calculateDuration(pipeline)}

Next steps:
- Use /commit to commit changes
- Use /sync-registry if you created new entities`;
```

## Flow

```
/feature name
    ↓
EXPLORE → SPEC → /approve
    ↓
IMPLEMENT
    ↓
/validate
    ↓
/complete  ← YOU ARE HERE
    ↓
COMPLETED (pipeline removed)
```

## Checks

| Condition | Result |
|-----------|--------|
| No active pipeline | ⚠️ Error - nothing to complete |
| Pipeline in "explore" | ⚠️ Error - approve first |
| Validation failed | ❌ Error - fix and retry |
| Validation passed | ✅ Complete |

## Output

### Success

```
✅ Pipeline completed successfully!

Pipeline: add-email-partner
Duration: 2h 15min
Validation: ✅ Passed

Modified files:
- src/models/customer.ts
- src/services/customer-service.ts
- src/api/customer-endpoint.ts
- tests/customer.test.ts

Next steps:
- /commit to commit changes
- /sync-registry if entities created
```

### Error - Validation Failed

```
❌ Validation failed:

Build errors:
- src/services/partner.ts(45): 'email' does not exist

Type errors:
- src/models/partner.ts(12): Property 'email' missing in type

Fix errors and try again.
Pipeline remains active.
```

### Error - No Pipeline

```
⚠️ No active pipeline found.

Use /resume to check status.
Use /feature to start new pipeline.
```

## Validation Executed

The command runs /validate internally, which auto-detects projects:

```
/validate
    ├── Detects all projects by manifest files
    ├── Runs appropriate build command per stack
    └── Reports errors if any
```

See [/validate](./validate.md) for stack detection details.

## Notes

- **Always** runs validation before finalizing
- If validation fails, pipeline remains active
- History can be kept by removing `delete_entities` call
- After completion, start new pipeline with /feature or /bugfix

## See Also

- [/validate](./validate.md) - Validate without completing
- [/approve](./approve.md) - Approve spec
- [/feature](./feature.md) - Start new pipeline
