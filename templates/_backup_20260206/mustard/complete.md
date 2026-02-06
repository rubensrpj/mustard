# /complete - Finalizar Pipeline

> Finalizes the active pipeline after successful validation.
> **v2.6** - Permanent learnings + context cleanup

## Usage

```bash
/complete
/complete [final notes]
/complete --keep-learnings    # Keep learnings entity after completion
```

## What It Does

1. **Executes** /validate (build + type-check)
2. **Verifies** validation passed
3. **Detects** if entity files were modified
4. **Updates** entity-registry automatically if entities changed
5. **Extracts** learnings from pipeline (patterns, decisions, gotchas)
6. **Saves** permanent learnings to memory MCP
7. **Records** completion in memory MCP
8. **Cleans** pipeline entities (keeps learnings)

## Implementation

### Step 1: Find Active Pipeline

```javascript
const result = await mcp__memory__search_nodes({
  query: "pipeline phase implement"
});

if (!result.entities.length) {
  return "⚠️ No active pipeline in implement phase.";
}

const pipeline = result.entities[0];
const pipelineName = pipeline.name;
```

### Step 2: Run Validation

```javascript
const validateResult = await runValidation();
if (!validateResult.success) {
  return `❌ Validation failed:\n${validateResult.errors.join('\n')}\n\nFix errors and try again.`;
}
```

### Step 3: Detect Entity Changes (NEW)

Check if any entity-related files were modified during the pipeline:

```javascript
// Get modified files from git
const modifiedFiles = await Bash({ command: "git diff --name-only HEAD~10" });

// Entity file patterns (adjust per project stack)
const entityPatterns = [
  /models?\//i,           // models/, model/
  /entities?\//i,         // entities/, entity/
  /schemas?\//i,          // schemas/, schema/
  /\.entity\.(ts|cs|py)$/i,  // *.entity.ts, *.entity.cs
  /\.model\.(ts|cs|py)$/i,   // *.model.ts
  /drizzle\/.*schema/i,      // drizzle schemas
  /prisma\/schema\.prisma/i, // prisma schema
];

const entityFilesChanged = modifiedFiles
  .split('\n')
  .some(file => entityPatterns.some(p => p.test(file)));
```

### Step 4: Update Registry if Needed (NEW)

```javascript
if (entityFilesChanged) {
  console.log("🔄 Entity files detected in changes. Updating registry...");

  // Use Task(Explore) to scan entities
  await Task({
    subagent_type: "Explore",
    prompt: `Scan the project for entities and update .claude/entity-registry.json.

    Look for:
    - Database models/schemas (source of truth)
    - Entity relationships (sub-entities, foreign keys)
    - Reference patterns (simple, withTabs, withSubItems, etc.)
    - Enum definitions

    Update the registry following the format in .claude/core/entity-registry-spec.md`
  });

  console.log("✅ Entity registry updated");
}
```

### Step 5: Extract and Save Learnings (NEW)

```javascript
// Gather all checkpoints from this pipeline
const checkpoints = await mcp__memory__search_nodes({
  query: `Checkpoint ${pipelineName}`
});

// Extract learnings from implementation
const learnings = {
  patterns: [],      // Patterns discovered/used
  decisions: [],     // Architectural decisions made
  gotchas: [],       // Problems encountered and solutions
  files_touched: [], // Key files modified
  duration: calculateDuration(pipeline)
};

// Analyze checkpoints for insights
for (const checkpoint of checkpoints.entities) {
  // Extract patterns, decisions, gotchas from each phase
  learnings.patterns.push(...extractPatterns(checkpoint));
  learnings.decisions.push(...extractDecisions(checkpoint));
  learnings.gotchas.push(...extractGotchas(checkpoint));
}

// Create permanent learning entity
const learningEntity = `Learning:${pipelineName.replace('Pipeline:', '')}:${Date.now()}`;

await mcp__memory__create_entities({
  entities: [{
    name: learningEntity,
    entityType: "Learning",
    observations: [
      `pipeline: ${pipelineName}`,
      `completed: ${new Date().toISOString()}`,
      `duration: ${learnings.duration}`,
      `objective: ${extractObservation(pipeline, 'objective')}`,
      `patterns: ${learnings.patterns.join('; ')}`,
      `decisions: ${learnings.decisions.join('; ')}`,
      `gotchas: ${learnings.gotchas.join('; ')}`,
      `files: ${learnings.files_touched.join(', ')}`,
      `registry_updated: ${entityFilesChanged}`
    ]
  }]
});

// Link to project context for future reference
await mcp__memory__create_relations({
  relations: [{
    from: "ProjectContext:current",
    to: learningEntity,
    relationType: "has_learning"
  }]
});
```

### Step 6: Record Completion

```javascript
await mcp__memory__add_observations({
  observations: [{
    entityName: pipelineName,
    contents: [
      `phase: completed`,
      `finished: ${new Date().toISOString()}`,
      `validation: passed`,
      `registry_updated: ${entityFilesChanged}`,
      `learning_saved: ${learningEntity}`
    ]
  }]
});
```

### Step 7: Clean Pipeline (Keep Learnings)

```javascript
const specName = pipelineName.replace('Pipeline:', 'Spec:');

// Delete pipeline and spec entities
await mcp__memory__delete_entities({
  entityNames: [pipelineName, specName]
});

// Delete checkpoints (learnings already extracted)
const checkpointNames = checkpoints.entities.map(c => c.name);
await mcp__memory__delete_entities({
  entityNames: checkpointNames
});

// Learning entity is KEPT for future reference
```

## Flow

```text
/feature name
    ↓
EXPLORE → SPEC → /approve
    ↓
IMPLEMENT
    ↓
/validate
    ↓
/complete  ← YOU ARE HERE
    │
    ├── Extract learnings from checkpoints
    ├── Save Learning entity (permanent)
    ├── Clean pipeline + checkpoints
    │
    ↓
COMPLETED (learnings preserved)
```

## Checks

| Condition | Result |
| --------- | ------ |
| No active pipeline | ⚠️ Error - nothing to complete |
| Pipeline in "explore" | ⚠️ Error - approve first |
| Validation failed | ❌ Error - fix and retry |
| Validation passed | ✅ Complete + save learnings |

## Output

### Success (with entity changes)

```text
✅ Pipeline completed successfully!

Pipeline: add-email-partner
Duration: 2h 15min
Validation: ✅ Passed

Modified files:
- src/models/customer.ts
- src/services/customer-service.ts
- src/api/customer-endpoint.ts
- tests/customer.test.ts

🔄 Entity registry updated (detected changes in models/)

💾 Learnings Saved
- Patterns: service-repository pattern, form validation
- Decisions: Used existing Customer model, added email validation
- Gotchas: Email uniqueness constraint needed migration

Learning: Learning:add-email-partner:1707235200

Next steps:
- /commit to commit changes
```

### Success (no entity changes)

```text
✅ Pipeline completed successfully!

Pipeline: fix-login-bug
Duration: 45min
Validation: ✅ Passed

Modified files:
- src/services/auth-service.ts
- tests/auth.test.ts

💾 Learnings Saved
- Patterns: error handling in auth flow
- Decisions: Added retry logic for token refresh
- Gotchas: Race condition on concurrent logins

Learning: Learning:fix-login-bug:1707238800

Next steps:
- /commit to commit changes
```

### Error - Validation Failed

```text
❌ Validation failed:

Build errors:
- src/services/partner.ts(45): 'email' does not exist

Type errors:
- src/models/partner.ts(12): Property 'email' missing in type

Fix errors and try again.
Pipeline remains active.
```

### Error - No Pipeline

```text
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

## Entity Detection Patterns

The following file patterns trigger automatic registry update:

| Pattern | Example |
| ------- | ------- |
| `models/`, `model/` | `src/models/customer.ts` |
| `entities/`, `entity/` | `domain/entities/Order.cs` |
| `schemas/`, `schema/` | `db/schemas/user.ts` |
| `*.entity.ts/cs/py` | `Customer.entity.ts` |
| `*.model.ts/cs/py` | `Order.model.cs` |
| `drizzle/*schema*` | `drizzle/schema.ts` |
| `prisma/schema.prisma` | Prisma schema |

## Learnings Structure

Each completed pipeline saves a Learning entity:

| Field | Description | Example |
| ----- | ----------- | ------- |
| `pipeline` | Original pipeline name | `Pipeline:add-login` |
| `objective` | What was implemented | "Add email login feature" |
| `duration` | Time from start to complete | "2h 15min" |
| `patterns` | Patterns discovered/used | "service-repository, form validation" |
| `decisions` | Key decisions made | "Use JWT over sessions" |
| `gotchas` | Problems and solutions | "Race condition on refresh" |
| `files` | Key files modified | "auth.ts, login.tsx" |

### Using Learnings

Future pipelines can query past learnings:

```javascript
// Search for relevant learnings
const learnings = await mcp__memory__search_nodes({
  query: "Learning authentication login"
});

// Get details
const details = await mcp__memory__open_nodes({
  names: learnings.entities.map(e => e.name)
});

// Apply gotchas to avoid past mistakes
```

## Notes

- **Always** runs validation before finalizing
- **Automatically** updates entity-registry if entity files changed
- **NEW**: Extracts and saves learnings permanently
- **NEW**: Learnings linked to ProjectContext for future reference
- If validation fails, pipeline remains active
- Checkpoints are cleaned after learnings extracted
- Learning entities persist indefinitely (manual cleanup if needed)
- After completion, start new pipeline with /feature or /bugfix

## See Also

- [/validate](./validate.md) - Validate without completing
- [/approve](./approve.md) - Approve spec
- [/feature](./feature.md) - Start new pipeline
- [/checkpoint](./checkpoint.md) - Manual checkpoints during pipeline
