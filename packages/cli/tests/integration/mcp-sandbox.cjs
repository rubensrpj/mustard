#!/usr/bin/env bun
'use strict';

/**
 * AC #8: server is read-only — no write tools exposed.
 *
 * Two assertions:
 *   1. tools/list contains zero tools whose name matches /append|write|set|delete|update|insert/i.
 *   2. Calling `append_event` (or any unknown write-shaped tool) returns a
 *      JSON-RPC error (method/tool not found OR invalid params).
 *
 * Run:  node tests/integration/mcp-sandbox.js
 */

const assert = require('node:assert');
const {
  McpClient, makeFixture, runMigration, writeKnowledge, cleanup,
} = require('./mcp-helpers.cjs');

const WRITE_PATTERN = /^(append|write|set|delete|update|insert|create|remove|put|patch)/i;

async function main() {
  const fix = makeFixture('sandbox');
  // Seed empty knowledge so migration produces a valid DB.
  writeKnowledge(fix, []);
  runMigration(fix);

  const client = new McpClient(fix.dbPath);
  try {
    await client.initialize();

    // (1) Inventory tools — assert no write-shaped names.
    const tools = await client.listTools();
    assert.ok(Array.isArray(tools.tools), 'tools/list must return { tools: [] }');
    const writeTools = tools.tools.filter((t) => WRITE_PATTERN.test(t.name));
    assert.strictEqual(writeTools.length, 0, 'write-shaped tools exposed: ' + writeTools.map((t) => t.name).join(','));

    // (2) Call an unregistered write tool — SDK returns isError:true with a
    // "Tool ... not found" content payload (MCP doesn't surface as JSON-RPC
    // error). Either path counts as rejection.
    let rejected = false;
    try {
      const r = await client.callTool('append_event', { ts: 'now', event: 'evil', payload: {} });
      if (r && r.isError === true) rejected = true;
    } catch (_) {
      rejected = true;
    }
    assert.ok(rejected, 'append_event call must be rejected (isError or JSON-RPC error)');

    console.log('PASS mcp-sandbox: ' + tools.tools.length + ' read-only tools, write call rejected');
  } finally {
    client.close();
    cleanup(fix);
  }
}

main().catch((err) => { console.error('FAIL', err.message); process.exit(1); });
