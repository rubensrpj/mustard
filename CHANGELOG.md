# Changelog

## [3.0.0] - 2026-02-07

### Breaking Changes

- **Namespaced commands**: All commands now use `mustard:` prefix
  - `/feature` → `/mustard:feature`
  - `/bugfix` → `/mustard:bugfix`
  - `/commit` → `/mustard:commit`
  - `/validate` → `/mustard:validate`
  - `/task-*` → `/mustard:task-*`
  - Backward compatibility: hooks accept both variants

### Removed

- **Agent Teams** (`/feature-team`, `/bugfix-team`) - experimental feature discontinued
- **Checkpoint** (`/checkpoint`) - replaced by Context Reset workflow
- **Compile Context** (`/compile-context`) - now automatic via hooks
- **Team Lead prompt** (`team-lead.md`) and context folder
- **Naming prompt** (`naming.md`) - conventions moved to `.core.md` files
- **Report prompt** (`report.md`) - use daily/weekly report commands instead

### Changed

- **Context architecture**: `patterns.md` → `README.md` + `{agent}.core.md`
  - Each agent now has explicit identity, responsibilities, and return format
  - README.md provides extensibility guide
  - `.core.md` files contain role-based instructions
- **Hook triggers**: `UserPromptSubmit` → `PreToolUse` with `Skill` matcher
- **Template size**: 90% reduction (externalized to compiled context)
- **Entity Registry**: Updated to v3.1 format with `_patterns` and `_enums`

### Added

- **Sync scripts**:
  - `sync-detect.js` - auto-discovers subprojects in monorepos
  - `sync-compile.js` - compiles contexts with SHA256 caching
  - `sync-registry.js` - generates entity-registry.json from Drizzle/.NET
- **Backend operational commands**:
  - `backend-run.md` - start backend in background
  - `backend-stop.md` - stop backend
  - `backend-restart.md` - restart backend
  - `backend-logs.md` - filtered log output
- **Design Principles skill**: UI guidelines with 4px grid, typography, depth strategy
- **New context structure**:
  - `backend.core.md` - Backend Specialist identity
  - `frontend.core.md` - Frontend Specialist identity
  - `database.core.md` - Database Specialist identity
  - `bugfix.core.md` - Bugfix Specialist identity
  - `review.core.md` - Review Specialist identity
  - `orchestrator.core.md` - Orchestrator with detailed pipeline phases

---

## [2.6.1] - 2026-02-06

### Added

- **Subproject Commands Collection**: For monorepos, automatically collects commands from subprojects
  - Scans `{subproject}/.claude/commands/` folders
  - Maps subproject type by name (Backend → backend, FrontEnd → frontend, etc.)
  - Compiles commands into `context/{type}/{subproject}-commands.md`
  - Runs automatically during `mustard init`, `mustard update`, and `/compile-context`

### Changed

- Updated `/compile-context` to v2.4 with Step 1.5 for subproject command collection
- Added `detectSubprojectType()` function for mapping subproject names to context types
- Added `collectSubprojectCommands()` function in generators

## [2.5.0] - 2026-02-06

### Added

- **Agent Teams support** (experimental): Alternative to Task subagents for complex features
  - `/feature-team <name>` - Feature pipeline with Agent Teams
  - `/bugfix-team <error>` - Bugfix pipeline with competing hypotheses
  - New `team-lead.md` prompt for coordinating teammates
  - New `context/team-lead/` folder with coordination patterns
- **Mandatory Pipeline Invocation (L-1)**: Skills must be invoked BEFORE any analysis
- Context compilation moved to skill invocation (ensures contexts are ready)

### Changed

- Version bump from 2.4 to 2.5
- Section numbering in CLAUDE.md (0-15 sections)
- Removed "Context Loading" section from agent prompts (now in skill commands)
- Added "Agent Teams Mode" section to specialist prompts (backend, frontend, database, review)
- Updated `enforce-pipeline.js` hook v1.1.0 with Agent Teams options
- Updated `context/README.md` with team-lead folder structure

### Updated

- `feature.md` and `bugfix.md` commands now compile contexts as Phase 0
- All specialist prompts link to `team-lead.md`

## [2.4.0] - 2026-02-06

### Added

- Memory MCP search in agents
- Improved CLI output

### Changed

- Agent prompts now include "Context Loading" section with git-based verification
- Updated all agent prompts (backend, frontend, database, bugfix, review, orchestrator)

## [2.2.0] - 2026-02-06

### Added

- **Auto-compiled context**: Agents check git for changes and compile context automatically
- **Compiled context files**: `prompts/{agent}.context.md` generated on-demand by Claude
- Context synthesis: Claude removes redundancies and optimizes tokens when compiling

### Removed

- `mustard sync` command - context compilation is now automatic
- `/context-init` command - structure created by `mustard init`
- `/context-normalize` command - Claude normalizes during compilation

### Changed

- Agent prompts now include "Context Loading" section with git-based verification
- Updated all agent prompts (backend, frontend, database, bugfix, review, orchestrator)
- Simplified CLI to just `init` and `update` commands

## [2.1.0] - 2026-02-05

### Added

- **Task Commands** for L0 Universal Delegation:
  - `/task-analyze <scope>` - Code analysis via Task(Explore)
  - `/task-review <scope>` - Code review via Task(general-purpose)
  - `/task-refactor <scope>` - Refactoring via Task(Plan) -> Task(general-purpose)
  - `/task-docs <scope>` - Documentation via Task(general-purpose)
- New command files in `claude/commands/mustard/`
- Command generator in `cli/src/generators/commands.ts`

### Changed

- Translated `claude/CLAUDE.md` template to English for consistency
- Updated `.claude/CLAUDE.md` with Task Commands section
- Updated `.claude/core/enforcement.md` with L0 delegation rules
- Updated `.claude/hooks/enforce-pipeline.js` for new commands
- Updated `.claude/prompts/orchestrator.md` with delegation instructions
- Updated `CLAUDE.md` (root) with L0 rule and delegation map
- Updated `README.md` with new commands documentation

## [2.0.0] - 2026-02-05

### Added

- `mustard sync` command - syncs prompts and context with current codebase state
- Auto-section markers (`<!-- MUSTARD:AUTO-START -->`) for preserving user customizations in prompts
- Prompt merge functionality - updates project context without losing user edits

### Changed

- Prompts now include auto-generated project context section
- Entity registry updated during sync with newly discovered entities

## [1.9.0] - 2026-02-05

### Added

- `mustard update` command - updates core files while preserving customizations
- Context auto-generation (`context/architecture.md`, `context/patterns.md`, `context/examples/`)
- Memory MCP search in agent prompts

### Changed

- All prompts now search context before implementing
- CLI passes code samples to generators

## [1.8.0] - 2026-02-05

### Added

- **Mustard CLI** - framework-agnostic project setup
- Stack detection (.NET, React, Next.js, Python, Java, Go, Rust, ORMs)
- Monorepo support
- Semantic analysis via grepai
- LLM generation via Ollama
- Status line script (`scripts/statusline.js`)

### Options

- `--force` - overwrite existing .claude/
- `--yes` - skip confirmations
- `--no-ollama` - use templates
- `--no-grepai` - skip semantic analysis

## [1.7.0] - 2026-02-05

### Added

- Pipeline via Memory MCP (entities: `Pipeline:{name}`, `Spec:{name}`)
- Enforcement hooks (`enforce-pipeline.js`, `enforce-grepai.js`)
- Commands: `/approve`, `/complete`, `/resume`
- Auto-detection of change intent

## [1.6.0] - 2026-02-05

### Added

- SOLID patterns documentation (`core/solid-patterns.md`)
- Rule L9 (Interface Segregation)
- Entity Registry v2.1 compact format
- Commands: `/sync-registry`, `/install-deps`, `/report-daily`, `/report-weekly`

## [1.5.0] - 2026-01-15

### Changed

- Use only native `subagent_type` values (Explore, Plan, general-purpose, Bash)
- Agents are now prompts loaded into `Task(general-purpose)`
- Renamed `agents/` to `prompts/`

### Added

- Rules L6-L8 (DbContext, Repository, Registry)
- Mandatory pipeline phases

## [1.0.0] - 2025-12-01

### Added

- Initial framework
- Pipeline for features/bugfixes
- Rules L0-L5
- Basic commands
