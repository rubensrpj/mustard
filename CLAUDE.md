# CLAUDE.md

Instructions for Claude Code when working with this repository.

> **Em português simples (1 parágrafo):** Mustard é uma "configuração pronta" para Claude Code. Quando você roda `mustard init` num projeto, ele cria a pasta `.claude/` com tudo que a IA precisa para trabalhar como um sênior: pipeline em fases (pesquisa → plano → execução → QA → fechamento), regras automáticas que evitam erros comuns (não rodar `rm -rf`, não passar de 40% da janela de contexto, não esquecer de testar, etc.), e um sistema que aprende com cada sessão. Esta página é a "primeira leitura" que a IA faz ao abrir o projeto — por isso é técnica de propósito.

## Project

Mustard is a CLI that generates `.claude/` folders for Claude Code projects. It creates prompts, commands, hooks, and rules. The repo is a **pnpm + Cargo monorepo**: the CLI lives in `apps/cli/` (crate `mustard-cli`), and a companion Tauri 2 + React 19 desktop dashboard lives in `apps/dashboard/`. The root `.claude/` is the monorepo orchestrator.

**Key concepts:**

- "Agents" are prompts loaded into `Task(general-purpose)` - custom subagent types don't work
- Only 4 native `subagent_type` values: `Explore`, `Plan`, `general-purpose`, `Bash`
- Enforcement via the Rust `mustard-rt` binary, dispatched per lifecycle event (`PreToolUse`/`PostToolUse`/`SessionStart`/`PreCompact`/`SessionEnd`/`SubagentStart`/`SubagentStop`/`UserPromptSubmit`)
- **Universal Delegation**: All code activities must be delegated via Task (separate context)
- **Skill+Recipe-driven context**: agents auto-load skills by description; recipes inject 90% skeletons by entity+operation
- **Auto-sync**: `mustard-rt run sync-detect`, `mustard-rt run sync-registry`
- **Namespaced Commands**: All commands use `mustard:` prefix (e.g., `/mustard:feature`)
- **Canonical methodology mapping**: ANALYZE↔Research, PLAN↔Spec+Plan, EXECUTE↔Implement (cf. [GitHub Spec Kit](https://github.com/github/spec-kit) RPI loop)

## L0 Rule - Universal Delegation

**CRITICAL:** The parent context (main) serves ONLY for:

- Receiving user requests
- Coordinating delegations via Task tool
- Presenting final results

**ALL** activities involving code MUST be delegated:

| Activity | Task Type |
|----------|-----------|
| Code exploration | `Task(Explore)` |
| Planning | `Task(Plan)` |
| Backend/APIs | `Task(general-purpose)` |
| Frontend/UI | `Task(general-purpose)` |
| Database | `Task(general-purpose)` |
| Bugfix | `Task(general-purpose)` |
| Code Review | `Task(general-purpose)` |
| Documentation | `Task(general-purpose)` |

## Build & Run

This repo is a **pnpm + Cargo monorepo**. The CLI lives in `apps/cli/`; the Tauri dashboard in `apps/dashboard/`.

```bash
# Install all workspace deps (run at repo root)
pnpm install

# Build / test the CLI (Rust crate mustard-cli)
cargo build -p mustard-cli
cargo test -p mustard-cli

# Build the dashboard
pnpm --filter mustard-dashboard build

# Initialize a project
cargo run -p mustard-cli -- init

# Update existing project
cargo run -p mustard-cli -- update
```

## Structure

```text
mustard/                         # monorepo root — pnpm-workspace.yaml + Cargo.toml
├── .claude/                     # root orchestrator (pipelines, hooks, registry)
├── packages/
│   └── core/                    # shared Rust library crate (mustard-core)
├── apps/
│   ├── cli/                     # the Mustard CLI (crate mustard-cli)
│   │   ├── src/                 # Rust source
│   │   │   ├── commands/        # init.rs, update.rs, config.rs, review.rs, add.rs
│   │   │   └── main.rs
│   │   └── templates/           # Templates (copied to target .claude/)
│   │       ├── CLAUDE.md        # Orchestrator rules (auto-loaded by Claude Code)
│   │       ├── settings.json    # Hook wiring + permissions + env modes
│   │       ├── pipeline-config.md
│   │       ├── commands/mustard/ # 18 namespaced slash commands
│   │       ├── skills/          # 13 foundation skills (karpathy, design-craft, hallmark, etc.)
│   │       ├── refs/            # Progressive-disclosure refs (loaded on demand)
│   │       ├── recipes/         # Structured recipes (90% skeletons)
│   │       └── context/qa/      # QA agent core context (only static .core.md kept)
│   │   # hooks + scripts are no longer a JS payload — enforcement and the
│   │   # sync/run subcommands all ship inside the mustard-rt binary.
│   ├── rt/                      # enforcement runtime (crate mustard-rt)
│   │   └── src/                 # Rust source — hooks/, run/, dispatch.rs, main.rs
│   └── dashboard/               # Tauri 2 + React 19 desktop dashboard (mustard-dashboard)
│       ├── src/                 # React/TypeScript UI
│       └── src-tauri/           # Rust backend
```

## Context Architecture

Mustard uses **skill+recipe-driven context loading** — agents receive context lazily, not from monolithic files.

### Loading sources (in order of preference)

| Source | Where | Loaded |
|---|---|---|
| **Project root rules** | `{root}/CLAUDE.md` | Auto, every session |
| **Subproject guards** | `{subproject}/CLAUDE.md` | Auto when working in subproject |
| **Foundation skills** | `templates/skills/{name}/SKILL.md` | Auto via skill description match |
| **Subproject patterns** | `{subproject}/.claude/skills/` | Auto via skill description match |
| **Recipes (structured)** | `.claude/recipes/{operation}.json` | Matched by `mustard-rt run recipe-match --entity --operation` |
| **Refs (progressive)** | `templates/refs/{cmd}/*.md` | Read on-demand by commands |
| **Stack/Modules** | `{subproject}/.claude/commands/{stack,patterns,guards,recipes,notes}.md` | Read on-demand |
| **Entity registry** | `.claude/entity-registry.json` | Grep by entity name |
| **QA core** | `templates/context/qa/qa.core.md` | Loaded by `/mustard:qa` |

### Methodology mapping (PRD ↔ Mustard)

| PRD term | Mustard phase | Reference |
|---|---|---|
| Research | ANALYZE | [GitHub Spec Kit](https://github.com/github/spec-kit) |
| Spec + Plan | PLAN | [Martin Fowler — SDD-3-tools](https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html) |
| Implement | EXECUTE | — |
| Acceptance | QA (Wave 10) | runnable AC commands |
| Close | CLOSE | sync registry + move spec |

### Sync flow (auto-discovery, monorepo-aware)

1. User invokes `/mustard:feature` or `/mustard:bugfix`
2. `mustard-rt run sync-detect` discovers subprojects + roles
3. `mustard-rt run sync-registry` scans entities (Drizzle/EF/Prisma/TypeORM/etc.)
4. Pipeline reads `entity-registry.json` for known entities
5. SHA256 hash skips recompilation when content unchanged

## CLI Flow

```text
mustard init
    -> scanProject() - detect stacks
    -> generateAll() - create .claude/ files + context structure
    -> generateMustardJson() - git flow config (interactive)

mustard update
    -> backup existing .claude/
    -> regenerate core files only
    -> preserve: CLAUDE.md, prompts/, context/*.md, mustard.json (user files)
```

## Model routing (`mustard-rt` `model_routing` module)

Models are auto-selected by intent. Upgrades blocked, downgrades allowed (opt-in via env).

| Intent | Model | Why |
|---|---|---|
| Explore (read-only search) | sonnet | quality-first; haiku allowed as opt-in downgrade |
| Plan | opus | bad plan = bad implementation |
| Feature pipeline (any) | opus | quality-first |
| Bugfix pipeline | opus | diagnosis needs deep reasoning |
| Default | sonnet | safe baseline |

## Commands

### Pipeline

- `/mustard:feature` - Start feature pipeline
- `/mustard:bugfix` - Start bugfix pipeline
- `/mustard:spec` - Approve or resume a spec (unified picker — letter to act, letter+r to approve+execute inline)
- `/mustard:complete` - Finalize pipeline

### Task (L0 Delegation)

- `/mustard:task-analyze` - Code analysis via Task(Explore)
- `/mustard:task-review` - Code review via Task(general-purpose)
- `/mustard:task-refactor` - Refactoring via Task(Plan) -> Task(general-purpose)
- `/mustard:task-docs` - Documentation via Task(general-purpose)

### Git (reads `mustard.json` for branch flow)

- `/mustard:git sync` - Pull parent branch into current
- `/mustard:git commit` - Simple commit
- `/mustard:git push` - Sync + commit + push
- `/mustard:git merge` - Promote to parent (local ff-only, no PRs)
- `/mustard:git merge main` - Promote dev → main (explicit)
- `/mustard:review [number|url]` - Review PR via Claude code-review

### Sync

- `/mustard:sync-registry` - Update entity registry
- `/mustard:sync-context` - Compile agent contexts
- `/mustard:validate` - Build + type-check
- `/mustard:status` - Project status

### Kill-switch

- `/mustard:unhook [--scope this|monorepo|all] [--confirm]` - Rename `.claude/settings.json` to `settings.json.disabled-<timestamp>` and wipe volatile harness state (`.agent-state/`, `.cluster-cache.json`, `.worktrees/`). `--scope all` requires `--confirm` to also touch `~/.claude/settings.json`.
- `/mustard:rehook [--scope this|monorepo|all] [--confirm]` - Reverse of `unhook`: restore the most recent `settings.json.disabled*` snapshot in each `.claude/` in scope.

## Enforcement Hooks (highlights)

Enforcement runs as the single Rust binary `mustard-rt` (the `apps/rt` crate): `settings.json` wires one `mustard-rt on <event>` entry per lifecycle event, and the dispatcher runs every registered module for that event. Highlights below — behavioral docs at `templates/pipeline-config.md`, module source at `apps/rt/src/hooks/`.

| `mustard-rt` module | Matcher | Behavior |
|------|---------|----------|
| `bash_guard` | `Bash` | **BLOCKS** rm -rf/mkfs/dd/credentials; redirects grep/ls/cat/head/tail/find → native tools; rewrites via `rtk`; commit gate (secrets/build) |
| `model_routing` | `Task` | **BLOCKS** upgrades vs routing table (downgrades allowed) |
| `tracker` | `.*` + Subagent | **BLOCKS** Explore agents at 15-20 tool uses (warn at 12); emits agent/tool/skill telemetry |
| `budget` | `Task` | **BLOCKS** Task prompts >per-role budget (Explore 10K chars, review 12K, general 30K); advisory >40% model window; warns on over-budget returns |
| `close_gate` | `Write\|Edit` to pipeline-states | **BLOCKS** CLOSE if build/lint/test/QA fail or checklist incomplete |
| `enforce_registry` | `Skill` | **BLOCKS** /feature, /bugfix if registry missing |
| `size_gate` | `Write\|Edit` | **WARNS** specs >500 lines (strict block opt-in); validates skill YAML frontmatter |
| `path_guard` | `Read\|Write\|Edit` | **BLOCKS** sensitive-file access; flags edits outside spec boundaries |
| `post_edit` | `Write\|Edit` (PostToolUse) | Auto-formats by extension; auto-marks Checklist items; guard-verify; pipeline-phase events |
| `knowledge` | `SessionEnd` / `PostToolUse(Task)` | **EXTRACTS** Decisões não-óbvias → `memory_decisions` table (SQLite); friction telemetry; `retry.attempt` events |
| `session_start` | `SessionStart` | Bootstraps the harness event bus; runs spec-hygiene; **INJECTS** top-N from `knowledge_patterns` + `memory_decisions` tables |
| `session_cleanup` | `SessionEnd` | Removes terminal pipeline-states + stale state files |
| `pre_compact` | `PreCompact` | **INJECTS** a working-state snapshot before compaction |
| `prompt_gate` | `UserPromptSubmit` | Archives pending `closed-followup` specs on a new pipeline command |

### Pre-Pipeline Validation Flow

```text
User: /mustard:feature add-login
         │
         ▼
    enforce_registry module (mustard-rt)
    - Registry exists? (BLOCK if not)
    - Version >= 3.x? (BLOCK if not)
         │
         ▼
    Pipeline starts...
```

## Sync & Run Subcommands

All of the following ship inside the `mustard-rt` binary — there is no JS
payload. They are invoked as `mustard-rt run <name>`.

### sync-detect

Auto-discovers subprojects in monorepos:

- Detection patterns: `.NET`, `React`, `Drizzle`, etc.
- Output: JSON with subprojects, agents, paths

### sync-registry

Generates `entity-registry.json` v3.1:

- Scans Drizzle schemas (`pgTable`, `pgEnum`)
- Scans .NET entities (`DbSet`, `class T`)
- Outputs `_patterns`, `_enums`, entity refs/subs

### security-scan

Scans for secrets, env exposure, and security misconfigurations:

- Detects leaked credentials, API keys, tokens
- Checks `.env` exposure and insecure patterns
- Reports findings with severity levels

### verify-pipeline

Runs build/test verification for the active pipeline:

- Executes build and test commands
- Reports pass/fail status per subproject
- Used during pipeline EXECUTE/CLOSE phases

### unhook / rehook

Harness kill-switch + restore:

- `unhook` renames every `.claude/settings.json` in scope to `settings.json.disabled-<timestamp>` and wipes volatile state (`.agent-state/`, `.cluster-cache.json`, `.worktrees/`).
- `rehook` reverses it: restores the newest `settings.json.disabled*` snapshot per `.claude/` in scope.
- Scope: `this` (default, repo only), `monorepo` (+ `apps/*/.claude/` + `packages/*/.claude/`), `all` (+ user-global `~/.claude/`, gated by `--confirm`).
- Output is a JSON report with one entry per `.claude/` touched and a `revert_with` one-liner.

## Project Structure

Monorepo (`pnpm-workspace.yaml`: `packages/*`, `apps/*` + `Cargo.toml` workspace).

| Subproject | Path | Technology | CLAUDE.md |
|------------|------|------------|-----------|
| CLI | `apps/cli/` | Rust — crate `mustard-cli`; the installer. `templates/` is the payload copied into target `.claude/` | [cli](./apps/cli/CLAUDE.md) |
| Runtime | `apps/rt/` | Rust — crate `mustard-rt`; enforcement hooks + `run` subcommands + MCP | [rt](./apps/rt/CLAUDE.md) |
| Core | `packages/core/` | Rust — crate `mustard-core`; shared library (config, io/SQLite, models) | [core](./packages/core/CLAUDE.md) |
| Dashboard | `apps/dashboard/` | Tauri 2 + React 19 + Tailwind 4 desktop app — `mustard-dashboard` | [dashboard](./apps/dashboard/CLAUDE.md) |

## Entity Registry

**CRITICAL:** Before searching for ANY entity, read `.claude/entity-registry.json` first.

## Ignore Paths

Never search in:
- `node_modules/`, `.next/`, `bin/`, `obj/`, `dist/`, `migrations/`

## Stacks Detected

TypeScript/JS, C#, Python, Java, Go, Rust, React, Next.js, .NET, FastAPI, Django, Drizzle, Prisma, TypeORM
