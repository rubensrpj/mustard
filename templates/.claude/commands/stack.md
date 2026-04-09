<!-- mustard:generated at:2026-03-25T00:00:00.000Z role:general -->
# Stack: Templates

> Technology stack and tooling for the Mustard templates subproject.

## Runtime

| Component | Version | Notes |
|-----------|---------|-------|
| Node.js | >=18 | All hooks/scripts use CommonJS (`require`) |
| JavaScript | ES2020+ | Optional chaining, nullish coalescing, `Set`, `Map` |

## Dependencies

None. All template files are dependency-free — they use only Node.js built-in modules:

| Module | Used In |
|--------|---------|
| `fs` | All hooks, all scripts |
| `path` | All hooks, all scripts |
| `child_process` | `auto-format.js`, `pre-compact.js`, `statusline.js`, `sync-detect.js`, `sync-registry.js` |
| `crypto` | `sync-detect.js` (SHA-256 hashing) |
| `os` | `statusline.js`, `session-cleanup.js` |

## File Categories

| Category | Path | Count | Purpose |
|----------|------|-------|---------|
| Hooks | `hooks/*.js` | 8 | PreToolUse/PostToolUse/Session lifecycle guards |
| Scripts | `scripts/*.js` | 3 | Sync-detect, sync-registry, statusline |
| Commands | `commands/mustard/*/SKILL.md` | 14 | Slash command definitions |
| Skills | `skills/*/SKILL.md` | 6 | Foundation skills (design-craft, react-best-practices, etc.) |
| Config | `settings.json` | 1 | Hook wiring, permissions, statusline |
| Config | `.claude/pipeline-config.md` | 1 | Agent dispatch rules, wave system, model selection |
| Template | `CLAUDE.md` | 1 | Orchestrator rules template |

## Commands

```bash
# Run hook tests
node --test hooks/__tests__/hooks.test.js

# Run sync-detect (outputs JSON)
node scripts/sync-detect.js
node scripts/sync-detect.js --no-cache

# Run sync-registry
node scripts/sync-registry.js
node scripts/sync-registry.js --force
```

## Structure

```
templates/
├── CLAUDE.md                    # Orchestrator rules (copied to .claude/CLAUDE.md)
├── settings.json                # Hook wiring + permissions
├── pipeline-config.md           # Agent/wave/model config
├── commands/mustard/             # 14 slash commands
│   ├── feature/SKILL.md
│   ├── bugfix/SKILL.md
│   ├── scan/SKILL.md
│   ├── git/SKILL.md
│   └── ... (approve, complete, resume, status, task, etc.)
├── hooks/                        # 8 lifecycle hooks
│   ├── bash-safety.js            # PreToolUse — block dangerous commands
│   ├── file-guard.js             # PreToolUse — block sensitive files
│   ├── enforce-registry.js       # PreToolUse — require entity-registry
│   ├── auto-format.js            # PostToolUse — prettier/dotnet format
│   ├── guard-verify.js           # PostToolUse — architectural rules
│   ├── subagent-tracker.js       # Pre/SubagentStart/Stop — agent state
│   ├── pre-compact.js            # PreCompact — snapshot before compaction
│   ├── session-cleanup.js        # SessionEnd — prune stale state
│   └── __tests__/hooks.test.js   # Tests (node:test + node:assert)
├── scripts/
│   ├── sync-detect.js            # Subproject discovery + role detection
│   ├── sync-registry.js          # Entity registry generation
│   └── statusline.js             # ANSI statusline renderer
└── skills/                       # Foundation skills
    ├── commit-workflow/
    ├── design-craft/
    ├── pipeline-execution/
    ├── react-best-practices/
    ├── senior-architect/
    └── skill-creator/
```
