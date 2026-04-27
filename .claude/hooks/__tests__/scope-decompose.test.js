#!/usr/bin/env node
/**
 * Tests for scripts/scope-decompose.js.
 * Run with: node --test .claude/hooks/__tests__/scope-decompose.test.js
 */

const { describe, it } = require('node:test');
const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const path = require('node:path');

const SCRIPT = path.resolve(__dirname, '..', '..', 'scripts', 'scope-decompose.js');

function runScript(input) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [SCRIPT], {
      stdio: ['pipe', 'pipe', 'pipe'],
    });

    let stdout = '';
    let stderr = '';

    child.stdout.on('data', d => (stdout += d));
    child.stderr.on('data', d => (stderr += d));
    child.on('error', reject);
    child.on('close', code => {
      let parsed = null;
      if (stdout.trim()) {
        try { parsed = JSON.parse(stdout.trim()); } catch (_) {}
      }
      resolve({ code, stdout: stdout.trim(), stderr: stderr.trim(), parsed });
    });

    const payload = typeof input === 'string' ? input : JSON.stringify(input);
    child.stdin.write(payload);
    child.stdin.end();
  });
}

describe('scope-decompose decision logic', () => {
  it('single layer with few files → decompose:false reason:single-layer', async () => {
    const r = await runScript({ fileCount: 3, layerCount: 1, newEntityCount: 0, knowledgeMatches: [] });
    assert.equal(r.code, 0);
    assert.equal(r.parsed.decompose, false);
    assert.equal(r.parsed.reason, 'single-layer');
    assert.equal(r.parsed.signals.fileCount, 3);
    assert.equal(r.parsed.signals.layerCount, 1);
  });

  it('two layers with few files → decompose:true reason:multi-layer', async () => {
    const r = await runScript({ fileCount: 3, layerCount: 2, newEntityCount: 1, knowledgeMatches: [] });
    assert.equal(r.code, 0);
    assert.equal(r.parsed.decompose, true);
    assert.equal(r.parsed.reason, 'multi-layer');
  });

  it('three layers with few files → decompose:true reason:multi-layer', async () => {
    const r = await runScript({ fileCount: 3, layerCount: 3, newEntityCount: 0, knowledgeMatches: [] });
    assert.equal(r.code, 0);
    assert.equal(r.parsed.decompose, true);
    assert.equal(r.parsed.reason, 'multi-layer');
  });

  it('historical knowledge match → decompose:true reason:history-match', async () => {
    const r = await runScript({
      fileCount: 1,
      layerCount: 1,
      newEntityCount: 0,
      knowledgeMatches: [{ id: 'heavy-pipeline-x', type: 'heavy-pipeline', scope: {} }],
    });
    assert.equal(r.code, 0);
    assert.equal(r.parsed.decompose, true);
    assert.match(r.parsed.reason, /^history-match:/);
    assert.equal(r.parsed.reason, 'history-match:heavy-pipeline-x');
    assert.equal(r.parsed.signals.historicalMatches, 1);
  });

  it('wide spec single layer with new entities → decompose:true reason:wide-and-new-entities', async () => {
    const r = await runScript({ fileCount: 15, layerCount: 1, newEntityCount: 2, knowledgeMatches: [] });
    assert.equal(r.code, 0);
    assert.equal(r.parsed.decompose, true);
    assert.equal(r.parsed.reason, 'wide-and-new-entities');
  });

  it('empty input → fail-open decompose:false', async () => {
    const r = await runScript('');
    assert.equal(r.code, 0);
    assert.equal(r.parsed.decompose, false);
    // empty signals defaults: layerCount=0 → single-layer branch
    assert.equal(r.parsed.reason, 'single-layer');
  });

  it('invalid JSON input → error-fallback', async () => {
    const r = await runScript('{not-json');
    assert.equal(r.code, 0);
    assert.equal(r.parsed.decompose, false);
    assert.equal(r.parsed.reason, 'error-fallback');
  });
});
