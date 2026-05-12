#!/usr/bin/env node
/**
 * Synthetic integration: span-emitter wrapper + subagent-tracker span flow.
 *
 * The full hook flow has heavy setup (pipeline state files, harness wiring,
 * spawning the script via stdin/stdout). To keep this fast and deterministic
 * we instead exercise the wrapper + TokenTracker directly with the SAME
 * inputs the hook would compute. This proves the integration contract:
 *
 *   1. span-emitter wrapper resolves the compiled TokenTracker from dist/.
 *   2. startSpan persists `.active-spans/{toolUseId}.json` next to spans.jsonl.
 *   3. endSpan emits a one-line OTLP wrapper into spans.jsonl with the
 *      required gen_ai.* + mustard.* attributes, then removes the sidecar.
 *
 * Run: node --test tests/integration/subagent-tracker-spans.test.js
 */

import test from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createRequire } from 'node:module';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const REPO_ROOT = path.resolve(__dirname, '..', '..');
const require = createRequire(import.meta.url);

function tmpClaudeDir() {
  const d = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-tracker-spans-'));
  const claudeDir = path.join(d, '.claude');
  fs.mkdirSync(claudeDir, { recursive: true });
  return claudeDir;
}

function findAttr(attrs, key) {
  return attrs.find((a) => a && a.key === key);
}

test('span-emitter wrapper loads compiled TokenTracker and emits one OTLP span', () => {
  // Fresh require — module caches a singleton; reset by deleting from cache.
  const wrapperPath = path.join(
    REPO_ROOT,
    'templates',
    'hooks',
    '_lib',
    'span-emitter.js'
  );
  delete require.cache[require.resolve(wrapperPath)];
  const { getTracker } = require(wrapperPath);

  const claudeDir = tmpClaudeDir();
  const tracker = getTracker(claudeDir);
  assert.ok(tracker, 'wrapper must resolve TokenTracker from dist/');

  const toolUseId = 'toolu_subagent_001';
  tracker.startSpan({
    name: 'task.dispatch',
    toolUseId,
    model: 'claude-opus-4-7',
    agentType: 'general-purpose',
    spec: '2026-05-12-test-spec',
    phase: 'EXECUTE',
    wave: 2,
    promptBytes: 2048,
  });

  const sidecar = path.join(claudeDir, '.harness', '.active-spans', `${toolUseId}.json`);
  assert.equal(fs.existsSync(sidecar), true, 'sidecar written by startSpan');

  // Spin briefly so duration is measurable (≥1 ms).
  const t0 = Date.now();
  while (Date.now() - t0 < 2) { /* spin */ }

  tracker.endSpan({ toolUseId, responseBytes: 512 });

  assert.equal(fs.existsSync(sidecar), false, 'sidecar removed by endSpan');

  const spansJsonl = path.join(claudeDir, '.harness', 'spans.jsonl');
  assert.equal(fs.existsSync(spansJsonl), true, 'spans.jsonl created');
  const lines = fs.readFileSync(spansJsonl, 'utf8').trim().split('\n').filter(Boolean);
  assert.equal(lines.length, 1, 'one OTLP wrapper line emitted');

  const span = JSON.parse(lines[0]).resourceSpans[0].scopeSpans[0].spans[0];
  const a = span.attributes;
  assert.ok(findAttr(a, 'gen_ai.system'), 'gen_ai.system present');
  assert.ok(findAttr(a, 'gen_ai.request.model'), 'gen_ai.request.model present');
  assert.ok(findAttr(a, 'gen_ai.usage.input_tokens'), 'input_tokens present');
  assert.ok(findAttr(a, 'gen_ai.usage.output_tokens'), 'output_tokens present');
  assert.ok(findAttr(a, 'mustard.spec'), 'mustard.spec present');
  assert.ok(findAttr(a, 'mustard.phase'), 'mustard.phase present');
});

test('migration projects spans.jsonl into spans table when Bun present (skipped on Node)', async () => {
  // The migration uses bun:sqlite — skip cleanly under plain Node.
  if (!process.versions.bun) {
    return; // soft skip: no Bun runtime here.
  }
  const migrateMod = await import(
    pathToFileURL(path.join(REPO_ROOT, 'dist', 'migrate', 'jsonl-to-sqlite.js')).href
  );
  const claudeDir = tmpClaudeDir();
  const harnessDir = path.join(claudeDir, '.harness');
  fs.mkdirSync(harnessDir, { recursive: true });

  // Seed a real OTLP line by exercising TokenTracker once.
  const trackerMod = await import(
    pathToFileURL(path.join(REPO_ROOT, 'dist', 'telemetry', 'token-tracker.js')).href
  );
  const tt = new trackerMod.TokenTracker(path.join(harnessDir, 'spans.jsonl'));
  tt.startSpan({
    name: 'task.dispatch',
    toolUseId: 'mig_001',
    model: 'claude-haiku-4-5',
    agentType: 'Explore',
    promptBytes: 100,
  });
  tt.endSpan({ toolUseId: 'mig_001', responseBytes: 50 });

  const result = migrateMod.migrate(harnessDir);
  assert.equal(result.spansImported, 1, 'one span projected into spans table');
});
