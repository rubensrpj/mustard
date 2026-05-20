<!-- mustard:generated at:2026-05-19T00:00:00Z role:general -->
# Exports — mustard-core public API

All items below are `pub` from `mustard_core::`.

## model::contract

| Item | Kind | Summary |
|---|---|---|
| `Trigger` | enum | Lifecycle event (`PreToolUse`, `PostToolUse`, …8 variants). `from_event_name` / `as_event_name` |
| `HookInput` | struct | Lenient stdin JSON. Known fields typed; `raw: Value` captures unknowns |
| `Verdict` | enum | Hook decision: `Allow`, `Deny{reason}`, `Warn{message}`, `Rewrite{tool_input}`, `Inject{context}` |
| `Outcome` | struct | Consolidated result of N checks. `fold(verdict)` accumulates; `Deny` is sticky |
| `Ctx` | struct | Ambient context for a check: `project_dir`, `trigger` |
| `Check` | trait | `evaluate(&HookInput, &Ctx) -> Result<Verdict, Error>` |
| `Observer` | trait | `observe(&HookInput, &Ctx)` — telemetry only, never blocks |

## model::event

| Item | Kind | Summary |
|---|---|---|
| `HarnessEvent` | struct | One DB row: `v`, `ts`, `session_id`, `wave`, `actor`, `event`, `payload`, `spec` |
| `HookEvent` | type alias | Same as `HarnessEvent` |
| `Actor` | struct | `kind: ActorKind`, optional `id`, optional `actor_type` |
| `ActorKind` | enum | `Hook`, `Agent`, `Orchestrator`, `Cli` |
| `SCHEMA_VERSION` | const | `1` |

## model::pipeline

| Item | Kind | Summary |
|---|---|---|
| `PipelineState` | struct | Full state of one in-flight pipeline. Lenient (`raw` catch-all) |
| `Phase` | enum | `Analyze`, `Plan`, `Execute`, `Review`, `Qa`, `Close`, `Coordinate` |
| `Scope` | enum | `Light`, `Medium`, `Full` |
| `Task` | struct | `name`, `agent`, `status`, `steps` |

## error

| Item | Kind | Summary |
|---|---|---|
| `Error` | enum | `Io`, `NotFound(String)`, `Parse`, `Config`, `Env`, `InvalidInput`, `CheckFailed`, `Sqlite` |
| `Result<T>` | type alias | `std::result::Result<T, Error>` |
| `fail_open(result, fallback)` | fn | Return value or fallback, discarding error |
| `fail_open_with(result, fn)` | fn | Lazy fallback variant |

## config

| Item | Kind | Summary |
|---|---|---|
| `Mode` | enum | `Off`, `Warn`, `Strict`. Default = `Strict` |
| `EnforcementConfig` | struct | Per-check mode map + disabled set. Builder: `with_check`, `with_disabled` |
| `EnforcementConfig::resolve` | fn | Layers defaults → `mustard.json` → env. Fail-open |
| `env_var_name_for(check)` | fn | `"close-gate"` → `"MUSTARD_CLOSE_GATE_MODE"` |

## env

| Item | Kind | Summary |
|---|---|---|
| `Env` | trait | `get(&str) -> Option<String>`, `set(&str, &str)` |
| `ProcessEnv` | struct | Production impl over `std::env` + thread-local overlay |
| `MapEnv` | struct | In-memory impl for tests. Builder: `.with(k, v)` |
| `HookProfile` | enum | `Minimal`, `Standard`, `Strict` |
| `should_run` | fn | Consults profile + disabled list |
| `acquire_guard` | fn | Re-entrancy guard via env var |
| `check_depth` | fn | Depth counter; blocks at `max_depth` |
| `guarded_run` | fn | Combined: `should_run → acquire_guard → check_depth → !in_hook_phase` |
| `resolve_cwd` | fn | `HookInput.cwd` → `CLAUDE_PROJECT_DIR` → `MUSTARD_PROJECT_DIR` |
| `resolve_session_id` | fn | `input.session_id` → `MUSTARD_SESSION_ID` → `CLAUDE_SESSION_ID` |
| `DEFAULT_MAX_DEPTH` | const | `3` |

## metrics

| Item | Kind | Summary |
|---|---|---|
| `MetricLine` | struct | Pure value: `ts`, `event`, `tokens_affected`, `tokens_saved`, `note`, `extras`. Builder-style |
| `emit_metric(cwd, line)` | fn | Fail-silent: appends to `<cwd>/.claude/.metrics/<event>.jsonl`. Returns `bool` |
| `metric_file_path(cwd, event)` | fn | Path helper |

## knowledge

| Item | Kind | Summary |
|---|---|---|
| `ToolBreakdown` | struct | `bash`, `edit`, `write`, `agent` counts from JSON |
| `PipelineMetrics` | struct | `retries`, `api_calls`, `tool_breakdown` |
| `FrictionEntry` | struct | Named friction signal with tags and optional prescription |
| `KnowledgePattern` | struct | Named knowledge pattern (extension point) |
| `ContextItem` | struct | Candidate knowledge item: `id`, `kind`, `text`, `tags` |
| `SelectionRequest` | struct | `agent`, `phase` — who is asking for context |
| `ContextSelector` | trait | `select(request, candidates) -> Vec<ContextItem>` |
| `PassthroughSelector` | struct | Baseline: returns all candidates unchanged |
| `derive_prescription` | fn | 3 heuristics → advisory text or `None` |
| `extract_friction` | fn | States → `Vec<FrictionEntry>` |
| `extract_patterns` | fn | Extension point; returns empty `Vec` today |
| `select_context` | fn | Convenience wrapper over `ContextSelector::select` |

## io::event_store

| Item | Kind | Summary |
|---|---|---|
| `EventSink` | trait | `append(&HarnessEvent) -> Result<()>` |

## io::sqlite_store

| Item | Kind | Summary |
|---|---|---|
| `SqliteEventStore` | struct | WAL-mode SQLite impl of `EventSink`. Fail-open reads |
| `SpecRow` | struct | One row from the `specs` projection |
| `MetricsRow` | struct | One row from `metrics_projection` |

## io::pipeline_repo

| Item | Kind | Summary |
|---|---|---|
| `PipelineRepo` | trait | `read(spec_name)`, `write(spec_name, state)` |
| `FsPipelineRepo` | struct | FS-backed impl. `for_project(dir)` / `new(dir)` |
| `read_optional` | fn | Convenience: `NotFound` → `Ok(None)` |

## io::fs

| Item | Kind | Summary |
|---|---|---|
| `write_atomic(path, bytes)` | fn | Temp-file + rename — never torn |
| `append_line(path, line)` | fn | Append mode; creates parent dir if missing |
| `read_to_string(path)` | fn | `NotFound` distinct from `Io` |
| `exists(path)` | fn | `bool` |

Ref: `packages/core/src/lib.rs`
