# /bugfix-team - Bugfix Pipeline (Agent Teams)

> Uses Agent Teams for parallel bug investigation with competing hypotheses.
> **Experimental** - Requires Agent Teams to be enabled in settings.

## Usage

```bash
/bugfix-team <error>
/bugfix-team "NullReferenceException in SaveContract"
/bugfix-team "Invoice total calculation wrong"
```

## What It Does

1. **Compiles contexts** for all agents (mandatory first step)
2. **Verifies** Agent Teams is enabled
3. **Creates team** with you as Team Lead
4. **Spawns investigator teammates** with different hypotheses
5. **Coordinates investigation** via shared findings
6. **Identifies root cause** from competing analyses
7. **Spawns fix teammate** to implement solution
8. **Spawns review teammate** to validate fix
9. **Cleans up team** when complete

## Prerequisites

Enable Agent Teams in `.claude/settings.json`:

```json
{
  "env": {
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1"
  }
}
```

## Pipeline (Agent Teams)

```text
/bugfix-team <error>
     │
     ▼
┌────────────────────────────────┐
│  YOU as TEAM LEAD              │
│  + team-lead.md prompt         │
│  + delegate mode (Shift+Tab)   │
└──────────────┬─────────────────┘
               │
     ┌─────────┼─────────┐
     ▼         ▼         ▼
 Hypothesis  Hypothesis  Hypothesis
     A          B           C
 (teammate) (teammate)  (teammate)
     │         │         │
     └────┬────┴────┬────┘
          ▼         ▼
      FINDINGS   ROOT CAUSE
          │
          ▼
    Fix Teammate
          │
          ▼
   Review Teammate
          │
          ▼
     TEAM CLEANUP
```

## Implementation

### Phase 0: Compile Contexts (MANDATORY FIRST STEP)

**BEFORE doing anything else, you MUST compile all agent contexts:**

#### Step 0.1: Get current commit hash

```bash
git rev-parse --short HEAD
```

Save the result as `currentHash`.

#### Step 0.2: For each agent, check and compile

For each agent in: `bugfix`, `review`, `team-lead`:

1. Use Glob to check if `.claude/prompts/{agent}.context.md` exists
2. If exists, Read the file and check if `compiled-from-commit: {hash}` matches `currentHash`
3. If missing OR hash differs:
   - Use Glob to find all `.md` files in `.claude/context/shared/` (exclude README)
   - Use Glob to find all `.md` files in `.claude/context/{agent}/` (exclude README)
   - Read each file's content
   - Synthesize into a single compiled context (remove duplicates, consolidate, optimize)
   - Write to `.claude/prompts/{agent}.context.md` with format:

```markdown
<!-- compiled-from-commit: {currentHash} -->
<!-- sources: {list of source files} -->
<!-- compiled-at: {ISO timestamp} -->

# {Agent} Context

{synthesized content}
```

#### Step 0.3: Report compilation status

```text
✅ Context compiled for all agents (commit: {hash})
```

> ⚠️ **DO NOT SKIP THIS STEP.** All teammates depend on compiled contexts.

### Phase 1: Verify Agent Teams

Check that Agent Teams is enabled before proceeding.

### Phase 2: Analyze Error

Collect initial information:

- Error message and stack trace
- Affected files
- Recent changes
- Reproduction steps

### Phase 3: Formulate Hypotheses

Based on error analysis, create 2-3 competing hypotheses:

```text
## Hypotheses for: {error}

### Hypothesis A: Data issue
- Possibility: Null/missing data in database
- Investigation: Check database records, query logic

### Hypothesis B: Logic error
- Possibility: Bug in business logic
- Investigation: Trace code path, check conditions

### Hypothesis C: Integration issue
- Possibility: Type mismatch between layers
- Investigation: Check DTOs, API contracts
```

### Phase 4: Spawn Investigation Team

Spawn teammates for parallel investigation:

```text
Spawn an Investigator teammate (Hypothesis A) with prompt:
"You are investigating a potential DATA ISSUE.
Error: {error}
Your hypothesis: Null/missing data in database.
Search for: database queries, null checks, seed data.
Report findings to Team Lead. Prove or disprove your hypothesis."

Spawn an Investigator teammate (Hypothesis B) with prompt:
"You are investigating a potential LOGIC ERROR.
Error: {error}
Your hypothesis: Bug in business logic.
Search for: conditionals, calculations, edge cases.
Report findings to Team Lead. Prove or disprove your hypothesis."

Spawn an Investigator teammate (Hypothesis C) with prompt:
"You are investigating a potential INTEGRATION ISSUE.
Error: {error}
Your hypothesis: Type mismatch between layers.
Search for: DTOs, mappers, API contracts.
Report findings to Team Lead. Prove or disprove your hypothesis."
```

### Phase 5: Collect Findings

As teammates report:

- Document findings from each hypothesis
- Identify which hypothesis is correct
- Synthesize root cause

### Phase 6: Create Fix Spec

Based on findings, create fix specification:

```text
## Fix Spec: {error}

### Root Cause
{Description from winning hypothesis}

### Files to Modify
- {file}: {change needed}

### Fix Approach
{Step by step fix}
```

### Phase 7: Spawn Fix Teammate

```text
Spawn a Bugfix Specialist teammate with prompt:
"You are the Bugfix Specialist.
Root cause: {root cause}
Fix spec: {fix spec}
Implement minimal fix. Message Team Lead when done."
```

### Phase 8: Review

```text
Spawn a Review Specialist teammate with prompt:
"You are the Review Specialist.
Review the fix for: {error}
Verify: fix addresses root cause, no regression, build passes."
```

### Phase 9: Cleanup

After review is complete:

```text
Ask all teammates to shut down.
Clean up the team.
Report final status.
```

## Comparison: /bugfix vs /bugfix-team

| Aspect | /bugfix (Task) | /bugfix-team (Teams) |
|--------|----------------|----------------------|
| Investigation | Sequential | Parallel hypotheses |
| Speed | Slower | Faster for complex bugs |
| Thoroughness | Single path | Multiple paths explored |
| Token Cost | Lower | Higher |
| Best For | Simple bugs | Complex, multi-cause bugs |

## Arguments

| Argument | Description | Example |
|----------|-------------|---------|
| `<error>` | Error description | `"NullRef in Save"` |

## Examples

```bash
# Exception
/bugfix-team "NullReferenceException in SaveContract"

# Wrong behavior
/bugfix-team "Invoice total shows negative value"

# Integration issue
/bugfix-team "Frontend shows old data after update"
```

## Output

### During Investigation

```text
Team Lead: Formulating hypotheses for NullReferenceException...
Team Lead: Spawning Investigator A (Data issue)...
Team Lead: Spawning Investigator B (Logic error)...
Team Lead: Spawning Investigator C (Integration issue)...

Investigator A: Checking database records...
Investigator B: Tracing code path...
Investigator C: Checking DTOs...

Investigator B: FOUND IT! Contract.Customer is null when saving.
Investigator A: No database issues found. Hypothesis disproven.
Investigator C: No type mismatches. Hypothesis disproven.

Team Lead: Root cause identified. Spawning Fix Specialist...
```

### After Completion

```text
Team Lead: Bugfix complete.

## Bugfix Complete: NullReferenceException in SaveContract

### Root Cause
Contract.Customer was not loaded before save operation.
Missing Include() in GetContract query.

### Hypothesis Results
| Hypothesis | Result |
|------------|--------|
| A: Data issue | Disproven |
| B: Logic error | CONFIRMED |
| C: Integration | Disproven |

### Fix Applied
File: ContractService.cs:42
Added: .Include(c => c.Customer) to query

### Review: APPROVED

Team cleanup complete.
```

## Notes

- **Competing hypotheses**: Multiple teammates investigate in parallel
- **Faster resolution**: For complex bugs with unclear cause
- **Higher token cost**: Multiple investigation contexts
- **Best for**: Bugs where root cause is not obvious
- **Avoid for**: Simple, obvious bugs (use `/bugfix` instead)

## Limitations

- Same limitations as `/feature-team`
- Token cost scales with number of hypotheses
- Best limited to 2-3 hypotheses

## See Also

- [/bugfix](./bugfix.md) - Task-based pipeline (lower cost)
- [/feature-team](./feature-team.md) - Team-based feature
- [team-lead.md](../../prompts/team-lead.md) - Team Lead prompt
- [bugfix.md](../../prompts/bugfix.md) - Bugfix Specialist prompt
