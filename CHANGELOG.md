# Changelog

## [3.0.0] - 2026-03-24

### Breaking Changes

- **CLI simplified**: `mustard init` now just copies templates (no scanning, no generation)
- **Removed Ollama**: No longer used — all intelligence lives in `/scan` skill inside Claude Code
- **Removed grepai**: No longer a dependency
- **Removed CLI flags**: `--ollama`, `--no-grepai`, `--verbose` removed from init/update
- **Removed old systems**: prompts/, context/, core/ directories no longer generated

### Removed

- `generators/commands.ts` — commands are now templates, not generated code
- `generators/hooks.ts` — hooks are now templates, not generated code
- `generators/prompts.ts` — prompt system eliminated
- `generators/claude-md-llm.ts` — Ollama generation removed
- `analyzers/llm.ts` — Ollama analysis removed
- `analyzers/semantic.ts` — grepai analysis removed
- `services/ollama.ts` — Ollama service removed
- `services/grepai.ts` — grepai service removed
- `scanners/` — all scanners removed (detection now done by `/scan` inside Claude Code)
- `templates/context/` — old compiled context system
- `templates/prompts/` — old prompt system
- `templates/core/` — old enforcement/pipeline docs
- `templates/commands/backend-*.md` — project-specific commands
- Dependencies: `ollama`, `glob`

### Changed

- **CLI is now a copier**: `mustard init` = copy `templates/` → `.claude/`, nothing more
- **CLI source**: from ~25 files to 5 files (`cli.ts`, `init.ts`, `update.ts`, `auto-update.ts`, `npm.ts`)
- **Version**: 2.0.14 → 3.0.0
- **Commands format**: flat `.md` files → subdirectories with `SKILL.md` (skill-creator standard)
- **Hooks**: 4 old generated hooks → 8 new template hooks (bash-safety, file-guard, enforce-registry, guard-verify, auto-format, pre-compact, session-cleanup, subagent-tracker)

### Added

- **14 pipeline skills** (SKILL.md format):
  - `feature`, `bugfix`, `approve`, `complete`, `resume` — pipeline lifecycle
  - `scan`, `scan-format` — codebase analysis
  - `git` — commit, push, merge, deploy (monorepo + single repo)
  - `maint` — deps, validate, sync
  - `task` — delegated analysis, audit, compare, review, refactor, docs
  - `knowledge` — notes, memory audit, reports
  - `skill` — install, create, list, remove, optimize, eval
  - `status` — consolidated status
  - `templates/agent-prompt` — agent dispatch template
- **6 bundled skills**: design-craft, react-best-practices, senior-architect, skill-creator, commit-workflow, pipeline-execution
- **8 enforcement hooks**: bash-safety, file-guard, enforce-registry, guard-verify, auto-format, pre-compact, session-cleanup, subagent-tracker
- **3 sync scripts**: sync-detect.js, sync-registry.js, statusline.js
- **pipeline-config.md**: agent dispatch configuration (populated by `/scan`)
- **settings.json**: full hook configuration with PreToolUse, PostToolUse, SessionStart, PreCompact, SessionEnd, SubagentStart, SubagentStop

---

## [2.0.14] - 2026-02-07

### Changed

- Last version with Ollama/grepai support
- Last version with code generation (scanners, analyzers, generators)

---

## [2.0.0] - 2026-02-05

### Added

- `mustard sync` command
- Auto-section markers for preserving user customizations
- Prompt merge functionality

---

## [1.8.0] - 2026-02-05

### Added

- **Mustard CLI** — initial framework-agnostic project setup
- Stack detection (.NET, React, Next.js, Python, Java, Go, Rust, ORMs)
- Monorepo support
- Semantic analysis via grepai
- LLM generation via Ollama

---

## [1.0.0] - 2025-12-01

### Added

- Initial framework
- Pipeline for features/bugfixes
- Rules L0-L5
- Basic commands
