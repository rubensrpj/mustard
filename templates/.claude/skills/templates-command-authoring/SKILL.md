---
name: templates-command-authoring
description: "Pattern for writing slash command SKILL.md files with trigger, procedure,
  and rules. Use when creating a new mustard command, adding a pipeline command,
  writing a /command, or the user says 'new command', 'add slash command',
  'create /feature-like command', 'write command template'."
---
<!-- mustard:generated at:2026-03-25T00:00:00.000Z role:general -->

# Command Authoring Pattern

Slash commands are SKILL.md files in `commands/mustard/{command-name}/SKILL.md` that define orchestrator behavior.

## Pattern

### File Convention
- Location: `commands/mustard/{command-name}/SKILL.md`
- No YAML frontmatter (commands are not auto-loaded by description)
- End with `ULTRATHINK` keyword **only for `/feature` and `/bugfix` commands** — do NOT add it to other commands

### Structure

```markdown
# /{command-name} - Title

> Advisory note (optional)

## Trigger
`/{command-name} <args>`

## Description
What the command does and when to use it.

## Procedure / ## Action
Step-by-step process. Use:
- Tables for signal → action mappings
- Numbered steps for sequential flow
- Code blocks for bash commands
- `### Phase` headers for multi-phase pipelines

## Rules
- Explicit constraints (NEVER, ALWAYS, MUST)
- Budget limits (max reads, max API calls)
- Delegation requirements
```

> Note: Add `ULTRATHINK` at the end only for `/feature` and `/bugfix` commands. All other commands end after the last rule/section.

### Key Rules
- Commands NEVER implement code directly — they orchestrate via Task tool
- Pipeline commands create spec files and pipeline state JSON
- Git commands read `mustard.json` for branch flow configuration
- Zero-confirmation: most commands execute without asking user

### Command Categories

| Category | Examples | Characteristic |
|----------|----------|---------------|
| Pipeline | `/feature`, `/bugfix`, `/approve`, `/complete`, `/resume` | Multi-phase, creates spec, state tracking |
| Task | `/task-analyze`, `/task-review`, `/task-refactor` | Single delegation, no spec |
| Git | `/git sync`, `/git commit`, `/git push`, `/git merge` | Reads `mustard.json`, submodule-aware |
| Scan | `/scan`, `/scan-format` | Discovery + analysis + generation |
| Status | `/status`, `/validate` | Read-only, reporting |

## Example

```markdown
# /my-command - Do Something

## Trigger
`/my-command <name>`

## Procedure
1. Read `.claude/pipeline-config.md` for agent config
2. Dispatch Task agent with context
3. Collect results and report

## Rules
- NEVER implement code directly
- ALWAYS delegate via Task tool
- Budget: max 5 API calls
```
Ref: `commands/mustard/feature/SKILL.md`, `commands/mustard/status/SKILL.md`

## References

For full code examples with variants:
> Read `references/examples.md`
