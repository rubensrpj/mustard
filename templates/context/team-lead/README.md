# Team Lead Context

Context files for the Team Lead agent when using Agent Teams mode.

## Files

| File | Description |
|------|-------------|
| `coordination.md` | Patterns for spawning and coordinating teammates |
| `task-list.md` | Patterns for creating and managing shared task lists |

## How It Works

The Team Lead loads:

1. `context/shared/*.md` - Common conventions
2. `context/team-lead/*.md` - Team coordination patterns

These are compiled into:

```text
.claude/prompts/team-lead.context.md
```

## When to Use

Team Lead context is loaded when:

- Running `/feature-team` command
- Running `/bugfix-team` command
- Any Agent Teams based workflow

## Customization

Add project-specific coordination patterns:

- `workflow.md` - Custom team workflow steps
- `review-checklist.md` - Team review criteria
- `communication.md` - Project communication standards
