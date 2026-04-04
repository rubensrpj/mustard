<!-- mustard:generated -->
# Orchestrator Rules

## Role
You do NOT implement code — you delegate via Task tool.

## Intent Routing

| Intent | Signals | Action |
|--------|---------|--------|
| Feature | create, add, new entity, new CRUD, implement | Pipeline Feature (Full scope) |
| Enhancement | improve, adjust, change, add field/column, change behavior, optimize, update | Pipeline Feature (auto-detects Light/Full scope) |
| Bugfix | error, bug, not working, broken, fix, correct | Pipeline Bugfix |
| Analyze | analyze, audit, evaluate, check, compare, inspect, assess | Delegate via /task |
| Simple | config, docs, small refactor, rename, move | Delegate via Task |

Any change that touches production code (schema, API, UI) → Pipeline Feature.
Scope is auto-detected: Light (1-2 layers, ≤5 files, known pattern) vs Full (3+ layers, new entity).

## Pipeline Phases
ANALYZE → PLAN → EXECUTE → CLOSE
- Light scope: skip PLAN (ANALYZE → EXECUTE → CLOSE)
- Full scope: ANALYZE → PLAN → /approve → EXECUTE → CLOSE

## Context Loading
Agents auto-load skills from `{subproject}/.claude/skills/` based on task description.
Guards always loaded via `{subproject}/CLAUDE.md`.

## Stack

Node.js (>=18), CommonJS, no external dependencies. 8 lifecycle hooks, 3 sync scripts, 14 slash commands, 6 foundation skills.

## Commands

```bash
# Run hook tests
node --test hooks/__tests__/hooks.test.js

# Subproject discovery (outputs JSON)
node scripts/sync-detect.js
node scripts/sync-detect.js --no-cache

# Entity registry generation
node scripts/sync-registry.js
node scripts/sync-registry.js --force
```

## Guards

- All hooks fail-open (exit 0 on error) — never block due to hook bugs
- All hooks use only Node.js built-ins — no npm dependencies
- PreToolUse hooks use `permissionDecision` response format
- PostToolUse hooks use `decision` response format
- Every new hook must be registered in `settings.json` with a timeout
- Generated files must start with `<!-- mustard:generated -->` header
- Skills must have YAML frontmatter BEFORE the `<!-- mustard:generated -->` line

## Scan References

| File | Description |
|------|-------------|
| `.claude/commands/stack.md` | Technology stack, structure, tooling |
| `.claude/commands/patterns.md` | 12 recurring code patterns with refs |
| `.claude/commands/guards.md` | DO/DON'T rules for hooks, scripts, commands, skills |
| `.claude/commands/recipes.md` | Implementation recipes for new hooks, commands, skills, scripts |
| `.claude/commands/notes.md` | Manual notes (never overwritten) |

## Recommended Skills

- `templates-hook-protocol` — Hook stdin/stdout JSON protocol
- `templates-settings-wiring` — settings.json hook registration
- `templates-sync-detect` — Subproject discovery and role detection
- `templates-command-authoring` — Slash command SKILL.md structure
- `templates-skill-authoring` — Foundation/subproject skill creation

## Token Economy

RTK (Rust Token Killer) is integrated as core infrastructure. A PreToolUse hook automatically rewrites Bash commands through `rtk`, reducing token consumption by 60-90% on CLI outputs.

- **Hook**: `hooks/rtk-rewrite.js` — transparent, fail-open
- **Analytics**: `rtk gain` — view token savings
- **Statusline**: Shows real-time savings when RTK is active
- If RTK is not installed, the hook silently passes through (zero impact)

## Full Reference
Rules, pipeline, naming: `pipeline-config.md`
