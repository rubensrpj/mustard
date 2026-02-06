# Tutorial: Mustard

Practical guide for using Mustard with Claude Code.

## Setup

```bash
cd my-project
npx mustard-claude init
```

Or install globally:

```bash
npm install -g mustard-claude
mustard init
```

## Pipeline Flow

```text
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

```text
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

```text
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

```text
You: /resume

Claude: "Active: add-email-person
        Phase: implement
        ✅ Database
        ⬜ Backend (pending)
        ⬜ Frontend

        Continue?"
```

## Context Loading (v2.2)

Agents automatically check for context changes:

1. Agent starts (e.g., Backend Specialist)
2. Checks git: `git diff --name-only HEAD -- .claude/context/shared/ .claude/context/backend/`
3. If changed → recompiles context to `prompts/backend.context.md`
4. Loads compiled context

**No manual commands needed** - context is always up-to-date.

## Customizing Context

Add project-specific patterns to context folders:

```text
.claude/context/
├── shared/           # All agents see this
│   └── conventions.md
├── backend/          # Backend Specialist sees this + shared
│   └── api-patterns.md
├── frontend/         # Frontend Specialist sees this + shared
│   └── component-patterns.md
└── database/         # Database Specialist sees this + shared
    └── schema-patterns.md
```

When you edit these files, agents will automatically recompile on next run.

## Enforcement Rules

Applied automatically:

| Rule | Effect |
|------|--------|
| L0 | Universal delegation via Task tool |
| L1 | Uses grepai instead of Grep/Glob |
| L2 | Requires pipeline for edits |
| L7-L9 | Repository patterns, SOLID |

## Tips

1. **Just describe** - No need to type `/feature`, Claude detects intent
2. **Review the spec** - Read before `/approve`
3. **Use resume** - `/resume` continues where you left off
4. **Use update** - `mustard update` gets new features without losing customizations
5. **Edit context files** - Add patterns to `context/{agent}/` folders

## Troubleshooting

| Issue | Solution |
|-------|----------|
| "No active pipeline" | Use `/feature <name>` |
| "Grep/Glob blocked" | Normal - Claude uses grepai |
| Build error | Claude shows errors, fix and continue |
| Lost customizations | Check `.claude.backup.{timestamp}` |
| Context not loading | Check `.claude/context/` folder exists |
