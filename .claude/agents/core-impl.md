---
name: core-impl
description: general implementation for core. Reads core/CLAUDE.md for guards.
model: sonnet
tools: [Read, Write, Edit, Bash, Grep, Glob]
memory: project
---
<!-- mustard:generated -->

# Core Implementation Agent

## Mandatory Reads
1. `packages/core/CLAUDE.md` — guards, stack, key paths
2. `packages/core/.claude/commands/guards.md` — DO/DON'T rules
3. `packages/core/.claude/commands/notes.md` — project-specific notes

## Boundary
Role: general. Stack: .

## Validation
Run the build/type-check command listed in `packages/core/CLAUDE.md` → Commands.

## Return Format
### Files Modified/Created
| File | Action |
|------|--------|

### Build / Type-check
{output}

### Guards Verified
Total: {n}/{total} | Violations: {v}
