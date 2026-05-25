-- Mustard harness store — SQLite schema for `SqliteEventStore`.
-- Idempotent: every CREATE uses IF NOT EXISTS. Safe to run on every open().
--
-- ============================================================================
-- W5 (2026-05-24-mustard-unification) — lean schema redesign.
--
-- The hot-path event log moved to per-spec NDJSON files
-- (`.claude/spec/{name}/events/*.ndjson`) — see
-- `apps/rt/src/run/event_writer_ndjson.rs`. Run-cost telemetry lives in the
-- sibling `telemetry.db` (`run_usage`, `usage_totals`). What stays in
-- `mustard.db` is the **lifecycle index** + the long-lived domain tables.
--
-- Dropped vs the v9 schema (no migration data carry — per dev-phase guidance):
--
--   * `events` + `events_fts`          → replaced by NDJSON per-spec files
--   * `knowledge` + `knowledge_fts`    → consolidated into `knowledge_patterns`
--   * `metrics_projection`             → duplicated `telemetry.db.run_usage`
--   * `api_cost_frames`                → consolidated into `telemetry.db.run_usage`
--
-- Kept:
--
--   * `pipeline_events`        — low-volume lifecycle index (status/phase/task)
--   * `sessions`               — session registry (Claude Code session ids)
--   * `pipeline_amend_window`  — session-bound amendment windows
--   * `specs`                  — denormalized per-spec cache for fast lists
--   * `savings_records`        — one row per Mustard intervention (writer hot)
--   * `context_cost_frames`    — one row per agent dispatch (writer hot)
--   * `knowledge_patterns`     — extracted patterns (FTS5 mirror)
--   * `memory_decisions`       — architectural decisions journal (FTS5 mirror)
--   * `memory_lessons`         — lessons learned journal (FTS5 mirror)
--   * `agent_memory`           — agent self-memory (W8 owns logic, DDL here)
--   * `memory_feedback`        — feedback signals on agent_memory rows
--
-- ## Index audit
--
-- Each consumer query was traced through `EXPLAIN QUERY PLAN`-style review.
-- The final index list (matches each `CREATE INDEX` below):
--
--   pipeline_events:
--     idx_pipeline_events_spec          — list-by-spec scans
--     idx_pipeline_events_kind          — by-kind probes
--     idx_pipeline_events_spec_kind     — list-by-spec-and-kind (hot)
--     idx_pipeline_events_session_kind  — `last_pipeline_scope_for_session`
--     idx_pipeline_events_parent        — task-child recursion
--     idx_pipeline_events_ts            — chronological sort
--
--   sessions:
--     idx_sessions_last_activity        — recent-first listing
--     idx_sessions_status               — filter by status
--
--   pipeline_amend_window:
--     PK(spec_id, session_id)           — single-key lookup
--     idx_pipeline_amend_window_session_status
--
--   knowledge_patterns:
--     UNIQUE(pattern)                   — dedupe writes
--     idx_knowledge_patterns_pattern    — exact-match lookup
--     idx_knowledge_patterns_last_seen  — recent-first
--     idx_knowledge_patterns_confidence_last_seen — SessionStart inject rank
--     idx_knowledge_patterns_spec       — per-spec scope (W5/W8)
--
--   memory_decisions / memory_lessons:
--     idx_memory_{decisions,lessons}_at         — recent-first
--     idx_memory_{decisions,lessons}_spec       — per-spec scope (W5/W8)
--     idx_memory_{decisions,lessons}_status     — active-only filters
--
--   agent_memory:
--     idx_agent_memory_spec
--     idx_agent_memory_status_confidence — active+top-confidence inject rank
--     idx_agent_memory_session
--
--   memory_feedback:
--     idx_memory_feedback_memory_id
--     idx_memory_feedback_kind
--
-- ============================================================================

-- pipeline_events: low-volume lifecycle events the dashboard reads by spec.
-- `kind` is the canonical event name (`pipeline.scope`, `pipeline.status`,
-- `pipeline.phase`, `pipeline.wave.complete`, `pipeline.wave.failed`,
-- `pipeline.task.dispatch`, `pipeline.task.complete`, `pipeline.complete`,
-- and the `pipeline.economy.*` family). Tool / agent / qa events live in the
-- per-spec NDJSON files — never in this table.
CREATE TABLE IF NOT EXISTS pipeline_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts TEXT NOT NULL,
    session_id TEXT,
    spec TEXT,
    wave INTEGER,
    kind TEXT NOT NULL,
    parent_id INTEGER REFERENCES pipeline_events(id),
    payload TEXT
);
CREATE INDEX IF NOT EXISTS idx_pipeline_events_spec ON pipeline_events(spec);
CREATE INDEX IF NOT EXISTS idx_pipeline_events_kind ON pipeline_events(kind);
CREATE INDEX IF NOT EXISTS idx_pipeline_events_spec_kind
    ON pipeline_events(spec, kind);
CREATE INDEX IF NOT EXISTS idx_pipeline_events_session_kind
    ON pipeline_events(session_id, kind);
CREATE INDEX IF NOT EXISTS idx_pipeline_events_parent
    ON pipeline_events(parent_id);
CREATE INDEX IF NOT EXISTS idx_pipeline_events_ts ON pipeline_events(ts);

-- sessions: one row per Claude Code session the harness has seen. Backs the
-- Sessions sidebar in the dashboard for sessions that ran without ever opening
-- a spec (otherwise the spec dir is the canonical entry).
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    started_at TEXT NOT NULL,
    last_activity_at TEXT,
    last_spec TEXT,
    cwd TEXT,
    status TEXT NOT NULL DEFAULT 'open'
);
CREATE INDEX IF NOT EXISTS idx_sessions_last_activity
    ON sessions(last_activity_at DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);

-- specs: denormalized per-spec cache. Rebuildable from NDJSON via
-- `mustard-rt run rebuild-specs` (W5.T5.2). The `affected_files` column is
-- legacy and may be NULL on fresh writes — readers that need the file set
-- read it from NDJSON `pipeline.task.complete` events.
CREATE TABLE IF NOT EXISTS specs (
    name TEXT PRIMARY KEY,
    status TEXT,
    phase TEXT,
    started_at TEXT,
    completed_at TEXT,
    affected_files TEXT
);

-- pipeline_amend_window: session-bound amendment windows opened after a
-- pipeline closes. One row per (spec, session). Status transitions through
-- 'open' → 'amending' → terminal.
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

-- ============================================================================
-- Economy — savings_records + context_cost_frames.
--
-- These are written on the hot path by every Mustard intervention (bash_guard
-- rewrites, model_routing downgrades, recipe injections, …). They are NOT
-- run-cost telemetry (which lives in `telemetry.db.run_usage`) — they are
-- side-channel measurements of the value the harness creates, with native
-- spec/wave/agent scope columns so dashboards can drill down without a JOIN.
-- ============================================================================

-- savings_records: one row per intervention (rtk-rewrite, model-routing
-- downgrade, recipe injection, etc.). `payload` is a JSON object the adapter
-- fills in so per-source drill-downs do not lose context.
CREATE TABLE IF NOT EXISTS savings_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts INTEGER NOT NULL,
    source TEXT NOT NULL,
    tokens_saved INTEGER NOT NULL,
    model_target TEXT,
    project_path TEXT NOT NULL,
    spec_id TEXT,
    wave_id TEXT,
    agent_id TEXT,
    payload TEXT
);
CREATE INDEX IF NOT EXISTS idx_savings_records_project_ts
    ON savings_records(project_path, ts);
CREATE INDEX IF NOT EXISTS idx_savings_records_spec_ts
    ON savings_records(spec_id, ts);

-- context_cost_frames: one row per agent dispatch. Every `*_bytes` column is
-- optional; an adapter records what it has, the dashboard renders what it gets.
CREATE TABLE IF NOT EXISTS context_cost_frames (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts INTEGER NOT NULL,
    agent_id TEXT NOT NULL,
    wave_id TEXT,
    spec_id TEXT,
    project_path TEXT NOT NULL,
    prompt_size_bytes INTEGER,
    prefix_stable_bytes INTEGER,
    slice_bytes INTEGER,
    recipe_bytes INTEGER,
    wave_slice_bytes INTEGER,
    return_size_bytes INTEGER,
    retry_overhead_bytes INTEGER
);
CREATE INDEX IF NOT EXISTS idx_context_cost_frames_project_ts
    ON context_cost_frames(project_path, ts);
CREATE INDEX IF NOT EXISTS idx_context_cost_frames_agent_ts
    ON context_cost_frames(agent_id, ts);

-- ============================================================================
-- Knowledge & memory — long-lived domain tables.
--
-- `knowledge_patterns` is the single knowledge table (no more legacy
-- `knowledge`); patterns include all extracted insights with a per-spec scope.
-- `memory_decisions` / `memory_lessons` mirror that shape for architectural
-- decisions and lessons. Every table has an FTS5 external-content mirror
-- maintained by triggers.
-- ============================================================================

CREATE TABLE IF NOT EXISTS knowledge_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern TEXT NOT NULL UNIQUE,
    confidence REAL NOT NULL DEFAULT 0.0,
    count INTEGER NOT NULL DEFAULT 1,
    last_seen TEXT NOT NULL,
    source TEXT,
    created_at TEXT NOT NULL,
    spec TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    last_used TEXT
);
CREATE INDEX IF NOT EXISTS idx_knowledge_patterns_pattern ON knowledge_patterns(pattern);
CREATE INDEX IF NOT EXISTS idx_knowledge_patterns_last_seen ON knowledge_patterns(last_seen);
CREATE INDEX IF NOT EXISTS idx_knowledge_patterns_confidence_last_seen
    ON knowledge_patterns(confidence DESC, last_seen DESC);
CREATE INDEX IF NOT EXISTS idx_knowledge_patterns_spec ON knowledge_patterns(spec);

CREATE TABLE IF NOT EXISTS memory_decisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    content TEXT NOT NULL,
    source TEXT,
    context TEXT,
    at TEXT NOT NULL,
    spec TEXT,
    wave INTEGER,
    confidence REAL NOT NULL DEFAULT 0.5,
    status TEXT NOT NULL DEFAULT 'active',
    superseded_by INTEGER
);
CREATE INDEX IF NOT EXISTS idx_memory_decisions_at ON memory_decisions(at);
CREATE INDEX IF NOT EXISTS idx_memory_decisions_spec ON memory_decisions(spec);
CREATE INDEX IF NOT EXISTS idx_memory_decisions_status ON memory_decisions(status);

CREATE TABLE IF NOT EXISTS memory_lessons (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    content TEXT NOT NULL,
    source TEXT,
    context TEXT,
    at TEXT NOT NULL,
    spec TEXT,
    wave INTEGER,
    confidence REAL NOT NULL DEFAULT 0.5,
    status TEXT NOT NULL DEFAULT 'active',
    superseded_by INTEGER
);
CREATE INDEX IF NOT EXISTS idx_memory_lessons_at ON memory_lessons(at);
CREATE INDEX IF NOT EXISTS idx_memory_lessons_spec ON memory_lessons(spec);
CREATE INDEX IF NOT EXISTS idx_memory_lessons_status ON memory_lessons(status);

-- FTS5 virtual tables (external content, kept in sync by triggers below).
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
-- agent_memory — W5 ships the DDL only. W8 owns insert/read/feedback semantics
-- (`mustard-rt run memory`, agent-side write/verify, lazy-decay).
-- ============================================================================

CREATE TABLE IF NOT EXISTS agent_memory (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT,
    spec TEXT,
    wave INTEGER,
    role TEXT,
    summary TEXT NOT NULL,
    details TEXT,
    confidence REAL NOT NULL DEFAULT 0.5,
    status TEXT NOT NULL DEFAULT 'active',
    at TEXT NOT NULL,
    last_used TEXT
);
CREATE INDEX IF NOT EXISTS idx_agent_memory_spec ON agent_memory(spec);
CREATE INDEX IF NOT EXISTS idx_agent_memory_status_confidence
    ON agent_memory(status, confidence DESC);
CREATE INDEX IF NOT EXISTS idx_agent_memory_session ON agent_memory(session_id);

-- memory_feedback: append-only journal of feedback signals (bump / depreciate /
-- supersede / use) attributed to an agent_memory row. W8 owns the writer.
CREATE TABLE IF NOT EXISTS memory_feedback (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_id INTEGER NOT NULL REFERENCES agent_memory(id),
    kind TEXT NOT NULL,
    delta REAL,
    by_role TEXT,
    at TEXT NOT NULL,
    note TEXT
);
CREATE INDEX IF NOT EXISTS idx_memory_feedback_memory_id
    ON memory_feedback(memory_id);
CREATE INDEX IF NOT EXISTS idx_memory_feedback_kind
    ON memory_feedback(kind);
