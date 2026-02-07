# Tutorial: Mustard v3.0

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
You: /mustard:approve
         │
         ▼
IMPLEMENT: Database → Backend → Frontend
         │
         ▼
VALIDATE: Build + type-check
         │
         ▼
You: /mustard:complete
```

## Commands

All commands now use the `mustard:` prefix:

| Command | Description |
|---------|-------------|
| `/mustard:feature <name>` | Start feature |
| `/mustard:bugfix <error>` | Start bugfix |
| `/mustard:approve` | Approve spec |
| `/mustard:complete` | Finalize |
| `/mustard:resume` | Resume in new session |
| `/mustard:validate` | Build + type-check |
| `/mustard:status` | Project status |

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

You: /mustard:approve

Claude: [Implements all layers]
        [Runs build]

        "Done. Finalize?"

You: /mustard:complete
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

You: /mustard:approve

Claude: [Fixes, validates]
        "Done. Finalize?"

You: /mustard:complete
```

## Analysis vs Implementation

Claude auto-detects intent:

- **Questions** → No pipeline: "How does ContractService work?"
- **Changes** → Pipeline: "Add CPF validation"

## Resuming

If you close Claude:

```text
You: /mustard:resume

Claude: "Active: add-email-person
        Phase: implement
        ✅ Database
        ⬜ Backend (pending)
        ⬜ Frontend

        Continue?"
```

## Context Architecture (v3.0)

Each agent has modular context with explicit identity:

```text
.claude/context/
├── shared/              # All agents load this
├── backend/
│   ├── README.md        # How to extend
│   └── backend.core.md  # Identity + Workflow
├── frontend/
│   ├── README.md
│   └── frontend.core.md
└── ...
```

### .core.md Files

Each specialist has explicit sections:

- **Identity**: "You are the Backend Specialist"
- **Responsibilities**: What to implement/not implement
- **Checklist**: Step-by-step workflow
- **Return Format**: Standardized response
- **Naming Conventions**: PascalCase, snake_case, kebab-case
- **Rules**: DO/DO NOT

## Customizing Context

Add project-specific patterns to context folders:

```text
.claude/context/
├── shared/           # All agents see this
│   └── my-patterns.md
├── backend/          # Backend Specialist sees this + shared
│   ├── backend.core.md  # Don't edit (managed by Mustard)
│   └── api-patterns.md  # Add your patterns here
├── frontend/         # Frontend Specialist sees this + shared
│   └── component-patterns.md
└── database/         # Database Specialist sees this + shared
    └── schema-patterns.md
```

When you edit these files, agents will automatically recompile on next run.

## Sync Scripts

Mustard v3.0 includes auto-sync scripts:

| Script | Purpose |
|--------|---------|
| `sync-detect.js` | Discovers subprojects in monorepos |
| `sync-compile.js` | Compiles contexts with SHA256 caching |
| `sync-registry.js` | Generates entity-registry.json |

These run automatically when you invoke pipeline commands.

## Enforcement Hooks

Applied automatically:

| Hook | Effect |
|------|--------|
| `enforce-registry.js` | Blocks if entity registry missing |
| `enforce-context.js` | Warns if contexts not compiled |
| `enforce-grepai.js` | Blocks Grep/Glob without path |
| `enforce-pipeline.js` | Reminds about pipeline for edits |

## Migration from v2.x

1. **Update command invocations**:

   ```bash
   # Before
   /feature add-login

   # After
   /mustard:feature add-login
   ```

2. **Regenerate registry**:

   ```bash
   /mustard:sync-registry --force
   ```

3. **Note removed features**:
   - Agent Teams (`/feature-team`, `/bugfix-team`) - removed
   - Checkpoint (`/checkpoint`) - use Context Reset instead

## Tips

1. **Just describe** - No need to type `/mustard:feature`, Claude detects intent
2. **Review the spec** - Read before `/mustard:approve`
3. **Use resume** - `/mustard:resume` continues where you left off
4. **Use update** - `mustard update` gets new features without losing customizations
5. **Edit context files** - Add patterns to `context/{agent}/` folders (not `.core.md`)

## Troubleshooting

| Issue | Solution |
|-------|----------|
| "No active pipeline" | Use `/mustard:feature <name>` |
| "Registry missing" | Run `/mustard:sync-registry` |
| "Grep/Glob blocked" | Normal - Claude uses grepai |
| Build error | Claude shows errors, fix and continue |
| Lost customizations | Check `.claude.backup.{timestamp}` |
| Context not loading | Run `/mustard:sync-context` |
