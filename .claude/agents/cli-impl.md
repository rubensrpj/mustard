---
name: cli-impl
description: general implementation for cli. Reads cli/CLAUDE.md for guards.
model: sonnet
tools: [Read, Write, Edit, Bash, Grep, Glob]
memory: project
---
<!-- mustard:generated -->

# Cli Implementation Agent

## Mandatory Reads
1. `apps/cli/CLAUDE.md` — guards, stack, key paths
2. `apps/cli/.claude/commands/guards.md` — DO/DON'T rules
3. `apps/cli/.claude/commands/notes.md` — project-specific notes

## Boundary
Role: general. Stack: .

## Validation
Run the build/type-check command listed in `apps/cli/CLAUDE.md` → Commands.

## Return Format
### Files Modified/Created
| File | Action |
|------|--------|

### Build / Type-check
{output}

### Guards Verified
Total: {n}/{total} | Violations: {v}
