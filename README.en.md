# Mustard

[Português](README.md) · **English**

> AI-assisted software development *harness* — enforces a disciplined, auditable, context-frugal pipeline on top of Claude Code.

**Mustard** wraps Claude Code and turns "ask the AI for a feature" into a **spec-driven pipeline** (Spec-Driven Development / SDD): named phases, blocking gates, and an auditable event trail. Discipline does not depend on the model's goodwill — the **machine enforces it** through hooks and gates.

The project's thesis is **minimum AI, maximum determinism**: everything statistics, graphs, or rules can solve lives in a Rust core; AI shows up only for orchestration and reasoning, never inside the engine.

---

## Core principle

> **Source code is never bulk-read.**

```mermaid
flowchart LR
    repo[("Repository")] -->|"census at the base gate (Rust, no AI)"| model[("grain.model.json")]
    model -->|digest| anchors["~12 anchors<br/>(anchor files)"]
    anchors -->|"AI reads only these"| work["feature/bugfix pipeline"]
```

1. The **census** mines the repository into a durable model (`grain.model.json`) — **deterministic, AI-free, language- and architecture-agnostic**: modules, declarations, dependency graph, roles, slices, contracts, and touchpoints. It is not a command: the **base gate** triggers it on its own whenever the census is stale and the tree is clean.
2. Pipeline commands consume that model through a **digest** (`mustard-rt run feature`, `scan spec`) and read only the ~12 anchors the digest points at.
3. Result: **context economy** — the digest finds *where to look*; it does not replace reading.

> The harness's real weight is not the commands but the **re-injection of ceremony into the context on every turn**. Routing therefore always picks the **cheapest path that serves** — the full pipeline is the exception that must justify itself (≥2 layers/subprojects **or** a new entity), never the default.

---

## Installation

Single prerequisite on every OS: **[Claude Code](https://docs.claude.com/claude-code)** installed and logged in (`claude --version` answers). You do **not** need Rust, Node, or any development tooling — the installers ship everything pre-compiled.

### Step 1 — your OS installer

On Windows and macOS, download **one** file from the [**Releases**](https://github.com/rubensrpj/mustard/releases) page (*Assets* section); on **Linux**, a single terminal line does it. Each installer carries the full CLI (`mustard`, `mustard-rt`, `mustard-mcp`, `scan`, `rtk`) **and** the **Mustard Dashboard**:

| OS | What to download | What to do |
|---|---|---|
| 🪟 **Windows** 10/11 | `Mustard Dashboard_<version>_x64-setup.exe` | Double-click. On the SmartScreen warning (the installer is unsigned): **"More info" → "Run anyway"**. When done, **open a new terminal** — PATH only applies to terminals opened after the install. |
| 🍎 **macOS** 11+ (Intel + Apple Silicon) | `Mustard-<version>-universal.pkg` | The package is unsigned: **right-click → Open** (Gatekeeper). Follow the wizard, then open a new terminal. |
| 🐧 **Linux** (Ubuntu 22.04+) | none — install in one line:<br>`curl -fsSL https://github.com/rubensrpj/mustard/releases/latest/download/install.sh \| sh` | The script downloads the `.deb` from the latest Release and hands it to `apt` (which resolves the dependencies). Manual route, for whoever wants to check the `sha256` first: download `mustard_<version>_amd64.deb` + `install.sh` into the same folder and run `chmod +x install.sh && ./install.sh` — Release assets arrive **without** the executable bit, and without the `chmod` the shell answers `Permission denied`. |

Verify in a fresh terminal:

```bash
mustard --version
mustard-rt --version
```

The complete walkthrough for each OS (including common issues and uninstall) ships as release *Assets*: `TUTORIAL-WINDOWS.md`, `TUTORIAL-MACOS.md`, `TUTORIAL-LINUX.md`.

### Step 2 — the Claude Code plugin

The harness (the `/mustard:*` commands, hooks, gates, agents, and the memory MCP server) is distributed as a **Claude Code plugin**:

```
/plugin marketplace add rubensrpj/mustard
/plugin install mustard@mustard-local
```

Restart (or reload) Claude Code so the hooks kick in. `add` registers the Mustard repository as a marketplace (it is the one carrying `.claude-plugin/marketplace.json`); the `@mustard-local` in `install` is the **marketplace name**, not a path. `add` also accepts the path of a local clone of this repository — the root containing `.claude-plugin/marketplace.json` — and the repository's full URL (`https://github.com/rubensrpj/mustard.git`), which is the form to use when the `owner/repo` shorthand cannot clone.

> **Automatic binaries:** the plugin ships no binaries in git. On the **first session**, the bootstrap (`mustard-boot`) downloads the `mustard-bins-<version>-<os>` package from the Release assets matching the plugin's version and installs it inside the plugin — silent and fail-open (no network → the session continues normally and it retries next time). If you also ran Step 1, the CLI is on your PATH anyway; both paths coexist.

### Step 3 — prepare a project

At the **root of your project's git repository** (`init` refuses subfolders of a repo — in a monorepo, everything lives at the root):

```bash
cd /path/to/your/project
mustard init
```

This creates `mustard.json` (the single configuration) and the `.claude/` folder (hooks, skills, templates). From there, **open Claude Code normally inside the project** and **describe the work in your own words** — there is no command to "get started", and no mapping step to run. The router is injected on every prompt and classifies the request on its own; the base gate mines the repository on the way in.

### For developers of this repository

```powershell
# Builds the binaries in release, installs them, and runs `mustard init` on the target:
.\install.ps1                  # target = current directory (with prompt)
.\install.ps1 -Target ..\app   # another project (no prompt)
```

---

## Canonical pipeline

```mermaid
flowchart LR
    A["ANALYZE"] --> P["PLAN"]
    P -->|/approve| E["EXECUTE"]
    E --> R["REVIEW"]
    R --> Q["QA"]
    Q -->|gate: pass| C["CLOSE"]
```

| Scope | Detection | Flow |
|---|---|---|
| **Light** | 1-2 layers, ≤5 files, known pattern | Skips PLAN: `ANALYZE → EXECUTE → REVIEW → QA → CLOSE` |
| **Full** | 3+ layers or a new entity | Complete, with **human approval** between PLAN and EXECUTE |

Every phase emits events; gates block progress. The **close-gate** refuses to close without a `qa.result` with `overall=pass`; editing the spec after an approved QA marks the pass *stale* and re-blocks until QA runs again.

---

## Commands

Installed as a plugin, every command lives under the `/mustard:` namespace.

### The single door is not a command

**Start by describing the work in natural language** — there is no entry command. The router is injected on every prompt: it classifies the request (feature / change / bugfix / investigation + scope), narrates how it read it, and dispatches the right flow. It asks only on genuine ambiguity.

### The four doors

There are **four**, and only four — what you type. Everything else is an internal flow the router dispatches.

| Command | Role |
|---|---|
| `/mustard:git` | Commit/push/sync/PR — reads the git flow from `mustard.json`. Always ships the complete work; reversible operations only. `delete <branch>` cancels an abandoned unit, removing branch, remote and PR in one gesture. |
| `/mustard:pr` | Lists, reviews and merges PRs. **Review, QA and close are steps here**, not commands: the merge crosses the gates (build+tests, QA, review spans, docs) and then prunes the unit. |
| `/mustard:spec` | Single picker — approves a planned spec or resumes one in progress. |
| `/mustard:upsert` | Installs/updates Mustard in the project. `--off` / `--on` turn the harness off and back on; `--doctor` diagnoses the installation. |

### Internal flows (the router picks)

| Flow | Role |
|---|---|
| census | Mines the repository into `grain.model.json` (deterministic, no AI) and enriches per-subproject maps (Guards + pattern molds). Triggered by the base gate. |
| `feature` | Full feature pipeline: understand, research via digest, plan, implement. |
| `bugfix` | Autonomous diagnosis + fix. Fast path (1-2 files) or full path (lean spec). |
| `tactical-fix` | Creates a sub-spec linked to a parent, preserving SDD purity. |
| `task` | Spec-less work delegation (analyze, audit, refactor, docs…). |

---

## Dashboard

The **Mustard Dashboard** is the desktop telemetry app (Tauri + React) for the harness: it reads the NDJSON events the hooks write under each project's `.claude/`, **straight from disk and live** — no server, no database, no open session required.

### Opening it

| OS | How |
|---|---|
| Windows | Start Menu → **"Mustard Dashboard"** |
| macOS | Launchpad / **Applications** folder → **"Mustard Dashboard"** |
| Linux | Application menu → **"Mustard Dashboard"** |

### First use

1. Open **Settings** in the sidebar.
2. Point the **projects root folder** — the directory containing your repositories (e.g. `C:\Atiz` or `~/code`).
3. The dashboard **auto-discovers** every Mustard-initialized project (`mustard.json` + `.claude/`) inside it.

### What each area shows

| Area | Content |
|---|---|
| **Workspace** | Aggregated overview of all discovered projects: active pipelines, latest events, health. |
| **Activity** | The **live** execution: running pipeline, waves, dispatched agents, and the trace grouped by agent/wave. |
| **Specs** | Every specification with its lifecycle state (active, suspicious, closed), acceptance criteria, and waves. |
| **Economy** | Token metrics: per-session/per-spec consumption and the savings obtained (rtk, digest, routing). |
| **Knowledge** | The project's knowledge base (patterns, conventions, recorded decisions). |
| **Commands** | History of executed pipeline commands. |
| **Sessions** | Claude Code session history for the project, with per-session drill-down. |
| **Project detail** | Per project: specs, execution trace, and the live pipeline card. |

> Tip: keep the dashboard open on a second monitor while Claude Code works — **Activity** shows each wave and agent in real time, and **Specs** reflects the gates (QA passed, CLOSE blocked, etc.) the moment they happen.

---

## Spec-Driven Development

Specs live in a **flat** layout under `.claude/spec/{name}/`:

- **`spec.md`** — pure narrative (no lifecycle metadata).
- **`meta.json`** — single source of truth for the lifecycle (`stage` + `outcome` + `flags`). There are no `active/`, `completed/`, or `superseded/` folders: archiving is semantic (a `pipeline.status` event), not a filesystem move.
- **`wave-plan.md`** + `wave-N-{role}/spec.md` — for full scope (one sub-spec per wave).

Mid-flight changes are auto-recorded (`change-requests.ndjson` + a readable `change-log.md`) — nothing is lost, and the frozen narrative is never touched.

---

## Architecture (monorepo)

| Path | Crate/App | Stack | Role |
|---|---|---|---|
| `apps/rt` | `mustard-rt` | Rust | **Deterministic core** — scan-digest, events, gates, hooks, pipeline commands. The engine. |
| `apps/scan` | `scan` | Rust | Repository miner → `grain.model.json`. |
| `apps/cli` | `mustard` | Rust | Install & scaffold — `init`, grammars, git-flow, fonts. |
| `apps/mcp` | `mustard-mcp` | Rust | MCP server (harness memory/queries). |
| `packages/core` | `core` | Rust | Shared types and logic (e.g. `ProjectConfig`). |
| `apps/dashboard` | `mustard-dashboard` | Tauri + React | Telemetry UI (specs, runs, trace, metrics). Reads NDJSON; outside the Cargo workspace. |
| `plugin/` | — | — | The Claude Code plugin: commands, hooks, agents, MCP, and the `mustard-boot` bootstrap (downloads the binaries from the Release on the first session). |

`cargo build --workspace` covers the Rust crates; the dashboard builds via `pnpm`.

---

## Build & tests

```bash
# Rust (workspace)
cargo build --workspace            # or: pnpm build:rust
cargo test  --workspace            # or: pnpm test:rust
cargo clippy --workspace           # lint

# Dashboard (Tauri + React)
pnpm dashboard:dev                 # dev with HMR
pnpm dashboard:build               # production build

# Everything
pnpm build                         # Rust workspace + dashboard
pnpm test                          # same
```

**Official release:** a `vX.Y.Z` tag triggers the workflow that builds one complete installer per OS + the `mustard-bins-*` packages (consumed by the plugin bootstrap) and publishes everything as a GitHub Release. The tag version **must** match `plugin/.claude-plugin/plugin.json` — the workflow refuses a desynchronized tag. Manual dispatch (Actions → Release → Run workflow) is a **rehearsal**: builds everything without publishing.

---

## Configuration

`mustard.json` at the root is the project's **single source** of configuration:

```jsonc
{
  "git":  { "flow": { "*": "dev", "dev": "main" }, "provider": "github" },
  "buildCommand": "cargo build",
  "testCommand":  "cargo test",
  "lintCommand":  "cargo clippy",
  "typeCheckCommand": "cargo check",
  "specLang": "en-US",      // language of generated artifacts
  "tone":     "didactic"    // tone of generated prose
}
```

Mustard is language- and architecture-**agnostic**: generated output follows `specLang` + `tone`; build/test/lint commands are read from here. Monorepo rule: all state lives at the git repository **root**; a subproject is its own Mustard project only when it is an independent git repository (submodule).

---

## Repository layout

```
apps/
  rt/         mustard-rt — deterministic core (Rust)
  scan/       repository miner (Rust)
  cli/        mustard — installer/scaffold (Rust)
  mcp/        MCP server (Rust)
  dashboard/  Tauri + React — telemetry
packages/
  core/       shared types/logic (Rust)
plugin/       Claude Code plugin (commands, hooks, agents, bootstrap)
packaging/    Win/macOS/Linux installers + tutorials
docs/         architecture analyses and redesigns
.claude/      harness config (hooks, skills, refs, specs, grain.model.json)
install.ps1   development installer (build + scaffold)
mustard.json  project configuration
```

---

## Documentation

- **[MUSTARD-COMMANDS.md](MUSTARD-COMMANDS.md)** — visual reference for each command and its flow (Mermaid diagrams).
- **Install tutorials** — `packaging/installer/TUTORIAL-{WINDOWS,MACOS,LINUX}.md` (also attached to every release).
- **[docs/](docs/)** — architecture redesigns (agnostic index/digest, multi-signal stack detection, plugin validation).

---

*Distributed under the MIT license.*
