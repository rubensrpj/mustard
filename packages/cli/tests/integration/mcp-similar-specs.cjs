#!/usr/bin/env bun
'use strict';

/**
 * AC #4: find_similar_specs — query "user authentication flow" against a base
 * with spec "auth-roadmap" returns ≥1 match with score >0.
 *
 * Run:  node tests/integration/mcp-similar-specs.js
 */

const assert = require('node:assert');
const {
  McpClient, makeFixture, writeSpecs, runMigration, cleanup,
} = require('./mcp-helpers.cjs');

async function main() {
  const fix = makeFixture('similar');
  writeSpecs(fix, [
    { specName: 'auth-roadmap',         status: 'active',    phaseName: 'PLAN',    startedAt: '2026-04-01', affectedFiles: ['src/auth/jwt.ts', 'src/user/login.ts'] },
    { specName: 'billing-fix',          status: 'completed', phaseName: 'CLOSE',   startedAt: '2026-03-01', affectedFiles: ['src/billing/charge.ts'] },
    { specName: 'cache-improvements',   status: 'active',    phaseName: 'EXECUTE', startedAt: '2026-04-10', affectedFiles: ['src/cache/lru.ts'] },
  ]);
  runMigration(fix);

  const client = new McpClient(fix.dbPath);
  try {
    await client.initialize();
    const result = await client.callTool('find_similar_specs', { description: 'user authentication flow' });
    const matches = JSON.parse(result.content[0].text);
    assert.ok(Array.isArray(matches), 'expected array');
    assert.ok(matches.length >= 1, 'expected ≥1 match, got ' + matches.length);
    assert.ok(matches[0].score > 0, 'top match score must be >0, got ' + matches[0].score);
    const names = matches.map((m) => m.spec.name);
    assert.ok(names.includes('auth-roadmap'), 'expected auth-roadmap in matches: ' + names.join(','));
    console.log('PASS mcp-similar-specs: matches=' + matches.length + ' top=' + matches[0].spec.name + '@' + matches[0].score);
  } finally {
    client.close();
    cleanup(fix);
  }
}

main().catch((err) => { console.error('FAIL', err.message); process.exit(1); });
