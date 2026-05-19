-- Mustard 2.0 Phase 1 — SQLite schema for the EventStore.
-- Idempotent: every CREATE uses IF NOT EXISTS. Safe to run on every init().
-- Kept in sync with the SCHEMA_SQL literal in event-store.ts.

-- Append-only event log mirror of events.jsonl.
CREATE TABLE IF NOT EXISTS events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts TEXT NOT NULL,
  session_id TEXT,
  wave INTEGER,
  spec TEXT,
  event TEXT NOT NULL,
  actor_kind TEXT,
  actor_id TEXT,
  payload TEXT
);
CREATE INDEX IF NOT EXISTS idx_events_spec ON events(spec);
CREATE INDEX IF NOT EXISTS idx_events_event ON events(event);
CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts);

-- FTS5 virtual table — full-text search over events.
-- content='events' + content_rowid='id' = external content table; FTS stores
-- only the index, the trigger below populates it on insert.
CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(
  event, spec, payload_text,
  content='events', content_rowid='id'
);

-- Keep FTS in sync on insert.
CREATE TRIGGER IF NOT EXISTS events_ai AFTER INSERT ON events BEGIN
  INSERT INTO events_fts(rowid, event, spec, payload_text)
  VALUES (new.id, new.event, new.spec, new.payload);
END;

-- Denormalized projections (regenerable from events via rebuild()).
CREATE TABLE IF NOT EXISTS specs (
  name TEXT PRIMARY KEY,
  status TEXT,
  phase TEXT,
  started_at TEXT,
  completed_at TEXT,
  affected_files TEXT
);

CREATE TABLE IF NOT EXISTS metrics_projection (
  spec TEXT PRIMARY KEY,
  api_calls INTEGER,
  retries INTEGER,
  pass1 INTEGER,
  tool_breakdown TEXT,
  dispatch_failures_by_phase TEXT,
  agent_count INTEGER,
  updated_at TEXT,
  FOREIGN KEY (spec) REFERENCES specs(name)
);

CREATE TABLE IF NOT EXISTS knowledge (
  id TEXT PRIMARY KEY,
  type TEXT,
  name TEXT,
  description TEXT,
  confidence REAL,
  created_at TEXT,
  updated_at TEXT,
  source TEXT
);

-- knowledge_fts: standalone FTS5 (no external content). knowledge.id is TEXT,
-- FTS5 external-content requires INTEGER rowid; mixing them produces "database
-- disk image is malformed" on query (observed on Windows bun:sqlite). The
-- migration + EventStore.knowledge({search}) own rowid assignment and join
-- results back via the UNINDEXED `id` column.
CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_fts USING fts5(
  id UNINDEXED, name, description
);

-- Spans projection (legacy Phase 2). Kept idempotent — Phase 2's homegrown
-- span emitter was removed in favor of consuming Claude Code's native OTEL.
-- See claude_code_otel table (added Fase 6) for the live data path.
CREATE TABLE IF NOT EXISTS spans (
  trace_id TEXT,
  span_id TEXT PRIMARY KEY,
  parent_span_id TEXT,
  name TEXT,
  started_at INTEGER,  -- ms epoch
  ended_at INTEGER,
  duration_ms INTEGER,
  attributes TEXT,     -- JSON
  spec TEXT,
  phase TEXT,
  model TEXT,
  input_tokens INTEGER,
  output_tokens INTEGER,
  is_error INTEGER     -- bool
);
CREATE INDEX IF NOT EXISTS idx_spans_spec ON spans(spec);
CREATE INDEX IF NOT EXISTS idx_spans_phase ON spans(phase);
CREATE INDEX IF NOT EXISTS idx_spans_started ON spans(started_at);

-- Claude Code native OTEL projection. Populated by the local OTLP collector
-- (templates/scripts/otel-collector.js) that receives metrics/logs from the
-- Claude Code CLI when CLAUDE_CODE_ENABLE_TELEMETRY=1 is set. Rows are
-- aggregated per minute by (metric, session_id, model, token_type) — same
-- composite is the natural PK to keep cardinality bounded.
CREATE TABLE IF NOT EXISTS claude_code_otel (
  ts_bucket INTEGER NOT NULL,         -- ms epoch, floored to minute
  signal TEXT NOT NULL,               -- 'metric' | 'log'
  metric TEXT NOT NULL,               -- 'claude_code.token.usage', 'claude_code.cost.usage', etc.
  session_id TEXT,
  model TEXT,
  token_type TEXT,                    -- 'input' | 'output' | 'cacheRead' | 'cacheCreation' (only for token.usage); null otherwise
  sum REAL DEFAULT 0,                 -- aggregated value within the bucket
  count INTEGER DEFAULT 0,            -- number of datapoints summed
  attrs TEXT,                         -- JSON of remaining OTel attributes
  PRIMARY KEY (ts_bucket, metric, session_id, model, token_type)
);
CREATE INDEX IF NOT EXISTS idx_cco_metric ON claude_code_otel(metric);
CREATE INDEX IF NOT EXISTS idx_cco_session ON claude_code_otel(session_id);
CREATE INDEX IF NOT EXISTS idx_cco_bucket ON claude_code_otel(ts_bucket);
