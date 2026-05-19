/**
 * Mustard 2.0 Phase 1 — JSONL → SQLite migration.
 *
 * One-shot, idempotent rehydration of `.claude/.harness/mustard.db` from the
 * legacy file-based stores:
 *   - events.jsonl                       → events  (INSERT OR IGNORE)
 *   - knowledge.json                     → knowledge + knowledge_fts (REPLACE)
 *   - .pipeline-states/<spec>.json       → specs   (INSERT OR REPLACE)
 *   - .pipeline-states/<spec>.metrics.json → metrics_projection (INSERT OR REPLACE)
 *
 * Idempotency contract:
 *   - Events: composite UNIQUE index (ts, session_id, event, actor_id) +
 *     `INSERT OR IGNORE`. Running the migration N times yields the same row
 *     count as running it once.
 *   - Knowledge / specs / metrics: natural primary keys + `INSERT OR REPLACE`.
 *
 * knowledge_fts (Phase 4 Wave 1 fix):
 *   The Phase 1 schema declared knowledge_fts as external-content over
 *   `knowledge` keyed by content_rowid='id', but knowledge.id is TEXT and
 *   FTS5 external-content requires INTEGER rowid — produced "database disk
 *   image is malformed" on Windows. Wave 1 changes the schema to STANDALONE
 *   FTS5 with an UNINDEXED `id` column. EventStore.init() self-heals
 *   pre-existing DBs by detecting the old declaration and dropping the
 *   virtual table before re-running SCHEMA_SQL. Migration here populates
 *   `(id, name, description)` with manual deterministic rowids so re-runs
 *   produce identical row layout.
 *
 * CLI:
 *   node dist/migrate/jsonl-to-sqlite.js <harnessDir>             # full migration
 *   node dist/migrate/jsonl-to-sqlite.js <harnessDir> --dry-count # count events.jsonl lines only
 *
 *   <harnessDir> is the `.claude/.harness/` directory of the target project.
 *   The script writes to `<harnessDir>/mustard.db` and reads legacy stores
 *   from the sibling `.claude/` directory (parent of `<harnessDir>`).
 */

import { createRequire } from 'node:module';
import * as fs from 'node:fs';
import * as path from 'node:path';
import * as url from 'node:url';
import { EventStore } from '../runtime/event-store.js';

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export interface MigrationResult {
  eventsImported: number;
  eventsSkipped: number;
  knowledgeImported: number;
  specsImported: number;
  metricsImported: number;
  spansImported: number;
  spansSkipped: number;
}

// ---------------------------------------------------------------------------
// SQLite driver bootstrap — mirrors EventStore.loadRuntimeShim for the
// statements the migration needs that EventStore doesn't expose (OR IGNORE,
// OR REPLACE, transactional bulk writes, manual FTS5 population).
// ---------------------------------------------------------------------------

interface SqliteStatement {
  run(...params: unknown[]): { changes?: number; lastInsertRowid?: number };
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

function loadRuntimeShim(): RuntimeShim {
  const here = url.fileURLToPath(import.meta.url);
  // dist/migrate/jsonl-to-sqlite.js → repo root is two levels up.
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
// File parsers — tolerant to malformed input (legacy stores are hand-written
// at times). Bad lines log to stderr and are skipped; never abort migration.
// ---------------------------------------------------------------------------

interface RawEvent {
  ts?: string;
  sessionId?: string;
  wave?: number;
  spec?: string;
  event?: string;
  actor?: { kind?: string; id?: string };
  payload?: Record<string, unknown>;
}

function readEventsJsonl(filePath: string): RawEvent[] {
  if (!fs.existsSync(filePath)) return [];
  const raw = fs.readFileSync(filePath, 'utf8');
  const out: RawEvent[] = [];
  let lineNo = 0;
  for (const line of raw.split('\n')) {
    lineNo += 1;
    const trimmed = line.trim();
    if (!trimmed) continue;
    try {
      out.push(JSON.parse(trimmed) as RawEvent);
    } catch (err) {
      process.stderr.write(
        `[migrate] events.jsonl: skipping malformed line ${lineNo}: ${(err as Error).message}\n`
      );
    }
  }
  return out;
}

interface RawKnowledge {
  id?: string;
  type?: string;
  name?: string;
  description?: string;
  confidence?: number;
  createdAt?: string;
  updatedAt?: string;
  source?: string;
}

function readKnowledgeJson(filePath: string): RawKnowledge[] {
  if (!fs.existsSync(filePath)) return [];
  let parsed: unknown;
  try {
    parsed = JSON.parse(fs.readFileSync(filePath, 'utf8'));
  } catch (err) {
    process.stderr.write(
      `[migrate] knowledge.json: parse failed: ${(err as Error).message}\n`
    );
    return [];
  }
  if (Array.isArray(parsed)) return parsed as RawKnowledge[];
  if (parsed && typeof parsed === 'object') {
    const obj = parsed as { entries?: unknown };
    if (Array.isArray(obj.entries)) return obj.entries as RawKnowledge[];
  }
  return [];
}

interface RawSpecState {
  specName?: string;
  status?: string;
  phaseName?: string;
  startedAt?: string;
  createdAt?: string;
  completedAt?: string;
  updatedAt?: string;
  affectedFiles?: unknown;
}

interface RawMetricsFile {
  v?: number;
  metrics?: {
    apiCalls?: number;
    retries?: number;
    pass1?: boolean;
    toolBreakdown?: Record<string, number>;
    dispatchFailuresByPhase?: Record<string, number>;
    agentCount?: number;
    updatedAt?: string;
  };
}

// ---------------------------------------------------------------------------
// Spans projection — Phase 2.
//
// spans.jsonl emits one full OTLP/JSON `resourceSpans` wrapper per line (one
// span per line by Mustard convention). This parser is intentionally
// conservative: malformed/incomplete lines are skipped (counted as skipped)
// without aborting the migration. Schema mapping is documented in the spec.
// ---------------------------------------------------------------------------

interface FlatSpan {
  traceId: string;
  spanId: string;
  parentSpanId: string | null;
  name: string;
  startedAtMs: number;
  endedAtMs: number;
  durationMs: number;
  attributes: Record<string, unknown>;
  spec: string | null;
  phase: string | null;
  model: string | null;
  inputTokens: number;
  outputTokens: number;
  isError: number; // 0|1
}

/** Convert OTLP nano-string to ms epoch (integer). Returns NaN on parse fail. */
function nanoStringToMs(ns: unknown): number {
  if (typeof ns !== 'string' || !ns) return NaN;
  try {
    return Number(BigInt(ns) / 1_000_000n);
  } catch {
    return NaN;
  }
}

/** Decode an OTLP attribute KeyValue into a plain JS scalar. */
function decodeAttrValue(v: unknown): unknown {
  if (!v || typeof v !== 'object') return undefined;
  const obj = v as Record<string, unknown>;
  if ('stringValue' in obj) return obj.stringValue;
  if ('intValue' in obj) {
    const iv = obj.intValue;
    if (typeof iv === 'string') {
      const n = Number(iv);
      return Number.isFinite(n) ? n : iv;
    }
    return iv;
  }
  if ('doubleValue' in obj) return obj.doubleValue;
  if ('boolValue' in obj) return obj.boolValue;
  return undefined;
}

/** Parse one OTLP/JSON wrapper line → flat row shape, or null if invalid. */
function parseOtlpSpanLine(line: string): FlatSpan | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(line);
  } catch {
    return null;
  }
  const root = parsed as { resourceSpans?: unknown };
  if (!root || !Array.isArray(root.resourceSpans) || root.resourceSpans.length === 0) {
    return null;
  }
  const rs = root.resourceSpans[0] as { scopeSpans?: unknown };
  if (!rs || !Array.isArray(rs.scopeSpans) || rs.scopeSpans.length === 0) return null;
  const ss = rs.scopeSpans[0] as { spans?: unknown };
  if (!ss || !Array.isArray(ss.spans) || ss.spans.length === 0) return null;
  const span = ss.spans[0] as Record<string, unknown>;

  const traceId = typeof span.traceId === 'string' ? span.traceId : '';
  const spanId = typeof span.spanId === 'string' ? span.spanId : '';
  if (!spanId) return null;

  const startedAtMs = nanoStringToMs(span.startTimeUnixNano);
  const endedAtMs = nanoStringToMs(span.endTimeUnixNano);
  if (!Number.isFinite(startedAtMs) || !Number.isFinite(endedAtMs)) return null;

  const rawAttrs = Array.isArray(span.attributes) ? (span.attributes as unknown[]) : [];
  const attrs: Record<string, unknown> = {};
  for (const kv of rawAttrs) {
    if (!kv || typeof kv !== 'object') continue;
    const { key, value } = kv as { key?: unknown; value?: unknown };
    if (typeof key !== 'string') continue;
    const decoded = decodeAttrValue(value);
    if (decoded !== undefined) attrs[key] = decoded;
  }

  const statusObj = span.status as { code?: unknown } | undefined;
  const isError =
    statusObj && typeof statusObj.code === 'number' && statusObj.code === 2 ? 1 : 0;

  const inputTokensRaw = attrs['gen_ai.usage.input_tokens'];
  const outputTokensRaw = attrs['gen_ai.usage.output_tokens'];

  return {
    traceId,
    spanId,
    parentSpanId: typeof span.parentSpanId === 'string' ? span.parentSpanId : null,
    name: typeof span.name === 'string' ? span.name : '',
    startedAtMs: Math.trunc(startedAtMs),
    endedAtMs: Math.trunc(endedAtMs),
    durationMs: Math.max(0, Math.trunc(endedAtMs - startedAtMs)),
    attributes: attrs,
    spec: typeof attrs['mustard.spec'] === 'string' ? (attrs['mustard.spec'] as string) : null,
    phase: typeof attrs['mustard.phase'] === 'string' ? (attrs['mustard.phase'] as string) : null,
    model:
      typeof attrs['gen_ai.request.model'] === 'string'
        ? (attrs['gen_ai.request.model'] as string)
        : null,
    inputTokens: typeof inputTokensRaw === 'number' ? inputTokensRaw : 0,
    outputTokens: typeof outputTokensRaw === 'number' ? outputTokensRaw : 0,
    isError,
  };
}

function readSpansJsonl(filePath: string): { spans: FlatSpan[]; skipped: number } {
  if (!fs.existsSync(filePath)) return { spans: [], skipped: 0 };
  const raw = fs.readFileSync(filePath, 'utf8');
  const out: FlatSpan[] = [];
  let skipped = 0;
  let lineNo = 0;
  for (const line of raw.split('\n')) {
    lineNo += 1;
    const trimmed = line.trim();
    if (!trimmed) continue;
    const parsed = parseOtlpSpanLine(trimmed);
    if (parsed) out.push(parsed);
    else {
      skipped += 1;
      process.stderr.write(
        `[migrate] spans.jsonl: skipping malformed line ${lineNo}\n`
      );
    }
  }
  return { spans: out, skipped };
}

function readJsonOrNull<T>(filePath: string): T | null {
  if (!fs.existsSync(filePath)) return null;
  try {
    return JSON.parse(fs.readFileSync(filePath, 'utf8')) as T;
  } catch (err) {
    process.stderr.write(
      `[migrate] ${path.basename(filePath)}: parse failed: ${(err as Error).message}\n`
    );
    return null;
  }
}

interface PipelineStateFiles {
  states: Array<{ specName: string; file: string }>;
  metrics: Array<{ specName: string; file: string }>;
}

function listPipelineStateFiles(pipelineStatesDir: string): PipelineStateFiles {
  const out: PipelineStateFiles = { states: [], metrics: [] };
  if (!fs.existsSync(pipelineStatesDir)) return out;
  for (const entry of fs.readdirSync(pipelineStatesDir)) {
    if (!entry.endsWith('.json')) continue;
    const full = path.join(pipelineStatesDir, entry);
    if (entry.endsWith('.metrics.json')) {
      const specName = entry.slice(0, -'.metrics.json'.length);
      out.metrics.push({ specName, file: full });
    } else {
      const specName = entry.slice(0, -'.json'.length);
      out.states.push({ specName, file: full });
    }
  }
  return out;
}

// ---------------------------------------------------------------------------
// CLI helpers
// ---------------------------------------------------------------------------

function countEventLines(harnessDir: string): number {
  const file = path.join(harnessDir, 'events.jsonl');
  if (!fs.existsSync(file)) return 0;
  const raw = fs.readFileSync(file, 'utf8');
  let n = 0;
  for (const line of raw.split('\n')) {
    if (line.trim()) n += 1;
  }
  return n;
}

// ---------------------------------------------------------------------------
// Main migration
// ---------------------------------------------------------------------------

/**
 * Migrate a single `.harness/` directory into `<harnessDir>/mustard.db`.
 * Safe to call repeatedly — duplicates in events.jsonl are deduped by the
 * UNIQUE index; knowledge/specs/metrics use REPLACE semantics keyed by their
 * natural primary keys.
 */
export function migrate(harnessDir: string): MigrationResult {
  if (!fs.existsSync(harnessDir)) {
    throw new Error(`[migrate] harness dir not found: ${harnessDir}`);
  }
  // .claude/ is the parent of .harness/ — legacy stores live there.
  const claudeDir = path.dirname(harnessDir);
  const dbPath = path.join(harnessDir, 'mustard.db');

  // Bootstrap schema via EventStore (no duplication of CREATE TABLE).
  const store = new EventStore(dbPath);
  store.init();

  // Open a second handle for migration-specific statements (OR IGNORE / OR
  // REPLACE / manual FTS population). bun:sqlite allows concurrent handles on
  // WAL-mode DBs. We never hold both writers simultaneously across awaits.
  const shim = loadRuntimeShim();
  const Ctor = shim.loadSqlite();
  if (!Ctor) {
    throw new Error(
      '[migrate] SQLite driver unavailable. Requires Bun (bun:sqlite).'
    );
  }
  const db = new Ctor(dbPath);

  try {
    // Ensure idempotency index. SQLite UNIQUE indexes treat NULL as distinct
    // (each NULL row is unique), so wrap nullable columns in COALESCE so two
    // events with the same ts/event but NULL actor_id collide as expected.
    // Created post-init so EventStore core stays unaware of migration
    // semantics (per task spec — Wave 5 may absorb later).
    db.exec(
      `CREATE UNIQUE INDEX IF NOT EXISTS uniq_events ON events(
         ts,
         COALESCE(session_id, ''),
         event,
         COALESCE(actor_id, '')
       )`
    );

    const result: MigrationResult = {
      eventsImported: 0,
      eventsSkipped: 0,
      knowledgeImported: 0,
      specsImported: 0,
      metricsImported: 0,
      spansImported: 0,
      spansSkipped: 0,
    };

    // -- Events -----------------------------------------------------------
    const rawEvents = readEventsJsonl(path.join(harnessDir, 'events.jsonl'));
    if (rawEvents.length > 0) {
      const insertEv = db.prepare(
        `INSERT OR IGNORE INTO events
           (ts, session_id, wave, spec, event, actor_kind, actor_id, payload)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)`
      );
      db.exec('BEGIN');
      try {
        for (const ev of rawEvents) {
          if (!ev.ts || !ev.event) {
            result.eventsSkipped += 1;
            continue;
          }
          const res = insertEv.run(
            ev.ts,
            ev.sessionId ?? null,
            ev.wave ?? null,
            ev.spec ?? null,
            ev.event,
            ev.actor?.kind ?? null,
            ev.actor?.id ?? null,
            JSON.stringify(ev.payload ?? {})
          );
          if (res.changes && res.changes > 0) {
            result.eventsImported += 1;
          } else {
            result.eventsSkipped += 1;
          }
        }
        db.exec('COMMIT');
      } catch (err) {
        db.exec('ROLLBACK');
        throw err;
      }
    }

    // -- Knowledge --------------------------------------------------------
    const rawKnowledge = readKnowledgeJson(path.join(claudeDir, 'knowledge.json'));
    if (rawKnowledge.length > 0) {
      const insertK = db.prepare(
        `INSERT OR REPLACE INTO knowledge
           (id, type, name, description, confidence, created_at, updated_at, source)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)`
      );
      db.exec('BEGIN');
      try {
        for (const k of rawKnowledge) {
          if (!k.id) continue;
          insertK.run(
            k.id,
            k.type ?? null,
            k.name ?? null,
            k.description ?? null,
            typeof k.confidence === 'number' ? k.confidence : null,
            k.createdAt ?? null,
            k.updatedAt ?? null,
            k.source ?? null
          );
          result.knowledgeImported += 1;
        }
        // Manual FTS5 repopulation against the standalone knowledge_fts.
        // Assign deterministic rowids (ROW_NUMBER ORDER BY id) so re-runs
        // produce identical layout — preserves idempotency contract.
        db.exec('DELETE FROM knowledge_fts');
        db.exec(
          `INSERT INTO knowledge_fts(rowid, id, name, description)
           SELECT ROW_NUMBER() OVER (ORDER BY id), id,
                  COALESCE(name, ''), COALESCE(description, '')
           FROM knowledge`
        );
        db.exec('COMMIT');
      } catch (err) {
        db.exec('ROLLBACK');
        throw err;
      }
    }

    // -- Specs (.pipeline-states/<spec>.json) -----------------------------
    const pipelineStatesDir = path.join(claudeDir, '.pipeline-states');
    const pipeFiles = listPipelineStateFiles(pipelineStatesDir);
    if (pipeFiles.states.length > 0) {
      const insertSpec = db.prepare(
        `INSERT OR REPLACE INTO specs
           (name, status, phase, started_at, completed_at, affected_files)
         VALUES (?, ?, ?, ?, ?, ?)`
      );
      db.exec('BEGIN');
      try {
        for (const { specName, file } of pipeFiles.states) {
          const state = readJsonOrNull<RawSpecState>(file);
          if (!state) continue;
          const name = state.specName ?? specName;
          const affected =
            state.affectedFiles && Array.isArray(state.affectedFiles)
              ? JSON.stringify(state.affectedFiles)
              : null;
          insertSpec.run(
            name,
            state.status ?? null,
            state.phaseName ?? null,
            state.startedAt ?? state.createdAt ?? null,
            state.completedAt ?? null,
            affected
          );
          result.specsImported += 1;
        }
        db.exec('COMMIT');
      } catch (err) {
        db.exec('ROLLBACK');
        throw err;
      }
    }

    // -- Metrics projection (.pipeline-states/<spec>.metrics.json) --------
    if (pipeFiles.metrics.length > 0) {
      // FK to specs(name): pre-insert a placeholder spec row if missing so
      // the metrics REPLACE doesn't fail FK. Idempotent via INSERT OR IGNORE.
      const ensureSpec = db.prepare(
        `INSERT OR IGNORE INTO specs (name, status, phase) VALUES (?, ?, ?)`
      );
      const insertMetrics = db.prepare(
        `INSERT OR REPLACE INTO metrics_projection
           (spec, api_calls, retries, pass1, tool_breakdown,
            dispatch_failures_by_phase, agent_count, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)`
      );
      db.exec('BEGIN');
      try {
        for (const { specName, file } of pipeFiles.metrics) {
          const raw = readJsonOrNull<RawMetricsFile>(file);
          if (!raw || !raw.metrics) continue;
          const m = raw.metrics;
          ensureSpec.run(specName, 'unknown', '');
          insertMetrics.run(
            specName,
            typeof m.apiCalls === 'number' ? m.apiCalls : 0,
            typeof m.retries === 'number' ? m.retries : 0,
            m.pass1 ? 1 : 0,
            JSON.stringify(m.toolBreakdown ?? {}),
            JSON.stringify(m.dispatchFailuresByPhase ?? {}),
            typeof m.agentCount === 'number' ? m.agentCount : 0,
            m.updatedAt ?? ''
          );
          result.metricsImported += 1;
        }
        db.exec('COMMIT');
      } catch (err) {
        db.exec('ROLLBACK');
        throw err;
      }
    }

    // -- Spans projection (Phase 2: spans.jsonl → spans) ------------------
    // Spans are optional — only Phase 2+ projects emit them. Skip silently
    // when the file is absent. Key: span_id PRIMARY KEY (INSERT OR IGNORE).
    const { spans: rawSpans, skipped: spansSkippedParse } = readSpansJsonl(
      path.join(harnessDir, 'spans.jsonl')
    );
    result.spansSkipped += spansSkippedParse;
    if (rawSpans.length > 0) {
      const insertSpan = db.prepare(
        `INSERT OR IGNORE INTO spans
           (trace_id, span_id, parent_span_id, name, started_at, ended_at,
            duration_ms, attributes, spec, phase, model, input_tokens,
            output_tokens, is_error)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`
      );
      db.exec('BEGIN');
      try {
        for (const s of rawSpans) {
          const res = insertSpan.run(
            s.traceId,
            s.spanId,
            s.parentSpanId,
            s.name,
            s.startedAtMs,
            s.endedAtMs,
            s.durationMs,
            JSON.stringify(s.attributes),
            s.spec,
            s.phase,
            s.model,
            s.inputTokens,
            s.outputTokens,
            s.isError
          );
          if (res.changes && res.changes > 0) result.spansImported += 1;
          else result.spansSkipped += 1;
        }
        db.exec('COMMIT');
      } catch (err) {
        db.exec('ROLLBACK');
        throw err;
      }
    }

    return result;
  } finally {
    db.close();
    store.close();
  }
}

// ---------------------------------------------------------------------------
// CLI entry
// ---------------------------------------------------------------------------

function isMain(): boolean {
  // ESM-equivalent of `require.main === module`. Works under Node 20+ and Bun.
  try {
    const thisFile = url.fileURLToPath(import.meta.url);
    const argv1 = process.argv[1];
    if (!argv1) return false;
    return path.resolve(argv1) === path.resolve(thisFile);
  } catch {
    return false;
  }
}

function cli(argv: string[]): number {
  if (argv.length < 1) {
    process.stderr.write(
      'usage: jsonl-to-sqlite <harnessDir> [--dry-count]\n'
    );
    return 2;
  }
  const harnessDir = argv[0]!;
  const dryCount = argv.includes('--dry-count');

  if (dryCount) {
    const n = countEventLines(harnessDir);
    process.stdout.write(`${n}\n`);
    return 0;
  }

  try {
    const result = migrate(harnessDir);
    process.stdout.write(JSON.stringify(result) + '\n');
    return 0;
  } catch (err) {
    process.stderr.write(`[migrate] failed: ${(err as Error).message}\n`);
    return 1;
  }
}

if (isMain()) {
  process.exit(cli(process.argv.slice(2)));
}
