# /bugfix - Bugfix Pipeline

> Single entry point for diagnosing and fixing bugs.
> **v2.6** - Prerequisites enforced via hooks + Context Reset support.

## Usage

```
/bugfix <error>
/bugfix "TenantId is null when saving contract"
/bugfix "TypeError: Cannot read property 'id' of undefined"
```

## Prerequisites (Enforced by Hooks)

Before this command runs, hooks validate:

1. **Entity Registry** - `.claude/entity-registry.json` must exist and be valid (v3.x)
2. **Compiled Contexts** - All agent contexts must be compiled and up-to-date

If prerequisites fail, you'll be prompted to run:

- `/sync-registry` - Update entity registry
- `/compile-context` - Compile agent contexts

## What It Does

1. **Diagnoses** root cause via Task(general-purpose) + bugfix prompt
2. **Proposes** minimal fix
3. **Awaits** approval
4. **Implements** fix
5. **Validates** that bug is fixed
6. **Suggests** commit

## Pipeline (Native Types)

```
/bugfix <error>
     │
     ▼
┌────────────────────────────────┐
│  Task(general-purpose)         │
│  + bugfix.md prompt            │
│  model: opus                   │
└──────────────┬─────────────────┘
               │
               ▼
         DIAGNOSIS
               │
               ▼
         SPEC/PROPOSAL
               │
               ▼
         APPROVAL
               │
               ▼
         FIX
               │
               ▼
         VALIDATION
               │
               ▼
         SUGGESTED COMMIT
```

## Implementation

### Command Execution

```javascript
// The /bugfix command executes:
Task({
  subagent_type: "general-purpose",
  model: "opus",
  description: `Bugfix: ${error}`,
  prompt: `
# You are the BUGFIX SPECIALIST

## Identity
You diagnose, fix, and validate bugs. Minimal and focused fixes.

## Work Phases

### PHASE 1: DIAGNOSIS
1. Collect information (error, stack trace, context)
2. Search code with grepai_search and grepai_trace_*
3. Identify root cause
4. Document diagnosis

### PHASE 2: FIX
1. Propose MINIMAL fix
2. Await user approval
3. Implement fix
4. Test build

### PHASE 3: VALIDATION
1. Verify error no longer occurs
2. Check for regression
3. Suggest commit message

## Tools
- grepai_search({ query: "..." })
- grepai_trace_callers({ symbol: "..." })
- grepai_trace_callees({ symbol: "..." })

## Rules
- Do NOT refactor beyond the bug
- Do NOT add features
- MINIMAL and FOCUSED fix
- ALWAYS validate build

## REPORTED ERROR
${error}

## ENFORCEMENT
- L0: Delegate if needed (e.g., Task(Explore) for analysis)
- L5: Build must pass after fix
  `
})
```

## Arguments

| Argument | Description | Example |
|----------|-------------|---------|
| `<error>` | Error description or stack trace | `"NullRef in SaveContract"` |

## Examples

```bash
# Simple error
/bugfix "Error saving contract"

# With stack trace
/bugfix "TypeError at ContractService.cs:142"

# Unexpected behavior
/bugfix "Contract list not filtering by tenant"
```

## Output

### Diagnosis Phase

```
Task(general-purpose): Diagnosing...

Diagnosis:
- Error: TenantId is null
- Cause: Middleware not setting TenantId
- Location: ContractService.cs:142
- Related: TenantMiddleware.cs

Proposed fix:
- Check if TenantMiddleware is registered
- Add validation in ContractService

Approve fix? [Y/N]
```

### After Approval

```
Task(general-purpose): Applying fix...

✅ Fix applied:
- ContractService.cs:142 - Added validation

✅ Validation:
- Build passes
- Error no longer occurs

Suggested commit:
fix: validate TenantId before saving contract

Added null check for TenantId in ContractService.Save()
to prevent NullReferenceException when tenant context
is not properly set.

Execute commit? [Y/N]
```

## Bug Spec

```markdown
# Bugfix: TenantId is null when saving contract

## Reported Error
```
System.NullReferenceException: TenantId is null
   at ContractService.Save() line 142
```

## Root Cause
TenantMiddleware not executing before endpoint.

## Affected Files
| File | Line | Problem |
|------|------|---------|
| ContractService.cs | 142 | Not validating TenantId |

## Proposed Fix
Add TenantId validation before saving.

## Validation
- [ ] Error no longer occurs
- [ ] Other endpoints work
- [ ] Build passes
```

## Notes

- **Prerequisites enforced** via hooks (registry + contexts)
- **Minimal and focused** fix
- Does **not** refactor beyond necessary
- **Always** validates regression
- **Always** suggests commit message
- **Uses only native types**: general-purpose, Explore (if needed)
- Compiled context available in `prompts/{agent}.context.md`

## See Also

- [/feature](./feature.md) - Feature pipeline
- [/sync-context](./sync-context.md) - Manually load context
- [context/README.md](../context/README.md) - How to create context files
