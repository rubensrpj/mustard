# Tutorial: Mustard

Practical guide for using Mustard with Claude Code.

## Setup

```bash
cd my-project
node path/to/mustard/cli/bin/mustard.js init
```

Or copy manually:

```bash
cp -r mustard/claude/ .claude/
```

## Pipeline Flow

```
You: "Add email field to Person"
         │
         ▼
Claude: "Detected change. Start pipeline?" → yes
         │
         ▼
EXPLORE: Claude analyzes with grepai
         │
         ▼
SPEC: "Will modify X, Y, Z. Approve?"
         │
         ▼
You: /approve
         │
         ▼
IMPLEMENT: Database → Backend → Frontend
         │
         ▼
VALIDATE: Build + type-check
         │
         ▼
You: /complete
```

## Commands

| Command | Description |
|---------|-------------|
| `/feature <name>` | Start feature |
| `/bugfix <error>` | Start bugfix |
| `/approve` | Approve spec |
| `/complete` | Finalize |
| `/resume` | Resume in new session |
| `/validate` | Build + type-check |
| `/status` | Project status |

## Example: Adding a Field

```
You: "Add email field to Person"

Claude: "Detected code change. Start pipeline?"

You: "yes"

Claude: [Explores with grepai]

        "## Spec: add-email-person

        Files:
        - schema/person.ts
        - backend/Person/...
        - frontend/person/...

        Approve?"

You: /approve

Claude: [Implements all layers]
        [Runs build]

        "Done. Finalize?"

You: /complete
```

## Example: Bug Fix

```
You: "Error: NullReferenceException saving contract"

Claude: "Detected bug. Start pipeline?"

You: "yes"

Claude: [Finds cause in ContractService.cs:145]

        "Cause: Partner validation doesn't check null
        Fix: Add null check

        Approve?"

You: /approve

Claude: [Fixes, validates]
        "Done. Finalize?"

You: /complete
```

## Analysis vs Implementation

Claude auto-detects intent:

- **Questions** → No pipeline: "How does ContractService work?"
- **Changes** → Pipeline: "Add CPF validation"

## Resuming

If you close Claude:

```
You: /resume

Claude: "Active: add-email-person
        Phase: implement
        ✅ Database
        ⬜ Backend (pending)
        ⬜ Frontend

        Continue?"
```

## Enforcement Rules

Applied automatically:

| Rule | Effect |
|------|--------|
| L1 | Uses grepai instead of Grep/Glob |
| L2 | Requires pipeline for edits |
| L7-L9 | Repository patterns, SOLID |

## Tips

1. **Just describe** - No need to type `/feature`, Claude detects intent
2. **Review the spec** - Read before `/approve`
3. **Use resume** - `/resume` continues where you left off
4. **Use update** - `mustard update` gets new features without losing customizations
5. **Use sync** - `mustard sync` updates prompts with current project context

## Syncing with Codebase

When your project evolves (new entities, changed architecture):

```bash
mustard sync
```

This updates:

- Prompts (auto-generated context section only)
- Context files (`context/*.md`)
- Entity registry

Your customizations in prompts are preserved - only the `<!-- MUSTARD:AUTO -->` section is updated.

## Troubleshooting

| Issue | Solution |
|-------|----------|
| "No active pipeline" | Use `/feature <name>` |
| "Grep/Glob blocked" | Normal - Claude uses grepai |
| Build error | Claude shows errors, fix and continue |
| Lost customizations | Check `.claude.backup.{timestamp}` |
| Outdated prompts | Run `mustard sync` to refresh context |
