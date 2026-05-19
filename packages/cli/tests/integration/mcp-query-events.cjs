#!/usr/bin/env bun
'use strict';

/**
 * AC #3: query_events — filter {spec, event} returns only rows matching both.
 *
 * Run:  node tests/integration/mcp-query-events.js
 */

const assert = require('node:assert');
const {
  McpClient, makeFixture, writeEvents, runMigration, cleanup,
} = require('./mcp-helpers.cjs');

async function main() {
  const fix = makeFixture('events');
  writeEvents(fix, [
    { ts: '2026-05-01T10:00:00Z', sessionId: 's1', event: 'tool.use', spec: 'telegram-alerting', actor: { kind: 'agent', id: 'a1' }, payload: { tool: 'Grep' } },
    { ts: '2026-05-01T10:01:00Z', sessionId: 's1', event: 'tool.use', spec: 'telegram-alerting', actor: { kind: 'agent', id: 'a1' }, payload: { tool: 'Edit' } },
    { ts: '2026-05-01T10:02:00Z', sessionId: 's1', event: 'tool.use', spec: 'other-spec',        actor: { kind: 'agent', id: 'a2' }, payload: { tool: 'Grep' } },
    { ts: '2026-05-01T10:03:00Z', sessionId: 's1', event: 'agent.start', spec: 'telegram-alerting', actor: { kind: 'agent', id: 'a1' }, payload: {} },
    { ts: '2026-05-01T10:04:00Z', sessionId: 's1', event: 'tool.use', spec: 'telegram-alerting', actor: { kind: 'agent', id: 'a1' }, payload: { tool: 'Read' } },
  ]);
  runMigration(fix);

  const client = new McpClient(fix.dbPath);
  try {
    await client.initialize();
    const result = await client.callTool('query_events', { spec: 'telegram-alerting', event: 'tool.use' });
    const rows = JSON.parse(result.content[0].text);
    assert.ok(Array.isArray(rows), 'expected array');
    assert.ok(rows.length >= 3, 'expected ≥3 matching rows, got ' + rows.length);
    for (const r of rows) {
      assert.strictEqual(r.spec, 'telegram-alerting', 'leaked spec: ' + r.spec);
      assert.strictEqual(r.event, 'tool.use', 'leaked event: ' + r.event);
    }
    console.log('PASS mcp-query-events: rows=' + rows.length);
  } finally {
    client.close();
    cleanup(fix);
  }
}

main().catch((err) => { console.error('FAIL', err.message); process.exit(1); });
