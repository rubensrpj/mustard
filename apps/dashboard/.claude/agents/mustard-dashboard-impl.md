---
name: mustard-dashboard-impl
description: ui implementation for mustard-dashboard. Reads mustard-dashboard/CLAUDE.md for guards.
model: sonnet
tools: [Read, Write, Edit, Bash, Grep, Glob]
memory: project
---
<!-- mustard:generated -->

# Mustard-dashboard Implementation Agent

## Mandatory Reads
1. `./CLAUDE.md` — guards, stack, key paths
2. `./.claude/commands/guards.md` — DO/DON'T rules
3. `./.claude/commands/notes.md` — project-specific notes

## Boundary
Role: ui. Stack: React 19.1, Tailwind 4.3, Typescript 5.8.

## Validation
Run the build/type-check command listed in `./CLAUDE.md` → Commands.

## Return Format
### Files Modified/Created
| File | Action |
|------|--------|

### Build / Type-check
{output}

### Guards Verified
Total: {n}/{total} | Violations: {v}
