<!-- mustard:generated at:2026-05-19T00:00:00.000Z role:general -->
# Modules: mustard-rt

The binary has four top-level faces and three subsystems.

## Faces (entry points via CLI subcommand)

| Face | CLI | Stdin? | Purpose |
|---|---|---|---|
| `on <event>` | `mustard-rt on PreToolUse` | Yes — harness JSON | Run all modules for a lifecycle event |
| `check <id>` | `mustard-rt check bash_guard` | Yes — harness JSON | Run a single named module |
| `run <subcommand>` | `mustard-rt run sync-detect` | No | Port of a standalone bun/JS script |
| `mcp` | `mustard-rt mcp` | Owned (JSON-RPC) | `mustard-memory` MCP server |

## Enforcement modules (hooks)

Registered in `src/registry.rs`. Keyed by `(Trigger, ToolMatch)`.

| Module id | File | Trigger(s) | Tool(s) | Check / Observer |
|---|---|---|---|---|
| `bash_guard` | `hooks/bash_guard.rs` | Pre+PostToolUse | Bash | Both |
| `budget` | `hooks/budget.rs` | Pre+PostToolUse | Task, Agent | Check |
| `model_routing` | `hooks/model_routing.rs` | PreToolUse | Task, Agent | Check |
| `tool_use_counter` | `hooks/tracker.rs` | Pre+Sub lifecycle | Any | Check |
| `main_context_counter` | `hooks/tracker.rs` | Pre+Sub lifecycle | Any | Check |
| `subagent_tracker` | `hooks/tracker.rs` | Pre+PostToolUse | Task, Agent | Observer |
| `metrics_tracker` | `hooks/tracker.rs` | PostToolUse | Bash/Write/Edit/Task/Agent/Read | Observer |
| `skill_usage_tracker` | `hooks/tracker.rs` | PostToolUse | Skill | Observer |
| `skills_audit` | `hooks/skills_audit.rs` | PreToolUse | Task, Agent | Check |
| `size_gate` | `hooks/size_gate.rs` | PreToolUse | Write, Edit | Check |
| `path_guard` | `hooks/path_guard.rs` | PreToolUse | Read/Write/Edit | Check |
| `close_gate` | `hooks/close_gate.rs` | PreToolUse | Write, Edit | Check |
| `enforce_registry` | `hooks/enforce_registry.rs` | PreToolUse | Skill | Check |
| `post_edit` | `hooks/post_edit.rs` | PostToolUse | Write, Edit | Both |
| `session_start` | `hooks/session_start.rs` | SessionStart | Any | Check |
| `knowledge` | `hooks/knowledge.rs` | SessionEnd + PostToolUse | Any / Task/Agent | Observer |
| `session_cleanup` | `hooks/session_cleanup.rs` | SessionEnd | Any | Observer |
| `pre_compact` | `hooks/pre_compact.rs` | PreCompact | Any | Check |
| `prompt_gate` | `hooks/prompt_gate.rs` | UserPromptSubmit | Any | Check |

## Run subcommands (ported scripts)

Ref: `src/run/mod.rs` — `RunCmd` enum.

| Subcommand | Purpose |
|---|---|
| `sync-detect` | Discover subprojects; emit SHA-256 change-detection JSON |
| `sync-registry` | Scan entities/clusters/conventions; write `entity-registry.json` v4 |
| `diff-context` | Compact git diff for agent context |
| `emit-event` | Append an arbitrary named event to the harness bus |
| `emit-phase` | Record a `pipeline.phase` transition |
| `complete-spec` | Finalize / archive a pipeline spec |
| `context-slice` | Cut term blocks from `CONTEXT.md` glossaries |
| `memory` | Persist agent memory, decisions, or knowledge entries |
| `epic-fold` | Detect / fold a completed epic |
| `spec-extract` | Cut a wave slice or AC block from a spec |
| `spec-link` | Link a child spec to a parent epic |
| `analyze-validation` | Validate a spec's structure (warn-only) |
| `mark-checklist-item` | Mark a checklist item done in a spec |
| `wave-tree` | Render a spec's wave structure as ASCII or JSON |
| `wave-dependency` | Analyze file dependencies across waves |
| `scope-decompose` | Suggest wave decomposition by count |
| `exec-rewave-check` | Check whether a spec needs decomposition at EXECUTE |
| `wave-size-check` | Audit per-wave file/layer counts |
| `recipe-match` | Match entity+operation to a code recipe skeleton |
| `qa-run` | Execute spec Acceptance Criteria; emit `qa.result` |
| `metrics` | Pipeline + hook telemetry (collect / report) |
| `event-projections` | Query harness event log by view |
| `verify-pipeline` | Build/test verification for active pipeline |
| `pipeline-summary` | CLOSE-phase Done/Left/Next-Steps summary |
| `review-result` | Record REVIEW-phase verdict |
| `statusline` | Render Claude Code status bar |
| `skills` | Skill-family CLI (validate / graph / orphans) |
| `security-scan` | Scan for committed secrets + misconfigurations |
| `verify-emit` | Confirm a named harness event landed in recent window |
| `rtk-gain` | Normalise `rtk gain` analytics |
| `scan-orchestrate` | Pre-dispatch orchestration for `/scan` |
| `scan-finalize` | Post-dispatch finalization (registry + skills + security) |
| `otel-collector` | Local OTLP/JSON receiver for Claude Code native telemetry |
| `diagnose-otel` | End-to-end health check of the OTEL pipeline |

## MCP tools (mcp face)

| Tool | Description |
|---|---|
| `search_knowledge` | FTS5 search over the `knowledge` table |
| `query_events` | Filter event log by spec / event / since |
| `find_similar_specs` | Rank specs by token overlap on a description |
| `get_spec_metrics` | Metrics projection row for a spec |
| `get_span_summary` | Aggregated token/duration totals from `spans` |
