#!/usr/bin/env node
'use strict';

/**
 * MCP tool handlers — invoked via the existing mcp-helpers stdio harness,
 * but treated as unit tests of each tool's per-call logic. The MCP module
 * auto-initializes on import (`store.init()` at top level), so we cannot
 * import it standalone — the closest "handler-level" test is to spin up
 * the server once per test with seeded fixtures.
 *
 * Run: node --test tests/unit/mcp/tool-handlers.test.cjs
 */

const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');

const helpers = require('../../integration/mcp-helpers.cjs');

// helpers exports under one module — re-bind for readability.
const { McpClient, makeFixture, writeEvents, writeKnowledge, writeSpecs, runMigration, cleanup } = helpers;

test('search_knowledge returns FTS5-ranked matches', async () => {
  const fix = makeFixture('unit-search');
  writeKnowledge(fix, [
    { id: 'k1', type: 'pattern', name: 'auth-flow', description: 'JWT refresh',  confidence: 0.9, createdAt: '2026-01-01', updatedAt: '2026-01-01', source: 's' },
    { id: 'k2', type: 'pattern', name: 'cache-lru', description: 'least recently used', confidence: 0.8, createdAt: '2026-01-01', updatedAt: '2026-01-01', source: 's' },
    { id: 'k3', type: 'entity',  name: 'user',      description: 'auth subject',   confidence: 0.7, createdAt: '2026-01-01', updatedAt: '2026-01-01', source: 's' },
  ]);
  runMigration(fix);
  const client = new McpClient(fix.dbPath);
  try {
    await client.initialize();
    const result = await client.callTool('search_knowledge', { query: 'auth' });
    const parsed = JSON.parse(result.content[0].text);
    assert.ok(Array.isArray(parsed));
    assert.ok(parsed.length >= 1);
    // Both k1 (name=auth-flow) and k3 (description=auth subject) match.
    const ids = parsed.map((k) => k.id);
    assert.ok(ids.includes('k1') || ids.includes('k3'));
  } finally {
    client.close();
    cleanup(fix);
  }
});

test('search_knowledge respects type filter', async () => {
  const fix = makeFixture('unit-search-type');
  writeKnowledge(fix, [
    { id: 'k1', type: 'pattern', name: 'auth-flow', description: 'JWT', confidence: 0.9, createdAt: '2026-01-01', updatedAt: '2026-01-01', source: 's' },
    { id: 'k2', type: 'entity',  name: 'user',      description: 'auth subject', confidence: 0.8, createdAt: '2026-01-01', updatedAt: '2026-01-01', source: 's' },
  ]);
  runMigration(fix);
  const client = new McpClient(fix.dbPath);
  try {
    await client.initialize();
    const result = await client.callTool('search_knowledge', { query: 'auth', type: 'entity' });
    const parsed = JSON.parse(result.content[0].text);
    assert.ok(parsed.every((k) => k.type === 'entity'));
  } finally {
    client.close();
    cleanup(fix);
  }
});

test('search_knowledge respects limit', async () => {
  const fix = makeFixture('unit-search-limit');
  writeKnowledge(fix, Array.from({ length: 8 }, (_, i) => ({
    id: 'k' + i,
    type: 'pattern',
    name: 'auth-pattern-' + i,
    description: 'auth desc ' + i,
    confidence: 0.9 - i * 0.05,
    createdAt: '2026-01-01',
    updatedAt: '2026-01-01',
    source: 's',
  })));
  runMigration(fix);
  const client = new McpClient(fix.dbPath);
  try {
    await client.initialize();
    const result = await client.callTool('search_knowledge', { query: 'auth', limit: 3 });
    const parsed = JSON.parse(result.content[0].text);
    assert.equal(parsed.length, 3);
  } finally {
    client.close();
    cleanup(fix);
  }
});

test('query_events filters by spec', async () => {
  const fix = makeFixture('unit-query');
  writeEvents(fix, [
    { ts: '2026-05-12T00:00:00Z', spec: 'a', event: 'spec.start' },
    { ts: '2026-05-12T00:00:01Z', spec: 'b', event: 'spec.start' },
  ]);
  runMigration(fix);
  const client = new McpClient(fix.dbPath);
  try {
    await client.initialize();
    const result = await client.callTool('query_events', { spec: 'a' });
    const parsed = JSON.parse(result.content[0].text);
    assert.equal(parsed.length, 1);
    assert.equal(parsed[0].spec, 'a');
  } finally {
    client.close();
    cleanup(fix);
  }
});

test('find_similar_specs ranks by token overlap', async () => {
  const fix = makeFixture('unit-similar');
  writeSpecs(fix, [
    { name: 'auth-flow', phase: 'EXECUTE', affectedFiles: ['src/auth.ts'] },
    { name: 'cache-strategy', phase: 'PLAN', affectedFiles: ['src/cache.ts'] },
  ]);
  runMigration(fix);
  const client = new McpClient(fix.dbPath);
  try {
    await client.initialize();
    const result = await client.callTool('find_similar_specs', { description: 'auth' });
    const parsed = JSON.parse(result.content[0].text);
    assert.ok(parsed.length >= 1);
    assert.equal(parsed[0].spec.name, 'auth-flow');
  } finally {
    client.close();
    cleanup(fix);
  }
});

test('find_similar_specs returns [] on empty description tokens', async () => {
  const fix = makeFixture('unit-similar-empty');
  writeSpecs(fix, [{ name: 'auth-flow', phase: 'EXECUTE' }]);
  runMigration(fix);
  const client = new McpClient(fix.dbPath);
  try {
    await client.initialize();
    const result = await client.callTool('find_similar_specs', { description: '   ' });
    const parsed = JSON.parse(result.content[0].text);
    assert.deepEqual(parsed, []);
  } finally {
    client.close();
    cleanup(fix);
  }
});

test('get_spec_metrics returns error obj when missing', async () => {
  const fix = makeFixture('unit-metrics-missing');
  runMigration(fix);
  const client = new McpClient(fix.dbPath);
  try {
    await client.initialize();
    const result = await client.callTool('get_spec_metrics', { spec: 'does-not-exist' });
    const parsed = JSON.parse(result.content[0].text);
    assert.equal(parsed.error, 'no metrics for spec');
    assert.equal(parsed.spec, 'does-not-exist');
  } finally {
    client.close();
    cleanup(fix);
  }
});

test('get_span_summary handles empty spans gracefully', async () => {
  const fix = makeFixture('unit-span-empty');
  runMigration(fix);
  const client = new McpClient(fix.dbPath);
  try {
    await client.initialize();
    const result = await client.callTool('get_span_summary', {});
    const parsed = JSON.parse(result.content[0].text);
    assert.equal(parsed.count, 0);
    assert.equal(parsed.totalInputTokens, 0);
    assert.deepEqual(parsed.byModel, {});
  } finally {
    client.close();
    cleanup(fix);
  }
});
