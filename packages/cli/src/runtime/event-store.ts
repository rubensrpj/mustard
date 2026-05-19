/**
 * EventStore — Mustard 2.0 Phase 1 SQLite-backed event store.
 *
 * Wraps the .claude/.harness/mustard.db database that mirrors events.jsonl
 * into structured tables + FTS5 indexes + denormalized projections (specs,
 * metrics, knowledge). All reads/writes go through this class; hooks and
 * scripts never touch the database directly.
 *
 * Runtime loading: this module only runs at use time, never at import. The
 * SQLite driver is resolved lazily inside `init()` via the runtime-shim from
 * Phase 0 (templates/hooks/_lib/runtime-shim.js). Under Bun the shim returns
 * the `bun:sqlite` Database constructor; under Node it returns `null` and
 * `init()` throws a clear error — Node fallback (better-sqlite3) is a later
 * phase.
 *
 * Build/consume contract: this TS file compiles to dist/runtime/event-store.js
 * (ESM per tsconfig). Hooks consume it via Node's `require(esm)` support
 * (Node 22+). The codebase has `"type": "module"` so the .js output is
 * implicitly ESM — no .mjs renaming needed.
 *
 * Schema is embedded as a template literal to avoid runtime fs lookups and
 * to keep the schema in lock-step with the class. The mirror file
 * `src/runtime/schema.sql` exists for human reference (kept in sync manually).
 */

import { createRequire } from 'node:module';
import * as path from 'node:path';
import * as url from 'node:url';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface EventRecord {
  ts: string;
  sessionId?: string;
  wave?: number;
  spec?: string;
  event: string;
  actor?: { kind: string; id?: string };
  payload?: Record<string, unknown>;
}

export interface SpecRecord {
  name: string;
  status: string;
  phase: string;
  startedAt?: string;
  completedAt?: string;
  affectedFiles?: string[];
}

export interface MetricsRecord {
  spec: string;
  apiCalls: number;
  retries: number;
  pass1: boolean;
  toolBreakdown: Record<string, number>;
  dispatchFailuresByPhase: Record<string, number>;
  agentCount: number;
  updatedAt: string;
}

export interface KnowledgeRecord {
  id: string;
  type: string;
  name: string;
  description: string;
  confidence: number;
  createdAt: string;
  updatedAt: string;
  source: string;
}

export interface SpanRecord {
  traceId: string;
  spanId: string;
  parentSpanId?: string;
  name: string;
  startedAt: number;
  endedAt: number;
  durationMs: number;
  attributes: Record<string, unknown>;
  spec?: string;
  phase?: string;
  model?: string;
  inputTokens: number;
  outputTokens: number;
  isError: boolean;
}

export interface QueryFilter {
  spec?: string;
  event?: string;
  since?: string;
}

export interface SpanFilter {
  spec?: string;
  phase?: string;
  /** ms epoch lower bound (inclusive). */
  since?: number;
  /** default 1000. */
  limit?: number;
}

// ---------------------------------------------------------------------------
// Schema (kept in sync with src/runtime/schema.sql)
// ---------------------------------------------------------------------------

const SCHEMA_SQL = `
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

CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(
  event, spec, payload_text,
  content='events', content_rowid='id'
);

CREATE TRIGGER IF NOT EXISTS events_ai AFTER INSERT ON events BEGIN
  INSERT INTO events_fts(rowid, event, spec, payload_text)
  VALUES (new.id, new.event, new.spec, new.payload);
END;

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
-- disk image is malformed" on query in some SQLite builds (observed on Windows
-- bun:sqlite). We use standalone FTS with an UNINDEXED id column so the
-- migration owns rowid assignment and search results can join back via id.
CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_fts USING fts5(
  id UNINDEXED, name, description
);

CREATE TABLE IF NOT EXISTS spans (
  trace_id TEXT,
  span_id TEXT PRIMARY KEY,
  parent_span_id TEXT,
  name TEXT,
  started_at INTEGER,
  ended_at INTEGER,
  duration_ms INTEGER,
  attributes TEXT,
  spec TEXT,
  phase TEXT,
  model TEXT,
  input_tokens INTEGER,
  output_tokens INTEGER,
  is_error INTEGER
);
CREATE INDEX IF NOT EXISTS idx_spans_spec ON spans(spec);
CREATE INDEX IF NOT EXISTS idx_spans_phase ON spans(phase);
CREATE INDEX IF NOT EXISTS idx_spans_started ON spans(started_at);

CREATE TABLE IF NOT EXISTS claude_code_otel (
  ts_bucket INTEGER NOT NULL,
  signal TEXT NOT NULL,
  metric TEXT NOT NULL,
  session_id TEXT,
  model TEXT,
  token_type TEXT,
  sum REAL DEFAULT 0,
  count INTEGER DEFAULT 0,
  attrs TEXT,
  PRIMARY KEY (ts_bucket, metric, session_id, model, token_type)
);
CREATE INDEX IF NOT EXISTS idx_cco_metric ON claude_code_otel(metric);
CREATE INDEX IF NOT EXISTS idx_cco_session ON claude_code_otel(session_id);
CREATE INDEX IF NOT EXISTS idx_cco_bucket ON claude_code_otel(ts_bucket);
`;

// ---------------------------------------------------------------------------
// Minimal structural typing for the SQLite driver returned by runtime-shim.
// We avoid importing `bun:sqlite` types here so the file builds under plain
// `tsc` with only `@types/node` available.
// ---------------------------------------------------------------------------

interface SqliteStatement {
  run(...params: unknown[]): unknown;
  all(...params: unknown[]): unknown[];
  get(...params: unknown[]): unknown;
}

interface SqliteDatabase {
  exec(sql: string): unknown;
  prepare(sql: string): SqliteStatement;
  close(): void;
}

type SqliteCtor = new (filename: string) => SqliteDatabase;

interface RuntimeShim {
  loadSqlite(): SqliteCtor | null;
}

// Resolve runtime-shim relative to the compiled dist location:
//   dist/runtime/event-store.js  →  templates/hooks/_lib/runtime-shim.js
// In dev (src/) the path math is symmetric. createRequire(import.meta.url)
// keeps this working under both ESM (Node) and Bun.
function loadRuntimeShim(): RuntimeShim {
  const here = url.fileURLToPath(import.meta.url);
  // dist/runtime/event-store.js → repo root is two levels up.
  const repoRoot = path.resolve(path.dirname(here), '..', '..');
  const shimPath = path.join(
    repoRoot,
    'templates',
    'hooks',
    '_lib',
    'runtime-shim.js'
  );
  const req = createRequire(import.meta.url);
  return req(shimPath) as RuntimeShim;
}

// ---------------------------------------------------------------------------
// Row shapes for SELECT results — narrow at the boundary, expose typed records.
// ---------------------------------------------------------------------------

interface EventRow {
  id: number;
  ts: string;
  session_id: string | null;
  wave: number | null;
  spec: string | null;
  event: string;
  actor_kind: string | null;
  actor_id: string | null;
  payload: string | null;
}

interface SpecRow {
  name: string;
  status: string | null;
  phase: string | null;
  started_at: string | null;
  completed_at: string | null;
  affected_files: string | null;
}

interface MetricsRow {
  spec: string;
  api_calls: number | null;
  retries: number | null;
  pass1: number | null;
  tool_breakdown: string | null;
  dispatch_failures_by_phase: string | null;
  agent_count: number | null;
  updated_at: string | null;
}

interface KnowledgeRow {
  id: string;
  type: string | null;
  name: string | null;
  description: string | null;
  confidence: number | null;
  created_at: string | null;
  updated_at: string | null;
  source: string | null;
}

interface SpanRow {
  trace_id: string | null;
  span_id: string;
  parent_span_id: string | null;
  name: string | null;
  started_at: number | null;
  ended_at: number | null;
  duration_ms: number | null;
  attributes: string | null;
  spec: string | null;
  phase: string | null;
  model: string | null;
  input_tokens: number | null;
  output_tokens: number | null;
  is_error: number | null;
}

function safeJsonParse<T>(text: string | null | undefined, fallback: T): T {
  if (!text) return fallback;
  try {
    return JSON.parse(text) as T;
  } catch {
    return fallback;
  }
}

function rowToEvent(row: EventRow): EventRecord {
  const ev: EventRecord = {
    ts: row.ts,
    event: row.event,
    payload: safeJsonParse<Record<string, unknown>>(row.payload, {}),
  };
  if (row.session_id !== null) ev.sessionId = row.session_id;
  if (row.wave !== null) ev.wave = row.wave;
  if (row.spec !== null) ev.spec = row.spec;
  if (row.actor_kind !== null) {
    ev.actor = { kind: row.actor_kind };
    if (row.actor_id !== null) ev.actor.id = row.actor_id;
  }
  return ev;
}

function rowToSpec(row: SpecRow): SpecRecord {
  const spec: SpecRecord = {
    name: row.name,
    status: row.status ?? '',
    phase: row.phase ?? '',
  };
  if (row.started_at !== null) spec.startedAt = row.started_at;
  if (row.completed_at !== null) spec.completedAt = row.completed_at;
  if (row.affected_files !== null) {
    spec.affectedFiles = safeJsonParse<string[]>(row.affected_files, []);
  }
  return spec;
}

function rowToMetrics(row: MetricsRow): MetricsRecord {
  return {
    spec: row.spec,
    apiCalls: row.api_calls ?? 0,
    retries: row.retries ?? 0,
    pass1: !!row.pass1,
    toolBreakdown: safeJsonParse<Record<string, number>>(row.tool_breakdown, {}),
    dispatchFailuresByPhase: safeJsonParse<Record<string, number>>(
      row.dispatch_failures_by_phase,
      {}
    ),
    agentCount: row.agent_count ?? 0,
    updatedAt: row.updated_at ?? '',
  };
}

function rowToSpan(row: SpanRow): SpanRecord {
  const rec: SpanRecord = {
    traceId: row.trace_id ?? '',
    spanId: row.span_id,
    name: row.name ?? '',
    startedAt: row.started_at ?? 0,
    endedAt: row.ended_at ?? 0,
    durationMs: row.duration_ms ?? 0,
    attributes: safeJsonParse<Record<string, unknown>>(row.attributes, {}),
    inputTokens: row.input_tokens ?? 0,
    outputTokens: row.output_tokens ?? 0,
    isError: !!row.is_error,
  };
  if (row.parent_span_id !== null) rec.parentSpanId = row.parent_span_id;
  if (row.spec !== null) rec.spec = row.spec;
  if (row.phase !== null) rec.phase = row.phase;
  if (row.model !== null) rec.model = row.model;
  return rec;
}

function rowToKnowledge(row: KnowledgeRow): KnowledgeRecord {
  return {
    id: row.id,
    type: row.type ?? '',
    name: row.name ?? '',
    description: row.description ?? '',
    confidence: row.confidence ?? 0,
    createdAt: row.created_at ?? '',
    updatedAt: row.updated_at ?? '',
    source: row.source ?? '',
  };
}

// ---------------------------------------------------------------------------
// EventStore
// ---------------------------------------------------------------------------

export class EventStore {
  private readonly path: string;
  private db: SqliteDatabase | null = null;

  constructor(dbPath: string) {
    this.path = dbPath;
  }

  init(): void {
    if (this.db) return; // memoize
    const shim = loadRuntimeShim();
    const Ctor = shim.loadSqlite();
    if (!Ctor) {
      throw new Error(
        'EventStore: SQLite driver unavailable. Phase 1 requires Bun runtime ' +
          '(bun:sqlite). Node fallback (better-sqlite3) is not yet implemented.'
      );
    }
    const db = new Ctor(this.path);
    // WAL: concurrent reads + serialized writes. Required for hook safety.
    db.exec('PRAGMA journal_mode=WAL;');
    db.exec('PRAGMA foreign_keys=ON;');

    // Self-heal: pre-existing DBs from Phase 1 Wave 1 carry the broken
    // knowledge_fts declaration (content='knowledge', content_rowid='id' with
    // TEXT id). Detect via sqlite_master and drop so SCHEMA_SQL recreates
    // standalone. Idempotent: missing/already-fixed → no-op.
    try {
      const row = db
        .prepare(
          `SELECT sql FROM sqlite_master WHERE type='table' AND name='knowledge_fts'`
        )
        .get() as { sql?: string } | undefined;
      if (row && typeof row.sql === 'string' && /content\s*=\s*'knowledge'/i.test(row.sql)) {
        db.exec('DROP TABLE IF EXISTS knowledge_fts;');
      }
    } catch {
      // best-effort — if introspection fails, SCHEMA_SQL's IF NOT EXISTS keeps us safe.
    }

    db.exec(SCHEMA_SQL);
    this.db = db;
  }

  private requireDb(): SqliteDatabase {
    if (!this.db) {
      throw new Error('EventStore: call init() before using the store.');
    }
    return this.db;
  }

  append(ev: EventRecord): void {
    const db = this.requireDb();
    const stmt = db.prepare(
      `INSERT INTO events (ts, session_id, wave, spec, event, actor_kind, actor_id, payload)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`
    );
    stmt.run(
      ev.ts,
      ev.sessionId ?? null,
      ev.wave ?? null,
      ev.spec ?? null,
      ev.event,
      ev.actor?.kind ?? null,
      ev.actor?.id ?? null,
      JSON.stringify(ev.payload ?? {})
    );
  }

  query(filter: QueryFilter = {}): EventRecord[] {
    const db = this.requireDb();
    const where: string[] = [];
    const params: unknown[] = [];
    if (filter.spec !== undefined) {
      where.push('spec = ?');
      params.push(filter.spec);
    }
    if (filter.event !== undefined) {
      where.push('event = ?');
      params.push(filter.event);
    }
    if (filter.since !== undefined) {
      where.push('ts >= ?');
      params.push(filter.since);
    }
    const sql =
      'SELECT id, ts, session_id, wave, spec, event, actor_kind, actor_id, payload FROM events' +
      (where.length ? ` WHERE ${where.join(' AND ')}` : '') +
      ' ORDER BY id ASC';
    const rows = db.prepare(sql).all(...params) as EventRow[];
    return rows.map(rowToEvent);
  }

  search(text: string): EventRecord[] {
    const db = this.requireDb();
    const rows = db
      .prepare(
        `SELECT e.id, e.ts, e.session_id, e.wave, e.spec, e.event, e.actor_kind, e.actor_id, e.payload
         FROM events e
         JOIN events_fts ON events_fts.rowid = e.id
         WHERE events_fts MATCH ?
         ORDER BY e.id ASC`
      )
      .all(text) as EventRow[];
    return rows.map(rowToEvent);
  }

  /**
   * Rebuild denormalized projections (specs + metrics_projection) from the
   * events table. Atomic — either the projections fully reflect events or
   * the transaction rolls back. Does not touch the knowledge tables (those
   * are populated by migration / hook writes, not derived from events).
   */
  rebuild(): void {
    const db = this.requireDb();
    db.exec('BEGIN');
    try {
      db.exec('DELETE FROM metrics_projection;');
      db.exec('DELETE FROM specs;');

      const events = this.queryRaw();

      // Specs: any event carrying a `spec` field anchors the spec. Status/phase
      // derive from completion + pipeline.phase events. This mirrors
      // buildPipelineState (templates/scripts/event-projections.js) which is the
      // ground truth: the dashboard expects metrics for every spec the harness
      // mentions, not only those with explicit spec.start/spec.complete.
      interface SpecAcc {
        name: string;
        status: string;
        phase: string;
        startedAt?: string;
        completedAt?: string;
        affectedFiles?: string[];
      }
      const specs = new Map<string, SpecAcc>();
      const ensureSpec = (name: string, ts: string): SpecAcc => {
        let s = specs.get(name);
        if (!s) {
          s = { name, status: 'active', phase: '', startedAt: ts };
          specs.set(name, s);
        } else if (!s.startedAt || ts < s.startedAt) {
          s.startedAt = ts;
        }
        return s;
      };

      // Metrics: per-spec accumulators (aligned with buildPipelineState).
      interface MetricsAcc {
        apiCalls: number;
        retries: number;
        toolBreakdown: Record<string, number>;
        dispatchFailuresByPhase: Record<string, number>;
        agentCount: number;
        updatedAt: string;
      }
      const metrics = new Map<string, MetricsAcc>();
      const ensureMetrics = (spec: string): MetricsAcc => {
        let m = metrics.get(spec);
        if (!m) {
          m = {
            apiCalls: 0,
            retries: 0,
            toolBreakdown: {},
            dispatchFailuresByPhase: {},
            agentCount: 0,
            updatedAt: '',
          };
          metrics.set(spec, m);
        }
        return m;
      };

      for (const ev of events) {
        const spec = ev.spec;
        if (!spec) continue;

        // Spec projection — every spec-tagged event is an anchor.
        const s = ensureSpec(spec, ev.ts);

        if (typeof ev.event === 'string') {
          if (ev.event === 'spec.complete' || ev.event === 'pipeline.complete') {
            s.completedAt = ev.ts;
            s.status = 'completed';
          } else if (ev.event === 'spec.cancel') {
            s.status = 'cancelled';
          } else if (ev.event === 'pipeline.phase') {
            // buildPipelineState derives phase from pipeline.phase events.
            const p = ev.payload ?? {};
            const to = typeof p['to'] === 'string' ? (p['to'] as string) : null;
            const from =
              typeof p['from'] === 'string' ? (p['from'] as string) : null;
            if (to) s.phase = to;
            else if (from) s.phase = from;
          } else if (
            ev.event === 'phase.enter' &&
            ev.payload &&
            typeof ev.payload['phase'] === 'string'
          ) {
            s.phase = ev.payload['phase'] as string;
          } else if (ev.event.startsWith('spec.')) {
            // Legacy spec.* events may carry phase in payload.
            const payloadPhase =
              ev.payload && typeof ev.payload['phase'] === 'string'
                ? (ev.payload['phase'] as string)
                : null;
            if (payloadPhase) s.phase = payloadPhase;
          }
        }

        // Metrics projection — rules copied from buildPipelineState.
        const m = ensureMetrics(spec);
        m.updatedAt = ev.ts;
        if (ev.event === 'tool.use') {
          // buildPipelineState reads payload.tool (not payload.toolName) and
          // excludes Read from apiCalls + toolBreakdown.
          const tool =
            ev.payload && typeof ev.payload['tool'] === 'string'
              ? (ev.payload['tool'] as string)
              : 'unknown';
          if (tool !== 'Read') {
            m.apiCalls += 1;
            m.toolBreakdown[tool] = (m.toolBreakdown[tool] ?? 0) + 1;
          }
        }
        if (ev.event === 'dispatch.failure') {
          // retries = count of dispatch.failure (buildPipelineState semantics).
          m.retries += 1;
          const phase =
            ev.payload && typeof ev.payload['phase'] === 'string'
              ? (ev.payload['phase'] as string)
              : 'UNKNOWN';
          m.dispatchFailuresByPhase[phase] =
            (m.dispatchFailuresByPhase[phase] ?? 0) + 1;
        }
        if (ev.event === 'agent.start') {
          // buildPipelineState counts every agent.start, not unique actors.
          m.agentCount += 1;
        }
      }

      const specInsert = db.prepare(
        `INSERT INTO specs (name, status, phase, started_at, completed_at, affected_files)
         VALUES (?, ?, ?, ?, ?, ?)`
      );
      for (const s of specs.values()) {
        specInsert.run(
          s.name,
          s.status,
          s.phase,
          s.startedAt ?? null,
          s.completedAt ?? null,
          s.affectedFiles ? JSON.stringify(s.affectedFiles) : null
        );
      }

      const metricsInsert = db.prepare(
        `INSERT INTO metrics_projection
         (spec, api_calls, retries, pass1, tool_breakdown, dispatch_failures_by_phase, agent_count, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)`
      );
      for (const [spec, m] of metrics.entries()) {
        // Only insert metrics for specs that also have a row (FK).
        if (!specs.has(spec)) continue;
        // pass1 derives from retries (no dispatch failures ⇒ first-pass success).
        const pass1 = m.retries === 0 ? 1 : 0;
        metricsInsert.run(
          spec,
          m.apiCalls,
          m.retries,
          pass1,
          JSON.stringify(m.toolBreakdown),
          JSON.stringify(m.dispatchFailuresByPhase),
          m.agentCount,
          m.updatedAt
        );
      }

      db.exec('COMMIT');
    } catch (err) {
      db.exec('ROLLBACK');
      throw err;
    }
  }

  // Raw events read used by rebuild (avoids double row→record conversion cost).
  private queryRaw(): EventRecord[] {
    return this.query();
  }

  tables(): string[] {
    const db = this.requireDb();
    const rows = db
      .prepare(
        `SELECT name FROM sqlite_master WHERE type IN ('table', 'view') ORDER BY name`
      )
      .all() as Array<{ name: string }>;
    return rows.map((r) => r.name);
  }

  eventCount(): number {
    const db = this.requireDb();
    const row = db.prepare('SELECT COUNT(*) AS n FROM events').get() as {
      n: number;
    };
    return row?.n ?? 0;
  }

  specs(): SpecRecord[] {
    const db = this.requireDb();
    const rows = db
      .prepare(
        `SELECT name, status, phase, started_at, completed_at, affected_files
         FROM specs ORDER BY name ASC`
      )
      .all() as SpecRow[];
    return rows.map(rowToSpec);
  }

  metrics(spec: string): MetricsRecord | null {
    const db = this.requireDb();
    const row = db
      .prepare(
        `SELECT spec, api_calls, retries, pass1, tool_breakdown,
                dispatch_failures_by_phase, agent_count, updated_at
         FROM metrics_projection WHERE spec = ?`
      )
      .get(spec) as MetricsRow | undefined;
    return row ? rowToMetrics(row) : null;
  }

  knowledge(
    filter: { minConfidence?: number; limit?: number; search?: string } = {}
  ): KnowledgeRecord[] {
    const db = this.requireDb();

    // FTS5 branch: when `search` is present, join knowledge_fts on the
    // UNINDEXED `id` column and order by bm25 (lower=better). Falls back
    // silently when knowledge_fts is empty or MATCH is malformed.
    if (filter.search !== undefined && filter.search.trim() !== '') {
      const params: unknown[] = [filter.search];
      let sql =
        `SELECT k.id, k.type, k.name, k.description, k.confidence,
                k.created_at, k.updated_at, k.source
         FROM knowledge_fts f
         JOIN knowledge k ON k.id = f.id
         WHERE knowledge_fts MATCH ?`;
      if (filter.minConfidence !== undefined) {
        sql += ' AND k.confidence >= ?';
        params.push(filter.minConfidence);
      }
      sql += ' ORDER BY bm25(knowledge_fts), k.confidence DESC, k.id ASC';
      if (filter.limit !== undefined) {
        sql += ' LIMIT ?';
        params.push(filter.limit);
      }
      try {
        const rows = db.prepare(sql).all(...params) as KnowledgeRow[];
        return rows.map(rowToKnowledge);
      } catch (err) {
        // FTS5 MATCH parse errors (e.g. user types "a:b") fail-open to empty.
        process.stderr.write(
          `[event-store] knowledge FTS5 query failed: ${String(err)}\n`
        );
        return [];
      }
    }

    // Fallback: no search → simple confidence/limit filter.
    const where: string[] = [];
    const params: unknown[] = [];
    if (filter.minConfidence !== undefined) {
      where.push('confidence >= ?');
      params.push(filter.minConfidence);
    }
    let sql =
      `SELECT id, type, name, description, confidence, created_at, updated_at, source
       FROM knowledge` +
      (where.length ? ` WHERE ${where.join(' AND ')}` : '') +
      ' ORDER BY confidence DESC, id ASC';
    if (filter.limit !== undefined) {
      sql += ' LIMIT ?';
      params.push(filter.limit);
    }
    const rows = db.prepare(sql).all(...params) as KnowledgeRow[];
    return rows.map(rowToKnowledge);
  }

  /**
   * Query the spans projection. Legacy from Phase 2 (homegrown OTLP emitter
   * removed in favor of native Claude Code OTEL). Kept for historical rows.
   * See claude_code_otel table for live data. Default limit 1000.
   */
  spans(filter: SpanFilter = {}): SpanRecord[] {
    const db = this.requireDb();
    const where: string[] = [];
    const params: unknown[] = [];
    if (filter.spec !== undefined) {
      where.push('spec = ?');
      params.push(filter.spec);
    }
    if (filter.phase !== undefined) {
      where.push('phase = ?');
      params.push(filter.phase);
    }
    if (filter.since !== undefined) {
      where.push('started_at >= ?');
      params.push(filter.since);
    }
    const limit = filter.limit ?? 1000;
    const sql =
      `SELECT trace_id, span_id, parent_span_id, name, started_at, ended_at,
              duration_ms, attributes, spec, phase, model, input_tokens,
              output_tokens, is_error
       FROM spans` +
      (where.length ? ` WHERE ${where.join(' AND ')}` : '') +
      ' ORDER BY started_at ASC LIMIT ?';
    params.push(limit);
    const rows = db.prepare(sql).all(...params) as SpanRow[];
    return rows.map(rowToSpan);
  }

  close(): void {
    if (this.db) {
      this.db.close();
      this.db = null;
    }
  }
}
