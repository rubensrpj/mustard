-- Mustard harness store — SQLite schema for `SqliteEventStore`.
-- Idempotent: every CREATE uses IF NOT EXISTS. Safe to run on every open().
-- Sourced verbatim from the legacy TypeScript store
-- (`apps/cli/src/runtime/schema.sql`); kept in-crate so the schema travels
-- with `mustard-core` and is embedded via `include_str!`. The TS source file
-- is deleted in a later wave of the eliminate-bun pipeline.

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
CREATE INDEX IF NOT EXISTS idx_events_spec ON events(spec); -- used by pipeline_state_for_spec (Wave 2)
CREATE INDEX IF NOT EXISTS idx_events_event ON events(event);
CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts);
-- Session-scoped lookups (last_pipeline_scope_for_session, amend windows by session).
CREATE INDEX IF NOT EXISTS idx_events_session_id ON events(session_id);

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

-- Keep FTS in sync on delete (external-content FTS5 needs explicit removal —
-- see prune_events_older_than). Same external-content 'delete' form as
-- knowledge_patterns_ad; column list matches events_ai exactly.
CREATE TRIGGER IF NOT EXISTS events_ad AFTER DELETE ON events BEGIN
  INSERT INTO events_fts(events_fts, rowid, event, spec, payload_text)
  VALUES ('delete', old.id, old.event, old.spec, old.payload);
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
-- migration + SqliteEventStore.search() own rowid assignment and join results
-- back via the UNINDEXED `id` column.
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
-- (`mustard-rt run otel-collector`) that receives metrics/logs from the
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

-- ============================================================================
-- Wave 6a — knowledge + memory tables (2026-05-20)
-- Backing store for knowledge.json (knowledge_patterns) and
-- memory/decisions.json + memory/lessons.json migration.
-- All CREATEs use IF NOT EXISTS — idempotent on every open().
-- ============================================================================

-- knowledge_patterns: structured patterns extracted from sessions, replacing
-- the flat-array knowledge.json. Distinct from the legacy `knowledge` table
-- (which stores named/typed knowledge entries from the JS harness); this table
-- holds confidence-scored patterns with a last-seen timestamp.
CREATE TABLE IF NOT EXISTS knowledge_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern TEXT NOT NULL UNIQUE,
    confidence REAL NOT NULL DEFAULT 0.0,
    count INTEGER NOT NULL DEFAULT 1,
    last_seen TEXT NOT NULL,
    source TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_knowledge_patterns_pattern ON knowledge_patterns(pattern);
CREATE INDEX IF NOT EXISTS idx_knowledge_patterns_last_seen ON knowledge_patterns(last_seen);
-- Composite for the SessionStart rank: ORDER BY confidence DESC, last_seen DESC.
CREATE INDEX IF NOT EXISTS idx_knowledge_patterns_confidence_last_seen
    ON knowledge_patterns(confidence DESC, last_seen DESC);

-- memory_decisions: persistent architectural decisions (replaces
-- memory/decisions.json). One row per decision entry.
CREATE TABLE IF NOT EXISTS memory_decisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    content TEXT NOT NULL,
    source TEXT,
    context TEXT,
    at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_memory_decisions_at ON memory_decisions(at);

-- memory_lessons: lessons learned entries (replaces memory/lessons.json).
CREATE TABLE IF NOT EXISTS memory_lessons (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    content TEXT NOT NULL,
    source TEXT,
    context TEXT,
    at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_memory_lessons_at ON memory_lessons(at);

-- FTS5 virtual tables (external content, kept in sync by triggers below).
-- Named `knowledge_patterns_fts` to avoid collision with the standalone
-- `knowledge_fts` table above (which serves the legacy `knowledge` table).
CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_patterns_fts USING fts5(
    pattern, source,
    content='knowledge_patterns',
    content_rowid='id',
    tokenize='unicode61'
);

CREATE VIRTUAL TABLE IF NOT EXISTS memory_decisions_fts USING fts5(
    content, source, context,
    content='memory_decisions',
    content_rowid='id',
    tokenize='unicode61'
);

CREATE VIRTUAL TABLE IF NOT EXISTS memory_lessons_fts USING fts5(
    content, source, context,
    content='memory_lessons',
    content_rowid='id',
    tokenize='unicode61'
);

-- Triggers: keep FTS5 external-content tables in sync (INSERT/UPDATE/DELETE).
-- knowledge_patterns <-> knowledge_patterns_fts (9 triggers total across 3 tables)
CREATE TRIGGER IF NOT EXISTS knowledge_patterns_ai AFTER INSERT ON knowledge_patterns BEGIN
    INSERT INTO knowledge_patterns_fts(rowid, pattern, source)
    VALUES (new.id, new.pattern, new.source);
END;

CREATE TRIGGER IF NOT EXISTS knowledge_patterns_ad AFTER DELETE ON knowledge_patterns BEGIN
    INSERT INTO knowledge_patterns_fts(knowledge_patterns_fts, rowid, pattern, source)
    VALUES ('delete', old.id, old.pattern, old.source);
END;

CREATE TRIGGER IF NOT EXISTS knowledge_patterns_au AFTER UPDATE ON knowledge_patterns BEGIN
    INSERT INTO knowledge_patterns_fts(knowledge_patterns_fts, rowid, pattern, source)
    VALUES ('delete', old.id, old.pattern, old.source);
    INSERT INTO knowledge_patterns_fts(rowid, pattern, source)
    VALUES (new.id, new.pattern, new.source);
END;

-- memory_decisions <-> memory_decisions_fts
CREATE TRIGGER IF NOT EXISTS memory_decisions_ai AFTER INSERT ON memory_decisions BEGIN
    INSERT INTO memory_decisions_fts(rowid, content, source, context)
    VALUES (new.id, new.content, new.source, new.context);
END;

CREATE TRIGGER IF NOT EXISTS memory_decisions_ad AFTER DELETE ON memory_decisions BEGIN
    INSERT INTO memory_decisions_fts(memory_decisions_fts, rowid, content, source, context)
    VALUES ('delete', old.id, old.content, old.source, old.context);
END;

CREATE TRIGGER IF NOT EXISTS memory_decisions_au AFTER UPDATE ON memory_decisions BEGIN
    INSERT INTO memory_decisions_fts(memory_decisions_fts, rowid, content, source, context)
    VALUES ('delete', old.id, old.content, old.source, old.context);
    INSERT INTO memory_decisions_fts(rowid, content, source, context)
    VALUES (new.id, new.content, new.source, new.context);
END;

-- memory_lessons <-> memory_lessons_fts
CREATE TRIGGER IF NOT EXISTS memory_lessons_ai AFTER INSERT ON memory_lessons BEGIN
    INSERT INTO memory_lessons_fts(rowid, content, source, context)
    VALUES (new.id, new.content, new.source, new.context);
END;

CREATE TRIGGER IF NOT EXISTS memory_lessons_ad AFTER DELETE ON memory_lessons BEGIN
    INSERT INTO memory_lessons_fts(memory_lessons_fts, rowid, content, source, context)
    VALUES ('delete', old.id, old.content, old.source, old.context);
END;

CREATE TRIGGER IF NOT EXISTS memory_lessons_au AFTER UPDATE ON memory_lessons BEGIN
    INSERT INTO memory_lessons_fts(memory_lessons_fts, rowid, content, source, context)
    VALUES ('delete', old.id, old.content, old.source, old.context);
    INSERT INTO memory_lessons_fts(rowid, content, source, context)
    VALUES (new.id, new.content, new.source, new.context);
END;

-- ============================================================================
-- Wave 7 — session-bound amendment window (2026-05-20)
-- Tracks in-progress amend sessions opened after a pipeline closes.
-- ============================================================================

CREATE TABLE IF NOT EXISTS pipeline_amend_window (
    spec_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    closed_at TEXT NOT NULL,
    pipeline_file_set TEXT NOT NULL,
    subprojects TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',
    last_activity_at TEXT,
    build_verde_at TEXT,
    drift_unrelated_paths TEXT NOT NULL DEFAULT '[]',
    drift_emitted INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (spec_id, session_id)
);
CREATE INDEX IF NOT EXISTS idx_pipeline_amend_window_session_status
    ON pipeline_amend_window(session_id, status);
