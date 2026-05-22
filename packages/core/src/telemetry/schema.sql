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
