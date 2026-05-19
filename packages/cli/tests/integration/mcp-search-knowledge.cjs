#!/usr/bin/env bun
'use strict';

/**
 * AC #2: search_knowledge — query "auth" against a seeded base returns the
 * matching entries; result is parseable JSON with at least one match.
 *
 * Run:  node tests/integration/mcp-search-knowledge.js
 */

const assert = require('node:assert');
const {
  McpClient, makeFixture, writeKnowledge, runMigration, cleanup,
} = require('./mcp-helpers.cjs');

async function main() {
  const fix = makeFixture('search');
  writeKnowledge(fix, [
    { id: '1', type: 'pattern',    name: 'auth-flow-pattern', description: 'JWT auth refresh tokens', confidence: 0.9, createdAt: '2026-01-01', updatedAt: '2026-01-01', source: 'spec' },
    { id: '2', type: 'pattern',    name: 'cache-strategy',     description: 'LRU cache for hot keys',  confidence: 0.8, createdAt: '2026-01-01', updatedAt: '2026-01-01', source: 'spec' },
    { id: '3', type: 'convention', name: 'naming-rule',        description: 'camelCase for fns',        confidence: 0.7, createdAt: '2026-01-01', updatedAt: '2026-01-01', source: 'spec' },
    { id: '4', type: 'entity',     name: 'user-table',         description: 'auth subject row',         confidence: 0.6, createdAt: '2026-01-01', updatedAt: '2026-01-01', source: 'spec' },
    { id: '5', type: 'pattern',    name: 'retry-policy',       description: 'exponential backoff',      confidence: 0.5, createdAt: '2026-01-01', updatedAt: '2026-01-01', source: 'spec' },
  ]);
  runMigration(fix);

  const client = new McpClient(fix.dbPath);
  try {
    await client.initialize();
    const result = await client.callTool('search_knowledge', { query: 'auth' });
    assert.ok(result.content && result.content[0] && result.content[0].text, 'missing content[0].text');
    const parsed = JSON.parse(result.content[0].text);
    assert.ok(Array.isArray(parsed), 'expected array');
    assert.ok(parsed.length >= 1, 'expected ≥1 match for "auth", got ' + parsed.length);
    const ids = parsed.map((k) => k.id);
    assert.ok(ids.includes('1') || ids.includes('4'), 'expected id 1 or 4 in matches: ' + ids.join(','));
    console.log('PASS mcp-search-knowledge: matches=' + parsed.length + ' ids=' + ids.join(','));
  } finally {
    client.close();
    cleanup(fix);
  }
}

main().catch((err) => { console.error('FAIL', err.message); process.exit(1); });
