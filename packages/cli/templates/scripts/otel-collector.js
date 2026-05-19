#!/usr/bin/env bun
'use strict';
// <!-- mustard:generated -->
/**
 * OTEL-COLLECTOR: Local OTLP/JSON receiver for Claude Code native telemetry.
 *
 * Listens on 127.0.0.1:${MUSTARD_OTEL_PORT||4318} and projects incoming OTLP/JSON
 * payloads into the `claude_code_otel` table inside .claude/.harness/mustard.db.
 *
 * Routes:
 *   POST /v1/metrics  — opentelemetry-proto MetricsService (resourceMetrics[])
 *   POST /v1/logs     — opentelemetry-proto LogsService    (resourceLogs[])
 *   GET  /healthz     — liveness probe
 *
 * Aggregation: per-(metric, session_id, model, token_type) within 1-minute
 * buckets via UPSERT (sum += excluded.sum, count += excluded.count).
 *
 * Lifecycle:
 *   - SIGTERM/SIGINT → close DB, exit 0.
 *   - EventStore init failure → log to canary, exit 1 (parent respawns).
 *
 * Canary log: .claude/.harness/.canary.log
 *   - One JSONL line per request (latency_ms, route, count).
 *   - One JSONL line per parse error (error class + truncated message).
 *
 * Fail-open contract: parse errors return 400 but never crash the server. The
 * collector is best-effort — losing a few datapoints is preferable to taking
 * down the user's hook pipeline.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');

const PROJECT_DIR = process.env.CLAUDE_PROJECT_DIR || process.cwd();
const CLAUDE_DIR = path.join(PROJECT_DIR, '.claude');
const HARNESS_DIR = path.join(CLAUDE_DIR, '.harness');
const CANARY_LOG = path.join(HARNESS_DIR, '.canary.log');
const PORT = parseInt(process.env.MUSTARD_OTEL_PORT || '4318', 10);

// ── Canary logging (fail-silent) ─────────────────────────────────────────────
function canary(record) {
  try {
    if (!fs.existsSync(HARNESS_DIR)) fs.mkdirSync(HARNESS_DIR, { recursive: true });
    fs.appendFileSync(CANARY_LOG, JSON.stringify(record) + '\n', 'utf8');
  } catch (_) { /* fail-silent */ }
}

// ── EventStore handle ───────────────────────────────────────────────────────
let store = null;
let upsertMetricStmt = null;
let upsertLogStmt = null;

function initStore() {
  try {
    const { getStore } = require(path.join(CLAUDE_DIR, 'hooks', '_lib', 'event-store.js'));
    const s = getStore(CLAUDE_DIR);
    if (!s || !s.db) {
      canary({ ts: new Date().toISOString(), level: 'fatal', msg: 'event-store unavailable' });
      return null;
    }
    // Prepared statements (built once, reused per request).
    upsertMetricStmt = s.db.prepare(`
      INSERT INTO claude_code_otel
        (ts_bucket, signal, metric, session_id, model, token_type, sum, count, attrs)
      VALUES (?, 'metric', ?, ?, ?, ?, ?, 1, ?)
      ON CONFLICT(ts_bucket, metric, session_id, model, token_type)
      DO UPDATE SET sum = sum + excluded.sum, count = count + excluded.count
    `);
    upsertLogStmt = s.db.prepare(`
      INSERT INTO claude_code_otel
        (ts_bucket, signal, metric, session_id, model, token_type, sum, count, attrs)
      VALUES (?, 'log', ?, ?, ?, ?, 0, 1, ?)
      ON CONFLICT(ts_bucket, metric, session_id, model, token_type)
      DO UPDATE SET count = count + 1
    `);
    return s;
  } catch (err) {
    canary({ ts: new Date().toISOString(), level: 'fatal', msg: 'store init failed', err: String(err && err.message || err) });
    return null;
  }
}

// ── OTLP/JSON projection ────────────────────────────────────────────────────

/** Extract a single string value from an OTLP attribute KV (anyValue shape). */
function attrValue(kv) {
  if (!kv || !kv.value) return null;
  const v = kv.value;
  if (typeof v.stringValue === 'string') return v.stringValue;
  if (typeof v.intValue !== 'undefined') return String(v.intValue);
  if (typeof v.doubleValue !== 'undefined') return String(v.doubleValue);
  if (typeof v.boolValue !== 'undefined') return String(v.boolValue);
  return null;
}

/** Flatten an attributes[] array into a {key: stringValue} object. */
function flattenAttrs(attrs) {
  const out = {};
  if (!Array.isArray(attrs)) return out;
  for (const kv of attrs) {
    if (kv && typeof kv.key === 'string') {
      const val = attrValue(kv);
      if (val !== null) out[kv.key] = val;
    }
  }
  return out;
}

/** Floor ts (ms epoch) to the start of its containing minute. */
function bucketMs(timeUnixNano) {
  // timeUnixNano can be string or number per protobuf JSON encoding.
  const ms = Number(timeUnixNano) / 1e6;
  if (!Number.isFinite(ms)) return Math.floor(Date.now() / 60000) * 60000;
  return Math.floor(ms / 60000) * 60000;
}

/** Extract numeric value from an OTLP datapoint (sum or gauge). */
function pointValue(dp) {
  if (typeof dp.asDouble === 'number') return dp.asDouble;
  if (typeof dp.asInt !== 'undefined') return Number(dp.asInt);
  return 0;
}

/** Process one metric's datapoints. Returns number of rows upserted. */
function projectMetric(metric) {
  if (!metric || typeof metric.name !== 'string') return 0;
  const points = (metric.sum && metric.sum.dataPoints)
    || (metric.gauge && metric.gauge.dataPoints)
    || [];
  if (!Array.isArray(points) || points.length === 0) return 0;

  let written = 0;
  for (const dp of points) {
    try {
      const attrs = flattenAttrs(dp.attributes);
      const bucket = bucketMs(dp.timeUnixNano);
      const sessionId = attrs['session.id'] || null;
      const model = attrs.model || null;
      const tokenType = attrs.type || null; // only present on claude_code.token.usage
      const sum = pointValue(dp);
      // Keep remaining attrs as JSON (drop projected keys to avoid duplication).
      const remaining = { ...attrs };
      delete remaining['session.id'];
      delete remaining.model;
      delete remaining.type;
      upsertMetricStmt.run(
        bucket,
        metric.name,
        sessionId,
        model,
        tokenType,
        sum,
        JSON.stringify(remaining)
      );
      written += 1;
    } catch (err) {
      canary({ ts: new Date().toISOString(), level: 'warn', route: '/v1/metrics', msg: 'datapoint failed', err: String(err && err.message || err).slice(0, 200) });
    }
  }
  return written;
}

/** Walk OTLP metrics body. Returns total rows upserted. */
function projectMetrics(body) {
  let total = 0;
  const resourceMetrics = body && body.resourceMetrics;
  if (!Array.isArray(resourceMetrics)) return 0;
  for (const rm of resourceMetrics) {
    const scopeMetrics = rm && rm.scopeMetrics;
    if (!Array.isArray(scopeMetrics)) continue;
    for (const sm of scopeMetrics) {
      const metrics = sm && sm.metrics;
      if (!Array.isArray(metrics)) continue;
      for (const m of metrics) total += projectMetric(m);
    }
  }
  return total;
}

/** Walk OTLP logs body. Logs are less critical — just don't crash. */
function projectLogs(body) {
  let total = 0;
  const resourceLogs = body && body.resourceLogs;
  if (!Array.isArray(resourceLogs)) return 0;
  for (const rl of resourceLogs) {
    const scopeLogs = rl && rl.scopeLogs;
    if (!Array.isArray(scopeLogs)) continue;
    for (const sl of scopeLogs) {
      const records = sl && sl.logRecords;
      if (!Array.isArray(records)) continue;
      for (const lr of records) {
        try {
          const attrs = flattenAttrs(lr.attributes);
          const bucket = bucketMs(lr.timeUnixNano || lr.observedTimeUnixNano);
          const sessionId = attrs['session.id'] || null;
          const model = attrs.model || null;
          const metricName = (lr.body && typeof lr.body.stringValue === 'string')
            ? lr.body.stringValue
            : 'log';
          upsertLogStmt.run(
            bucket,
            metricName,
            sessionId,
            model,
            null, // token_type
            JSON.stringify(attrs)
          );
          total += 1;
        } catch (err) {
          canary({ ts: new Date().toISOString(), level: 'warn', route: '/v1/logs', msg: 'logRecord failed', err: String(err && err.message || err).slice(0, 200) });
        }
      }
    }
  }
  return total;
}

// ── HTTP request handling ───────────────────────────────────────────────────

async function readJsonBody(req) {
  // Bun's Request has .json() — use it when available, else fall back to text.
  if (typeof req.json === 'function') {
    return await req.json();
  }
  const text = await req.text();
  return JSON.parse(text);
}

async function handle(req) {
  const url = new URL(req.url);
  const route = url.pathname;
  const method = req.method;

  if (method === 'GET' && route === '/healthz') {
    return new Response('ok', { status: 200, headers: { 'content-type': 'text/plain' } });
  }

  if (method !== 'POST' || (route !== '/v1/metrics' && route !== '/v1/logs')) {
    return new Response('not found', { status: 404 });
  }

  const t0 = Date.now();
  let body;
  try {
    body = await readJsonBody(req);
  } catch (err) {
    canary({ ts: new Date().toISOString(), level: 'error', route, msg: 'parse failed', err: String(err && err.message || err).slice(0, 200) });
    return new Response('bad request', { status: 400 });
  }

  let count = 0;
  try {
    count = route === '/v1/metrics' ? projectMetrics(body) : projectLogs(body);
  } catch (err) {
    canary({ ts: new Date().toISOString(), level: 'error', route, msg: 'project failed', err: String(err && err.message || err).slice(0, 200) });
    return new Response('internal error', { status: 500 });
  }

  canary({
    ts: new Date().toISOString(),
    route,
    count,
    latency_ms: Date.now() - t0,
  });
  return new Response(JSON.stringify({ partialSuccess: {} }), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  });
}

// ── Lifecycle ───────────────────────────────────────────────────────────────

function shutdown(signal) {
  canary({ ts: new Date().toISOString(), level: 'info', msg: 'shutdown', signal });
  try { if (store && typeof store.close === 'function') store.close(); } catch (_) {}
  process.exit(0);
}

process.on('SIGTERM', () => shutdown('SIGTERM'));
process.on('SIGINT', () => shutdown('SIGINT'));

// ── Main ────────────────────────────────────────────────────────────────────

// Runtime check FIRST so a Node-only consumer sees a clear "wrong runtime"
// signal in canary, not a misleading SQLite-driver failure from the
// runtime-shim returning null in initStore().
// eslint-disable-next-line no-undef
if (typeof Bun === 'undefined' || typeof Bun.serve !== 'function') {
  canary({ ts: new Date().toISOString(), level: 'fatal', msg: 'Bun.serve unavailable — collector requires Bun runtime' });
  process.exit(1);
}

store = initStore();
if (!store) {
  // initStore already logged to canary.
  process.exit(1);
}

// eslint-disable-next-line no-undef
let server;
try {
  server = Bun.serve({
    hostname: '127.0.0.1', // loopback only — never expose to network
    port: PORT,
    fetch: handle,
    error(err) {
      canary({ ts: new Date().toISOString(), level: 'error', msg: 'serve error', err: String(err && err.message || err).slice(0, 200) });
      return new Response('internal error', { status: 500 });
    },
  });
} catch (err) {
  // EADDRINUSE (another collector already bound), EACCES (privileged port),
  // or any other bind failure. Without this catch, Bun.serve would throw
  // uncaught and exit without writing canary — leaving harness-init looking
  // like it never spawned anything.
  canary({
    ts: new Date().toISOString(),
    level: 'fatal',
    msg: 'Bun.serve failed to bind',
    port: PORT,
    code: (err && err.code) || null,
    err: String((err && err.message) || err).slice(0, 200),
  });
  try { if (store && typeof store.close === 'function') store.close(); } catch (_) {}
  process.exit(1);
}

canary({
  ts: new Date().toISOString(),
  level: 'info',
  msg: 'collector listening',
  host: '127.0.0.1',
  port: server.port,
  pid: process.pid,
});
