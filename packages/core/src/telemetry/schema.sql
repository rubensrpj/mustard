-- Mustard telemetry store — SQLite schema for the dedicated `telemetry.db`.
--
-- This database is INDEPENDENT of `mustard.db` (the hot harness store the hooks
-- open on every tool use). Telemetry is high-volume and never load-bearing, so
-- it lives in its own file to keep `mustard.db` lean. Every CREATE uses
-- IF NOT EXISTS — the schema is applied on every open without harm.

-- usage_totals — aggregated Claude Code OTEL counters, REDUCED from the legacy
-- `claude_code_otel` table. The dropped columns (ts_bucket at minute
-- granularity, signal, token_type, attrs, count) had no read consumer; the
-- only aggregations performed are SUM(sum) (total / by model / by session),
-- the two operational metrics ('claude_code.session.count',
-- 'claude_code.active_time.total'), and MAX(updated_at) for freshness. The
-- composite primary key collapses the per-minute buckets into one row per
-- (metric, model, session_id).
CREATE TABLE IF NOT EXISTS usage_totals (
  metric TEXT NOT NULL,
  model TEXT,
  session_id TEXT,
  sum REAL DEFAULT 0,
  updated_at INTEGER,             -- ms epoch of the most recent contributing datapoint
  PRIMARY KEY (metric, model, session_id)
);

-- run_usage — per-execution token usage + cost, REPLACES the legacy `spans`
-- table. Carries over the spans columns the economy reader projects PLUS a new
-- `agent_id`; `spec` / `wave_id` / `agent_id` are now load-bearing (populated
-- at write time by the Wave 2 collector, backfilled here for history).
CREATE TABLE IF NOT EXISTS run_usage (
  trace_id TEXT,
  span_id TEXT PRIMARY KEY,
  parent_span_id TEXT,
  name TEXT,
  started_at INTEGER,             -- ms epoch
  ended_at INTEGER,
  duration_ms INTEGER,
  attributes TEXT,                -- JSON
  spec TEXT,
  phase TEXT,
  model TEXT,
  input_tokens INTEGER,
  output_tokens INTEGER,
  cache_read_input_tokens INTEGER,
  cache_creation_input_tokens INTEGER,
  cost_usd_micros INTEGER,
  is_error INTEGER,               -- bool 0/1
  project_path TEXT,
  ts_iso TEXT,
  session_id TEXT,
  wave_id TEXT,
  tool_use_id TEXT,
  agent_id TEXT                   -- new: load-bearing attribution column
);
CREATE INDEX IF NOT EXISTS idx_run_usage_spec ON run_usage(spec);
CREATE INDEX IF NOT EXISTS idx_run_usage_started ON run_usage(started_at);
-- W11: covers `WHERE session_id = ?` (telemetry/reader.rs `specs_for_session`,
-- `session_last_at` via the Wave-3 self-attributed path) — previously a full
-- scan of `run_usage` on every per-session card render on /economia.
CREATE INDEX IF NOT EXISTS idx_run_usage_session ON run_usage(session_id);
-- W11: covers `GROUP BY (spec, wave_id)` (telemetry/reader.rs `runs_by_wave`)
-- and `WHERE spec = ? AND wave_id = ?` filters in `per_wave_costs_v2`. The
-- composite lets SQLite skip the spec-only sub-index when both columns are
-- supplied and still serves the spec-only roll-up via the leading column.
CREATE INDEX IF NOT EXISTS idx_run_usage_spec_wave ON run_usage(spec, wave_id);
-- W11: covers `GROUP BY agent_id` (telemetry/reader.rs `runs_by_agent`) and
-- `WHERE agent_id = ?` filters used by the per-agent /economia table.
CREATE INDEX IF NOT EXISTS idx_run_usage_agent ON run_usage(agent_id);
-- W11: covers `GROUP BY model` (telemetry/reader.rs `runs_by_model`,
-- consumption-summary by-model breakdown). Cardinality is low (~5 distinct
-- models), so the index is small but turns the by-model scan into a covering
-- aggregate path.
CREATE INDEX IF NOT EXISTS idx_run_usage_model ON run_usage(model);
-- W11: covers `GROUP BY phase` (telemetry/reader.rs `tokens_by_phase`,
-- `runs_by_phase`) consumed by the dashboard Quality page.
CREATE INDEX IF NOT EXISTS idx_run_usage_phase ON run_usage(phase);

-- W11.T11.3: Deep-refactor economy tables (matches AC-W11.1 literals
-- `CREATE TABLE economy_baselines` and `CREATE TABLE economy_savings`).
-- The JSON-on-disk baselines in `.claude/.economy-baselines.json`
-- (W5.T5.15) capture _operation cost_; this pair captures _wave-level
-- token savings_ so the dashboard `/economia` page can render a real
-- per-wave breakdown without re-reading the JSON file or re-counting
-- events.
--
-- `economy_baselines` — one row per (operation, captured_at) tuple. Multiple
-- captures of the same operation accumulate so the reader can see the trend
-- (`SELECT ... ORDER BY captured_at`). Inserts are append-only; reconcile uses
-- the most recent row for each operation.
-- DDL: CREATE TABLE economy_baselines (idempotent guard via IF NOT EXISTS).
CREATE TABLE IF NOT EXISTS economy_baselines (
  operation TEXT NOT NULL,
  baseline_tokens INTEGER NOT NULL DEFAULT 0,
  captured_at INTEGER NOT NULL,         -- ms epoch
  PRIMARY KEY (operation, captured_at)
);

-- `economy_savings` — one row per (wave_id, operation, measured_at) tuple.
-- The dashboard groups by `wave_id` for the per-wave table and SUMs
-- `savings_tokens` for the headline card. `wave_id` is TEXT to match
-- `run_usage.wave_id` and the Mustard wave slug format (`W0`..`W12`,
-- `wave-N-{role}`, etc.).
-- DDL: CREATE TABLE economy_savings (idempotent guard via IF NOT EXISTS).
CREATE TABLE IF NOT EXISTS economy_savings (
  wave_id TEXT NOT NULL,
  operation TEXT NOT NULL,
  savings_tokens INTEGER NOT NULL DEFAULT 0,
  measured_at INTEGER NOT NULL,         -- ms epoch
  PRIMARY KEY (wave_id, operation, measured_at)
);
CREATE INDEX IF NOT EXISTS idx_economy_savings_wave ON economy_savings(wave_id);
CREATE INDEX IF NOT EXISTS idx_economy_savings_measured ON economy_savings(measured_at);

-- run_attribution — write-time stamp map. Lets the Wave 2 collector resolve the
-- spec / wave / agent for a tool_use the moment it records a run, instead of the
-- read-time JOIN against `events(agent.start)` the legacy reader performed.
CREATE TABLE IF NOT EXISTS run_attribution (
  session_id TEXT NOT NULL,
  tool_use_id TEXT NOT NULL,
  spec TEXT,
  wave_id TEXT,
  agent_id TEXT,
  updated_at INTEGER,             -- ms epoch
  PRIMARY KEY (session_id, tool_use_id)
);
