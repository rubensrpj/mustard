<p align="center">
  <img src="assets/logo.svg" width="120" alt="Mustard">
</p>

<h1 align="center">Mustard</h1>

<p align="center">
  <em>The perfect sauce for your Claude Code</em>
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/mustard-claude"><img src="https://img.shields.io/npm/v/mustard-claude?style=for-the-badge&color=yellow&label=npm" alt="npm"></a>
  <img src="https://img.shields.io/badge/node-%3E%3D18-green?style=for-the-badge&logo=node.js" alt="Node">
  <img src="https://img.shields.io/badge/license-MIT-blue?style=for-the-badge" alt="License">
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Claude_Code-Ready-blueviolet?style=flat-square&logo=anthropic" alt="Claude Code">
  <img src="https://img.shields.io/badge/Monorepo-supported-green?style=flat-square" alt="Monorepo">
  <img src="https://img.shields.io/badge/Single_Repo-supported-green?style=flat-square" alt="Single Repo">
</p>

---

## What is Mustard?

Mustard sets up a `.claude/` folder that turns Claude Code into a structured development pipeline with explicit phases (ANALYZE → PLAN → EXECUTE → QA → CLOSE), wave-based agent dispatch, enforcement hooks, and token economy.

- **18 pipeline commands** — feature, bugfix, approve, complete, resume, scan, scan-format, git, maint, qa, task, knowledge, skill, status, stats, metrics, review, plus the agent-prompt template
- **31 enforcement hooks** — bash safety, model routing gate, context budget (with Dumb Zone 40% advisory), output budget, close-gate (Wave 9+10), spec hygiene, spec/skill size gates, RTK rewrite, file guard, registry enforcement, memory auto-extract, PR detection (DORA), session knowledge, and more
- **7 bundled skills** — design-craft, react-best-practices, senior-architect, skill-creator, commit-workflow, pipeline-execution, karpathy-guidelines
- **25 utility scripts** — subproject detection, entity registry sync (with doc-comment glossary enrichment), recipe matcher, harness views (incl. DORA `pr-metrics`), QA runner, wave decomposition, skill validation, and more
- **5 stack-agnostic recipes** — add-field, add-endpoint, add-component, add-validation, null-guard (90% skeletons matched by entity + operation)
- **Token economy** — auto-installs [RTK (Rust Token Killer)](https://github.com/rtk-ai/rtk) to reduce CLI-output tokens by 60–90%; per-role context budget (Explore 10K / review 12K / general 30K chars hard-block); Dumb Zone advisory ≥40% of model window (Liu et al. 2023, Dex Horthy)
- **Hook profiles & env overrides** — minimal/standard/strict profiles via `_lib/hook-env.js`; disable individual hooks with `MUSTARD_DISABLED_HOOKS`
- **Cursor IDE adapter** (experimental) — `mustard init --cursor` installs a Cursor-compatible hook adapter
- **Monorepo + single repo** — `sync-detect.js` auto-discovers subprojects and roles
- **GitHub PR template** — `templates/.github/pull_request_template.md` auto-installed when GitHub remote detected
- **Methodology mapping** — ANALYZE↔Research, PLAN↔Spec+Plan, EXECUTE↔Implement (cf. [GitHub Spec Kit](https://github.com/github/spec-kit) RPI loop)

## Em português simples (guia rápido sem jargão)

Mustard é uma "configuração pronta" para usar Claude Code de forma profissional. Ele faz 4 coisas que importam:

1. **Quebra qualquer pedido em etapas claras.** Quando você pede "adicione um campo email na tabela de usuários", o Mustard NÃO sai codando direto — ele primeiro pesquisa o código (ANALYZE), monta um plano com checklist e critérios de aceitação (PLAN), executa o plano em ondas paralelas (EXECUTE), valida automaticamente o resultado (QA), e só então fecha (CLOSE). Isso reduz drasticamente o "código gerado errado" porque há freios em cada etapa.

2. **Evita o "burrinho do meio" (Dumb Zone).** Há um problema conhecido em IA: quando você joga muita coisa no chat (>40% da janela), a qualidade despenca — a IA "esquece" o que está no meio do contexto. O Mustard avisa quando você passa de 40% e sugere `/compact`. Isso vem de pesquisa real (Liu et al. 2023, conceito popularizado por Dex Horthy).

3. **Aprende com o que aconteceu.** Cada decisão arquitetural escrita num spec ("escolhemos Redis em vez de Memcached porque...") é automaticamente persistida em `memory/decisions.json` no fim da sessão. Da próxima vez que você abrir o Claude Code, ele já sabe o histórico — sem você ter que lembrar e copiar.

4. **Garante que frontend não pareça "código gerado".** Se você pede um componente de UI, o Mustard automaticamente carrega um checklist anti-AI-look (estados loading/empty/error, microinterações, acessibilidade, sem Lorem Ipsum, etc.) e — pra bugs visuais — um playbook de debug com Playwright + Chrome DevTools.

**O que é "vaporware" e por que isso importa?** Vaporware é o nome que se dá pra funcionalidade prometida na documentação mas que nunca foi de fato implementada (ou foi e quebrou silenciosamente). Versões anteriores do Mustard tinham 3 vaporwares estruturais: Recipe Engine sem recipes, sistema de memória sem ninguém escrevendo nele, e contagens de hooks erradas no CLAUDE.md. Esta versão eliminou os 3.

**Como o Mustard se compara ao GitHub Spec Kit?** Spec Kit é a alternativa oficial do GitHub para o mesmo problema (Spec-Driven Development). É bom, mas focado em multi-tool. Mustard é Claude-Code-first com 31 hooks de garantia, decomposição automática em ondas, gate executável de QA, suporte multilíngue PT/EN, e marcação automática de checklist. Os dois funcionam — Mustard ganha em automação para times Claude-only.

## Quick Start

```bash
# Install globally
npm install -g mustard-claude

# Initialize your project
cd my-project
mustard init

# Open Claude Code and run /scan
```

After `/scan`, use `/feature`, `/bugfix`, or `/task` to work through structured pipelines.

## Installation

### Prerequisites

- **Node.js** >= 18.0.0
- **Claude Code** CLI or IDE extension
- **RTK** (auto-installed) — [Rust Token Killer](https://github.com/rtk-ai/rtk) for token economy

### Option 1: Global Install (recommended)

```bash
npm install -g mustard-claude
```

After install, the `mustard` command is available globally:

```bash
mustard init
mustard update
mustard auto-update
```

### Option 2: Run Without Installing

```bash
npx mustard-claude init
npx mustard-claude update
npx mustard-claude auto-update
```

### Verify Installation

```bash
mustard --version
```

## How It Works

1. `mustard init` copies the `.claude/` structure into your project
2. RTK is auto-installed for token economy (60–90% savings on CLI outputs)
3. Inside Claude Code, run `/scan` to analyze your codebase
4. `/scan` generates guards, recipes, patterns, agents, and skills specific to your project
5. Use `/feature`, `/bugfix`, or `/task` to work through structured pipelines

The CLI is a **one-time setup tool**. All intelligence lives in the skills and hooks inside `.claude/`.

## CLI Commands

| Command | Description |
|---------|-------------|
| `mustard init` | Copy `.claude/` structure into current project |
| `mustard update` | Update core files (preserves user customizations) |
| `mustard auto-update` | Check npm for newer version and install |
| `mustard add <template>` | Install a community template |
| `mustard review --pr <N>` | Review a pull request (local or CI mode) |
| `mustard --version` | Show installed version |
| `mustard --help` | Show help |

### `mustard init`

| Option | Description |
|--------|-------------|
| `-f, --force` | Overwrite existing `.claude/` without backup |
| `-y, --yes` | Skip confirmation prompts (merge mode: skip existing files) |
| `--cursor` | Install Cursor IDE adapter at `.cursor/hooks/adapter.js` |

**Behavior:**
- If `.claude/` doesn't exist → copies all templates
- If `.claude/` exists → asks: backup & overwrite, merge (skip existing), or cancel
- Merge mode preserves all existing files and only adds new ones
- Auto-installs RTK if not present (silent, never blocks on failure)

### `mustard update`

| Option | Description |
|--------|-------------|
| `-f, --force` | Skip backup and confirmation |

**Recreates** (from latest templates):
- `commands/mustard/` — pipeline commands
- `hooks/` — enforcement hooks
- `skills/` — bundled skills
- `scripts/` — sync scripts
- `settings.json` — hook configuration

**Preserves** (user customizations):
- `CLAUDE.md` — orchestrator rules (populated by `/scan`)
- `pipeline-config.md` — agent dispatch config (populated by `/scan`)
- `entity-registry.json` — entity map (populated by sync-registry)
- `commands/*.md` — user commands outside `mustard/`
- `docs/`, `.agent-memory/`, `spec/`, `plans/`

### `mustard auto-update`

| Option | Description |
|--------|-------------|
| `--check-only` | Only check for updates, do not install |
| `-y, --yes` | Skip confirmation prompts |

### `mustard add`

| Option | Description |
|--------|-------------|
| `-f, --force` | Overwrite existing files |

**Sources:**
- GitHub: `github.com/mustard-templates/{name}`
- npm: `mustard-template-{name}`

```bash
mustard add template:dotnet-clean-arch
mustard add template:nextjs-app-router
```

### `mustard review`

| Option | Description |
|--------|-------------|
| `--pr <number>` | PR number to review (required) |
| `--ci` | CI mode: post as PR comment, exit 1 on critical issues |

**Requirements:** `gh` (GitHub CLI) and `claude` CLI.

```bash
# Interactive review
mustard review --pr 42

# CI mode (for GitHub Actions)
mustard review --ci --pr 42
```

## Pipeline Commands (inside Claude Code)

### Core Pipeline

| Command | Description |
|---------|-------------|
| `/scan` | Analyze codebase — generates guards, recipes, agents, skills |
| `/feature <name>` | Start feature pipeline (ANALYZE → PLAN → EXECUTE → REVIEW → CLOSE). Auto-detects Light / Extended-Light / Full scope. Prints full spec before asking for approval |
| `/bugfix <error>` | Autonomous bug fix (diagnose → fix → validate). Fast Path skips spec; Full Path writes and presents the spec before `/approve` |
| `/approve [--resume]` | Approve the active spec. With `--resume`, immediately chains into the `/resume` flow in the same session |
| `/resume` | Resume an interrupted pipeline from the last checkpoint |
| `/complete` | Finalize or cancel a pipeline |

### Operations

| Command | Description |
|---------|-------------|
| `/git <action>` | commit, push, sync, merge, deploy — handles monorepo git flow (`/git merge main` cascades from any branch) |
| `/maint <action>` | deps, validate, sync — maintenance utilities |
| `/status` | Consolidated git + pipeline + build + registry status |
| `/review [number\|url]` | Review a PR locally using the bundled review skill |
| `/dashboard [start\|stop\|status]` | Local web dashboard (`http://localhost:7878`) with live spec progress, telemetry, PRD builder, settings editor, glossary and command catalog — see [Dashboard](#dashboard) |

### Analysis & Delegation

| Command | Description |
|---------|-------------|
| `/task analyze <scope>` | Code exploration via Explore agent |
| `/task audit <domain> <scope>` | Quality audit (copy, design, a11y, i18n, consistency, api-contract) |
| `/task compare <criteria>` | Cross-subproject comparison (parallel explorers + consolidation) |
| `/task review <scope>` | Code review (SOLID, security, performance) |
| `/task refactor <scope>` | Plan + approve + implement refactoring (prints full plan before approval) |
| `/task docs <scope>` | Documentation generation |
| `/task implement <scope>` | Single-dispatch standardized implementation (low-cost, no audit gate) |

### Metrics & Knowledge

| Command | Description |
|---------|-------------|
| `/stats` | Pipeline metrics, token savings, performance |
| `/metrics` | Enforcement metrics report — hook hit rates, budget distributions, gate activity. Supports `--compare <from> <to>` (git tag or ISO date) to diff two windows |
| `/knowledge notes [target]` | Manage project observations |
| `/knowledge audit` | Audit memory for duplicates |
| `/knowledge report daily\|weekly` | Progress reports from git data |

### Skills

| Command | Description |
|---------|-------------|
| `/skill list` | List installed skills |
| `/skill install <source>` | Install skill from local path or GitHub |
| `/skill create <name>` | Create new skill via skill-creator |
| `/skill optimize <name>` | Optimize skill triggering descriptions |

## Dashboard

The Mustard dashboard is a local web UI served by `node .claude/scripts/dashboard.js` on `http://localhost:7878`. Zero npm dependencies (Node built-ins only), no auth, localhost-bound. Started/stopped via the `/dashboard` slash command.

### Starting the dashboard

```bash
# Inside Claude Code
/dashboard            # starts (or shows the URL if already running)
/dashboard status     # check if running
/dashboard stop       # kill the server
```

The PID is stored in `.claude/.dashboard.pid` (gitignored).

### Tabs

| Tab | What it shows |
|-----|---------------|
| **Overview** | Live KPIs (active specs, completed, tokens saved, today's events), the spec currently in production with progress bar, and recent harness activity. Polls every 12s. |
| **Specs** | Active specs grouped (epics show their child waves nested with mini progress bars). Completed specs filtered by period (7d/15d/30d/60d/90d/all), grouped by month. Click any spec → side panel with markdown viewer; click any wave → live monitor with stream. |
| **Telemetry** | Six sections: tokens (RTK + hooks), pipeline aggregates with **Pass@1**, phase distribution bar, active specs aging (<7d / 7–30d / >30d), hooks table with per-hook plain-language explanation, tools breakdown, 7-day events line chart, storage + knowledge sizes. |
| **Compose PRD** | Form-based PRD generator following the `tools/prd-builder.html` standard. Fields: title, project (auto-detected from monorepo), type, scope, priority, route, entity, CRUD ops, layers, design, bug repro, AC, constraints, OOS. Live preview, copy / copy-with-`/mustard:feature` / download. |
| **Comandos** | Catalog of every `/mustard:*` command with both a plain-language explanation (for non-developers) and a technical one (for developers). Filterable by category and full-text search. |
| **Settings** | Mustard env vars (`MUSTARD_*` keys) grouped by purpose (Pipeline Gates, Hygiene, Tool Use, UX & Profile). Each option (strict/warn/off) is a clickable card explaining what it does. Persists to `.claude/settings.json` `env` block. |
| **Glossário** | All acronyms and concepts used across the dashboard (CI, QA, AC, RTK, PRD, hooks, gates, agents, etc) with definitions. The same terms get hover tooltips throughout the UI via `<abbr>` tags. |

### Live indicator

A green pulsing pill ("ao vivo") appears on any spec/wave whose `lastActivity` is < 5 min ago (read from `.claude/.pipeline-states/<spec>.metrics.json`). A sticky banner at the top of every tab shows "Processando: <epic> · wave <X>" while a pipeline is running, with an "Acompanhar" button that opens the live monitor side panel (polling 3s).

### HTTP routes

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/` | The single-page UI (HTML + CSS + JS, ~110 KB) |
| `GET` | `/api/specs` | All specs (active + completed) with checklist progress, lastActivity, apiCalls, retries |
| `GET` | `/api/spec?path=<rel>` | Single spec markdown + metrics |
| `GET` | `/api/spec/live?spec=<name>` | Live view: events filtered by spec, phase, agentAttempts, toolBreakdown, isLive flag, plus checklist + summary fallback when no metrics file exists |
| `GET` | `/api/metrics` | Hook events table + RTK savings + last 7 days (parsed from `metrics-collect.js`) |
| `GET` | `/api/telemetry-extra` | Pipeline aggregates (runs, Pass@1, totals), phase distribution, active aging, storage breakdown, detect cache info, activeNow |
| `GET` | `/api/events?n=200` | Tail of `.claude/.harness/events.jsonl` |
| `GET` | `/api/projects` | Subprojects detected by `sync-detect.js` (read from `.detect-cache.json`) |
| `GET` | `/api/commands` | Catalog of `/mustard:*` slash commands with leigo + tecnico explanations |
| `GET` | `/api/settings` | Current Mustard env values + catalog (with `valueDocs` per option) |
| `POST` | `/api/settings` | Update Mustard env block in `.claude/settings.json` (validates against catalog) |
| `POST` | `/api/prd` | Generate a spec.md from the Compose PRD form (creates `.claude/spec/active/<date>-<slug>/spec.md`) |

### Files

```
.claude/scripts/
├── dashboard.js                    # HTTP server (~600 lines, http/fs built-ins only)
├── dashboard-ui.js                 # CSS + ICONS + CLIENT_JS + renderHtml
├── dashboard-prd-template.js       # generatePrdMarkdown() following Mustard spec format
├── dashboard-env-catalog.js        # MUSTARD_* env catalog with valueDocs
└── dashboard-commands-catalog.js   # /mustard:* command catalog with leigo+tecnico
```

## What Gets Installed

```
.claude/
├── CLAUDE.md                          # Orchestrator rules (template)
├── pipeline-config.md                 # Agent dispatch config (template)
├── settings.json                      # Hooks + permissions + statusline
├── entity-registry.json               # Empty skeleton (populated by /scan)
├── commands/mustard/                  # Pipeline commands
│   ├── feature/SKILL.md               #   /feature — feature pipeline
│   ├── bugfix/SKILL.md                #   /bugfix — bug fix pipeline
│   ├── approve/SKILL.md               #   /approve [--resume] — approve spec
│   ├── complete/SKILL.md              #   /complete — finalize pipeline
│   ├── resume/SKILL.md                #   /resume — resume pipeline
│   ├── scan/SKILL.md                  #   /scan — analyze codebase
│   ├── scan-format/SKILL.md           #   /scan agent format rules
│   ├── git/SKILL.md                   #   /git — commit, push, merge, deploy
│   ├── maint/SKILL.md                 #   /maint — deps, validate, sync
│   ├── task/SKILL.md                  #   /task — delegated analysis/review/refactor
│   ├── knowledge/SKILL.md             #   /knowledge — notes, audit, reports
│   ├── skill/SKILL.md                 #   /skill — manage skills
│   ├── status/SKILL.md                #   /status — consolidated status
│   ├── stats/SKILL.md                 #   /stats — pipeline metrics
│   ├── metrics/SKILL.md               #   /metrics — enforcement metrics report
│   ├── review/SKILL.md                #   /review — PR review
│   └── templates/agent-prompt/        #   Agent prompt template
├── recipes/                           # Stack-agnostic recipes (5)
│   ├── add-field.json                 #   Schema + DTO + optional FE form
│   ├── add-endpoint.json              #   Handler + DTO + service + route
│   ├── add-component.json             #   UI component with anti-AI-look defaults
│   ├── add-validation.json            #   Form validation + error UX
│   └── null-guard.json                #   Bugfix recipe for null/undefined
├── refs/                              # Progressive-disclosure references
│   ├── feature/fe-craft-check.md      #   Anti-AI-look checklist for UI work
│   ├── bugfix/browser-debug.md        #   Playwright + Chrome DevTools MCP playbook
│   └── feature/spec-language.md       #   Spec format + Component Contract
├── hooks/                             # Enforcement hooks (31)
│   ├── _lib/hook-env.js               #   Shared runtime controls (profiles, env overrides)
│   ├── rtk-rewrite.js                 #   Rewrites Bash commands through RTK
│   ├── bash-safety.js                 #   Blocks dangerous commands
│   ├── bash-native-redirect.js        #   Redirects grep/ls/cat/find → native tools
│   ├── file-guard.js                  #   Blocks sensitive file access
│   ├── enforce-registry.js            #   Blocks pipeline if no registry
│   ├── guard-verify.js                #   Validates architectural rules
│   ├── auto-format.js                 #   Auto-formats on write
│   ├── pre-compact.js                 #   Saves state before compaction
│   ├── session-cleanup.js             #   Cleans transient state on session end
│   ├── subagent-tracker.js            #   Tracks agent lifecycle, logs dispatch failures
│   ├── session-memory.js              #   Injects persistent memory on session start
│   ├── review-gate.js                 #   Pre-commit validation (fail-open)
│   ├── metrics-tracker.js             #   Tracks pipeline API calls, retries, gate saves
│   ├── memory-auto-extract.js         #   SessionEnd: extracts decisions/lessons from active specs
│   ├── pr-detect.js                   #   PostToolUse(Bash): emits pr.opened/pr.merged DORA events
│   ├── session-knowledge.js           #   Extracts patterns from session before cleanup
│   ├── session-knowledge-inc.js       #   Incremental knowledge capture
│   ├── context-budget.js              #   Per-role hard block + Dumb Zone advisory (≥40% window)
│   ├── spec-hygiene.js                #   Guards spec format + checkbox integrity
│   ├── spec-size-gate.js              #   Warns/blocks specs >500 lines
│   ├── skill-size-gate.js             #   Warns/blocks SKILL.md >500 lines
│   ├── skill-validate-gate.js         #   Validates skill YAML frontmatter
│   ├── close-gate.js                  #   Wave 9+10 strict gate: build/test/lint/QA + checklist
│   ├── checklist-auto-mark.js         #   PostToolUse: auto-marks checklist items by file pista
│   ├── output-budget.js               #   Caps agent return-size (advisory)
│   ├── tool-use-counter.js            #   Caps Explore agents at 15-20 tool uses
│   ├── model-routing-gate.js          #   Blocks model upgrades vs routing table
│   ├── duplication-check.js           #   Levenshtein vs entity registry (default off)
│   ├── convention-check.js            #   Conventions from knowledge.json (default off)
│   ├── recommended-skills-audit.js    #   Warn if >10 skills in agent prompt
│   ├── pipeline-phase.js              #   Records phase events to harness
│   ├── harness-init.js                #   SessionStart: rotates events.jsonl + index
│   ├── _lib/size-gate.js              #   Shared lib for size-based gates
│   └── user-prompt-hint.js            #   Surfaces contextual hints on prompt input
├── scripts/                           # Utility scripts (25)
│   ├── sync-detect.js                 #   Detects subprojects + roles (SHA-256 incremental)
│   ├── sync-registry.js               #   Generates entity-registry.json (_patterns.discovered[])
│   ├── skill-validate.js              #   Validates SKILL.md frontmatter across subprojects
│   ├── statusline.js                  #   Claude Code statusline
│   ├── memory-persist.js              #   Persists decisions/lessons across sessions
│   ├── memory-write.js                #   Writes agent memory entries between waves
│   ├── diff-context.js                #   Generates git diff summary for agents
│   ├── knowledge-update.js            #   Updates project knowledge base
│   ├── metrics-collect.js             #   Collects pipeline metrics
│   ├── metrics-report.js              #   Renders enforcement metrics report
│   ├── rtk-gain-import.js             #   Imports RTK token-savings data for metrics
│   ├── security-scan.js               #   Scans for secrets / security misconfigs
│   ├── verify-pipeline.js             #   Runs build/test verification
│   ├── analyze-validation.js          #   Validates ANALYZE phase output
│   ├── recipe-match.js                #   Structured recipe matcher (entity + operation)
│   └── _metrics-write.js              #   Internal metrics writer (used by hooks)
├── memory/                            # Persistent memory (auto-created)
│   ├── decisions.json                 #   Decisions across pipelines
│   └── lessons.json                   #   Lessons learned
├── .agent-memory/                     # Wave-to-wave agent handoff memory
├── .pipeline-states/                  # Active pipeline state + diff snapshots
├── spec/
│   ├── active/                        #   In-progress specs
│   └── completed/                     #   Archived specs
├── knowledge.json                     # Evolutionary knowledge base
├── metrics/                           # Pipeline metrics archive
├── adapters/cursor/                   # Cursor IDE adapter (experimental)
│   ├── README.md                      #   Setup and usage guide
│   └── adapter.js                     #   Cursor ↔ Claude Code hook protocol adapter
└── skills/                            # Bundled skills (7)
    ├── karpathy-guidelines/           #   Anti-slop principles (mandatory pre-Edit/Write)
    ├── design-craft/                  #   UI design methodology
    ├── react-best-practices/          #   React/Next.js optimization (40+ rules)
    ├── senior-architect/              #   System architecture patterns
    ├── skill-creator/                 #   Create and optimize skills
    ├── commit-workflow/               #   Git commit strategy
    └── pipeline-execution/            #   Pipeline phases, waves, retries
```

## How `/scan` Works

`/scan` is the most important command. It runs inside Claude Code and:

1. **Detects subprojects** — reads git submodules or scans for `CLAUDE.md` files
2. **Incremental detection** — compares source hashes to skip unchanged subprojects
3. **Launches analysis agents** — one per subproject, in parallel
4. **Generates per-subproject**:
   - `{subproject}/CLAUDE.md` — stack, commands, guards
   - `{subproject}/.claude/commands/` — guards, recipes, patterns, modules
   - `{subproject}/.claude/skills/` — granular pattern skills
   - `.claude/agents/{subproject}-impl.md` — implementation agent
   - `.claude/agents/{subproject}-explorer.md` — read-only explorer
5. **Updates root files** — `CLAUDE.md`, `pipeline-config.md`, `entity-registry.json`

After `/scan`, pipeline commands have full context to dispatch specialized agents.

## Pipeline Flow

```
/feature <name>
     │
     ▼
  ANALYZE — read registry + pipeline-config, determine layers, classify scope
     │
     ▼
  PLAN — create spec with tasks per agent, print spec inline for user review
     │
     ▼ (user approves — inline on Light, /approve on Full, /approve --resume to chain)
     │
  EXECUTE — dispatch agents per wave (DB+Backend ∥, Frontend after or parallel-safe)
     │
     ▼
  REVIEW — mandatory per-subproject review (SOLID, patterns, i18n, design, …)
     │
     ▼
  CLOSE — sync registry, move spec to completed/, cleanup state
```

### Scope detection

| Scope | Signals | Flow |
|-------|---------|------|
| **Light** | 1–2 layers, ≤5 files, known pattern, no new entity | ANALYZE → EXECUTE → CLOSE (one session) |
| **Extended Light** | Entity in registry + modification + ≤8 files, no new table/enum | Same as Light |
| **Full** | 3+ layers, 5+ files, new entity/CRUD, or new pattern | ANALYZE → PLAN → `/approve` → EXECUTE → CLOSE |

### Approval & resume shortcut

- `/approve` — prepares state and STOPS. User opens a new session and runs `/resume` (clean context).
- `/approve --resume` — prepares state and immediately continues to EXECUTE in the same session (skips the session hop; trades context freshness for convenience).

## Enforcement Hooks

Hooks are fail-open (any hook error exits 0 — pipeline is never blocked by a hook bug). They follow the PreToolUse / PostToolUse / Subagent / Session lifecycle.

### Cost-optimization hooks

| Hook | Matcher | Mode | Effect |
|------|---------|------|--------|
| `bash-native-redirect.js` | Bash | strict/warn/off | Blocks grep/ls/cat/head/tail/find — suggests Grep/Glob/Read |
| `model-routing-gate.js` | Task | strict/warn/off | Blocks model upgrades vs routing table |
| `tool-use-counter.js` | `.*` + Subagent | hard | Caps Explore agents at 15-20 tool uses (warn at 12) |
| `context-budget.js` | Task | strict/warn/observe | Per-role hard block (Explore 10K / review 12K / general 30K chars) + Dumb Zone advisory ≥40% of model window |
| `output-budget.js` | Subagent | warn | Caps agent return size (advisory) |
| `rtk-rewrite.js` | Bash | transparent | Routes CLI output through RTK for 60–90% token reduction |

### Safety & validation hooks

| Hook | Purpose |
|------|---------|
| `bash-safety.js` | Blocks dangerous commands (`rm -rf /`, force-push to main, etc.) |
| `file-guard.js` | Blocks access to `.env`, secrets, credentials |
| `enforce-registry.js` | Blocks pipeline commands if `entity-registry.json` is missing |
| `guard-verify.js` | Validates architectural rules during EXECUTE |
| `review-gate.js` | Pre-commit validation (fail-open) |
| `spec-hygiene.js` | Guards spec format + checkbox integrity |
| `spec-size-gate.js` / `skill-size-gate.js` | Warns/blocks files >500 lines (shared lib `_lib/size-gate.js`) |
| `close-gate.js` | Wave 9+10 strict gate: blocks CLOSE if build/lint/test/QA fail or checklist incomplete |
| `memory-auto-extract.js` | SessionEnd: extracts `## Decisões` / `## Lessons` from active specs into `memory/decisions.json` and `lessons.json` |
| `pr-detect.js` | PostToolUse(Bash): detects `gh pr create\|merge` and emits DORA events (`pr.opened`, `pr.merged`) |

### Memory & telemetry hooks

| Hook | Purpose |
|------|---------|
| `session-memory.js` | Injects persistent memory on session start |
| `session-cleanup.js` | Cleans transient state on session end |
| `session-knowledge.js` | Extracts patterns from session before cleanup |
| `session-knowledge-inc.js` | Incremental knowledge capture |
| `pre-compact.js` | Saves state before Claude's auto-compaction |
| `subagent-tracker.js` | Tracks agent lifecycle, logs dispatch failures |
| `metrics-tracker.js` | Pipeline telemetry (API calls, retries, gate saves) |
| `auto-format.js` | Auto-formats on Write/Edit |
| `user-prompt-hint.js` | Surfaces contextual hints on prompt input |

### Environment overrides

```bash
# Change mode (default: strict for cost-optimization hooks)
MUSTARD_BASH_REDIRECT_MODE=warn
MUSTARD_MODEL_GATE_MODE=off

# Disable individual hooks
MUSTARD_DISABLED_HOOKS=bash-native-redirect,model-routing-gate

# Switch profile
MUSTARD_PROFILE=minimal   # minimal | standard (default) | strict
```

See `.claude/hooks/_lib/hook-env.js` for the full list.

## Token Economy

Mustard integrates [RTK (Rust Token Killer)](https://github.com/rtk-ai/rtk) as core infrastructure:

- **Auto-install** — `mustard init` and `mustard update` install RTK silently if not present
- **Transparent hook** — `rtk-rewrite.js` (PreToolUse:Bash) wraps every Bash command through `rtk`, compressing output before it enters Claude's context
- **Fail-open** — if RTK is not available, the hook passes through with zero impact
- **Statusline** — real-time token savings displayed in the Claude Code status bar
- **Pipeline report** — `/complete` and `/stats` show total tokens saved

| Command type | Typical savings |
|--------------|-----------------|
| `git status/diff/log` | 75–80% |
| `npm test` / `cargo test` | 90–99% |
| `git add/commit/push` | 92% |
| Build output (`next build`, `tsc`, `cargo build`) | 80–90% |
| `ls` / `tree` / `grep` | 80% |

RTK only applies to Bash tool calls. Claude Code's built-in tools (Read, Grep, Glob) are already optimized and bypass the hook.

## Persistent Memory

Mustard maintains lightweight persistent memory across sessions:

- **Decisions** (`.claude/memory/decisions.json`) — architectural and implementation decisions
- **Lessons** (`.claude/memory/lessons.json`) — what went wrong and the correction applied
- **Knowledge base** (`.claude/knowledge.json`) — patterns, conventions, entities discovered across pipelines
- **Agent memory** (`.claude/.agent-memory/`) — wave-to-wave handoff summaries within a pipeline
- **Pipeline state** (`.claude/.pipeline-states/`) — active pipeline state + per-phase diff snapshots

Memory is automatically:
- **Injected** into each new session and into every dispatched agent
- **Capped** (50 entries memory / 200 entries knowledge — oldest pruned)
- **Never cleaned** by session cleanup — persists until manually removed

## CI Integration

Review PRs automatically in your CI pipeline:

```yaml
# GitHub Actions
- name: Mustard PR Review
  run: mustard review --ci --pr ${{ github.event.pull_request.number }}
```

CI mode:
- Fetches PR diff via `gh pr diff`
- Runs Claude review with project guards and rules
- Posts review as PR comment
- Exits with code 1 if CRITICAL issues found (fails the build)

Requires `gh` and `claude` CLI in the CI environment.

## Supported Projects

Mustard is **framework-agnostic**. The CLI just copies templates. `/scan` handles detection:

| Type | Examples |
|------|----------|
| **Backend** | .NET, Node.js (Express/Fastify/NestJS), Python (FastAPI/Django), Go, Rust, Java (Spring Boot) |
| **Frontend** | React, Next.js, Vue, Nuxt, Svelte, Angular |
| **Mobile** | Flutter/Dart, React Native |
| **Database** | Drizzle, Prisma, EF Core, TypeORM |
| **Monorepo** | Any combination of the above |
| **Single repo** | Any single project |

## Updating

### Update Mustard CLI

```bash
# Check if there's a new version
mustard auto-update --check-only

# Update to latest
mustard auto-update

# Or manually
npm install -g mustard-claude@latest
```

### Update Project Templates

After updating the CLI, update your project's `.claude/` files:

```bash
cd my-project
mustard update
```

This recreates core files (hooks, skills, scripts, commands) while preserving your customizations.

## Development

```bash
git clone https://github.com/rubensrpj/mustard.git
cd mustard
npm install
npm run build

# Run hook tests
node --test templates/hooks/__tests__/hooks.test.js

# Run script tests (metrics-report, etc.)
node --test templates/scripts/__tests__/

# Test locally
node bin/mustard.js init
```

## License

MIT
