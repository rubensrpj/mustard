# Changelog

## [3.2.1] - 2026-02-05

### Added

- **Task Commands** for L0 Universal Delegation:
  - `/mtd-task-analyze <scope>` - Code analysis via Task(Explore)
  - `/mtd-task-review <scope>` - Code review via Task(general-purpose)
  - `/mtd-task-refactor <scope>` - Refactoring via Task(Plan) → Task(general-purpose)
  - `/mtd-task-docs <scope>` - Documentation via Task(general-purpose)
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

## [3.2.0] - 2026-02-05

### Added

- `mustard sync` command - syncs prompts and context with current codebase state
- Auto-section markers (`<!-- MUSTARD:AUTO-START -->`) for preserving user customizations in prompts
- Prompt merge functionality - updates project context without losing user edits

### Changed

- Prompts now include auto-generated project context section
- Entity registry updated during sync with newly discovered entities

## [3.1.0] - 2026-02-05

### Added

- `mustard update` command - updates core files while preserving customizations
- Context auto-generation (`context/architecture.md`, `context/patterns.md`, `context/examples/`)
- Memory MCP search in agent prompts

### Changed

- All prompts now search context before implementing
- CLI passes code samples to generators

## [3.0.0] - 2026-02-05

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

## [2.2.0] - 2026-02-05

### Added

- Pipeline via Memory MCP (entities: `Pipeline:{name}`, `Spec:{name}`)
- Enforcement hooks (`enforce-pipeline.js`, `enforce-grepai.js`)
- Commands: `/mtd-pipeline-approve`, `/mtd-pipeline-complete`, `/mtd-pipeline-resume`
- Auto-detection of change intent

## [2.1.0] - 2026-02-05

### Added

- SOLID patterns documentation (`core/solid-patterns.md`)
- Rule L9 (Interface Segregation)
- Entity Registry v2.1 compact format
- Commands: `/mtd-sync-registry`, `/mtd-sync-dependencies`, `/mtd-report-daily`, `/mtd-report-weekly`

## [2.0.0] - 2026-01-15

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
