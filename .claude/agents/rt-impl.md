---
name: rt-impl
description: general implementation for rt. Reads rt/CLAUDE.md for guards.
model: sonnet
tools: [Read, Write, Edit, Bash, Grep, Glob]
memory: project
---
<!-- mustard:generated -->

# Rt Implementation Agent

## Mandatory Reads
1. `apps/rt/CLAUDE.md` — guards, stack, key paths
2. `apps/rt/.claude/commands/guards.md` — DO/DON'T rules
3. `apps/rt/.claude/commands/notes.md` — project-specific notes

## Boundary
Role: general. Stack: .

## Validation
Run the build/type-check command listed in `apps/rt/CLAUDE.md` → Commands.

## Return Format
### Files Modified/Created
| File | Action |
|------|--------|

### Build / Type-check
{output}

### Guards Verified
Total: {n}/{total} | Violations: {v}
