# Commands

This folder contains Claude Code slash commands.

## Structure

```
commands/
├── mustard/          # Mustard core commands (managed by Mustard CLI)
│   ├── feature.md
│   ├── bugfix.md
│   ├── commit.md
│   └── ...
├── README.md         # This file
└── my-command.md     # Your custom commands (preserved on updates)
```

## Mustard Commands (`mustard/`)

Commands in the `mustard/` subfolder are managed by the Mustard CLI:
- **Created** during `mustard init`
- **Overwritten** during `mustard update`
- Do NOT edit these files - your changes will be lost on update

## User Commands (root folder)

Create your own commands directly in this folder (not in `mustard/`):
- **Preserved** during `mustard update`
- Full control over content
- Example: `my-deploy.md`, `run-tests.md`

## Creating a Custom Command

1. Create a `.md` file in this folder (e.g., `my-command.md`)
2. Use the following format:

```markdown
# /my-command - Description

## Trigger

`/my-command [args]`

## Description

What this command does.

## Actions

1. Step one
2. Step two
3. ...
```

3. The command will be available as `/my-command` in Claude Code
