# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository state

This repo is currently a **Mustard scaffold awaiting subprojects** — there is no application source code yet (no root `package.json`, no API/UI/DB code, empty `entity-registry.json`). Everything under `.claude/` is the orchestration scaffold. The commands declared in `mustard.json` (`npm test`, `npm run build`, `npm run lint`, `tsc --noEmit`) will only work once a real subproject (with its own `package.json`) is added.

When the user asks to "create the API", "scaffold the dashboard", etc., they are populating this repo for the first time. Run `/scan` after the first subproject lands so the registry, agent files, and per-subproject `CLAUDE.md` get generated.

## Where to read what

| File | Purpose | Maintenance |
|------|---------|-------------|
| `.claude/CLAUDE.md` | Orchestrator rules (intent routing, Task delegation, pipeline phases, guards) — auto-loaded by Claude Code | **Generated** by `/scan` (`<!-- mustard:generated -->`). Do not edit by hand — changes are lost on next `/scan`. |
| `.claude/pipeline-config.md` | Wave 10 pipeline: phases, gates, model selection, escalation statuses, shared memory architecture | Manual — authoritative reference |
| `.claude/settings.json` | All 35+ hooks registered across lifecycle events; permissions; MCP servers | Manual |
| `mustard.json` | Declared build/test/lint/type-check commands and git flow (`dev → main`, GitHub provider) | Manual |
| `.claude/entity-registry.json` | Domain entities discovered by `/scan` | **Generated** — currently empty |
| `.claude/recipes/*.json` | Structured recipes consumed by `recipe-match.js` (5 entries today: add-field, add-endpoint, add-component, add-validation, null-guard) | Manual or `/scan`-generated |

## Commands that actually run today

```bash
# Run all hook + script tests (Node's built-in test runner)
node --test .claude/hooks/__tests__/*.test.js
node --test .claude/scripts/__tests__/*.test.js

# Run a single hook test file
node --test .claude/hooks/__tests__/hooks.test.js

# Subproject discovery (prints JSON to stdout; safe to run, idempotent)
node .claude/scripts/sync-detect.js
node .claude/scripts/sync-detect.js --no-cache

# Refresh entity registry from current code (no-op until subprojects exist)
node .claude/scripts/sync-registry.js
node .claude/scripts/sync-registry.js --force

# Validate generated skills under .claude/skills/ and per-subproject skills/
node .claude/scripts/skill-validate.js
node .claude/scripts/skill-validate.js --json

# Query the shared-memory views (harness event log)
node .claude/scripts/harness-views.js --view pipeline-state --spec <name> --compact
node .claude/scripts/harness-views.js --view session-summary --compact
```

The user's global `~/.claude/CLAUDE.md` instructs prefixing CLI commands with `rtk` for token compaction (`rtk git status`, `rtk node --test ...`). The `hooks/rtk-rewrite.js` PreToolUse hook already rewrites Bash commands transparently, so plain commands also get filtered — explicit `rtk` is only needed if RTK is not installed and you want to verify it's not being used.

## Architecture in one screen

Mustard is a **pure Node.js scaffold (CommonJS, zero npm deps)** that turns Claude Code into a multi-phase pipeline orchestrator. The moving parts:

1. **Hooks (`.claude/hooks/*.js`, 34 files)** — lifecycle event handlers wired in `settings.json`. PreToolUse hooks use `permissionDecision`; PostToolUse hooks use `decision`. All hooks **fail open** (exit 0 on any error) so a broken hook never blocks work. All use Node built-ins only.
2. **Scripts (`.claude/scripts/*.js`, 26 files)** — invokable utilities (discovery, registry sync, diff context, QA runner, metrics, harness views). Tests live in `.claude/scripts/__tests__/`.
3. **Slash commands (`.claude/commands/mustard/*/SKILL.md`, 18 commands)** — `/feature`, `/bugfix`, `/scan`, `/qa`, `/approve`, `/complete`, `/resume`, `/review`, `/task`, `/git`, `/status`, `/stats`, `/metrics`, `/knowledge`, `/maint`, `/skill`, `/scan-format`, `/templates:agent-prompt`.
4. **Skills (`.claude/skills/*/SKILL.md`, 8 foundation skills)** — auto-triggered context (karpathy-guidelines, commit-workflow, design-craft, react-best-practices, senior-architect, skill-creator, pipeline-execution, frontend-design).
5. **Shared memory (Wave 4)** — single truth source `.claude/.harness/events.jsonl` (append-only NDJSON). Projections: `knowledge.json`, `memory/decisions.json`, `memory/lessons.json`, `.pipeline-states/{spec}.json`. Read via `harness-views.js`, never by directly tailing the log.
6. **Pipeline** — ANALYZE → PLAN → EXECUTE → QA → CLOSE. Light scope skips PLAN. `close-gate.js` blocks CLOSE on build/type/lint/test failure or missing `qa.result`. Mode toggles via env (`MUSTARD_QA_GATE_MODE`, `MUSTARD_CLOSE_GATE_MODE`, etc., listed in `pipeline-config.md`).

## Hook authoring constraints

When adding or modifying a hook, every rule below is enforced by `/scan` or runtime, and breaking them causes silent regressions:

- **Node built-ins only** — never add an npm dependency. The whole scaffold runs without `node_modules`.
- **Fail open** — wrap the handler in try/catch and `process.exit(0)` on any unexpected error. Hooks bugs must not break user work.
- **Register in `settings.json`** with an explicit `timeout` (ms). Unregistered hooks never fire.
- **PreToolUse** returns `{ permissionDecision: "allow"|"deny"|"ask", ... }`. **PostToolUse** returns `{ decision: ... }`. Wrong shape = silently ignored.
- **Generated files** (anything emitted by `/scan` or a script) must begin with `<!-- mustard:generated -->` (or YAML frontmatter first, then the marker for skill `SKILL.md`).

## Adding a subproject

When the first subproject lands (e.g., `apps/api/`, `apps/web/`):

1. The subproject brings its own `package.json` and source code.
2. Run `/scan` (or `/scan <subproject>`) — this populates `entity-registry.json`, generates `.claude/agents/{role}-impl.md` and `-explorer.md`, creates per-subproject `CLAUDE.md`, refreshes Project Structure tables, and re-renders `.claude/CLAUDE.md`.
3. After that, `npm test`/`npm run build`/`npm run lint`/`tsc --noEmit` (from `mustard.json`) become meaningful — the pipeline runs them through `close-gate.js`.

Until a subproject exists, `/scan` will report no dispatches and most pipeline commands have nothing to act on.
