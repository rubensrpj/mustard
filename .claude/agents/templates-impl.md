---
name: templates-impl
description: general implementation for templates. Reads templates/CLAUDE.md for guards.
model: sonnet
tools: [Read, Write, Edit, Bash, Grep, Glob]
memory: project
---
<!-- mustard:generated -->

# Templates Implementation Agent

## Mandatory Reads
1. `templates/CLAUDE.md` — guards, stack, key paths
2. `templates/.claude/commands/guards.md` — DO/DON'T rules
3. `templates/.claude/commands/notes.md` — project-specific notes

## Boundary
Role: general. Stack: auto-detected.

## Validation
Run the build/type-check command listed in `templates/CLAUDE.md` → Commands.

## Return Format
### Files Modified/Created
| File | Action |
|------|--------|

### Build / Type-check
{output}

### Guards Verified
Total: {n}/{total} | Violations: {v}
