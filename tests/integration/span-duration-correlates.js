#!/usr/bin/env node
/**
 * AC #6 for Mustard 2.0 Phase 2: span duration correlates positively with
 * input_tokens.
 *
 * Strategy: emit 10 sequential spans with linearly increasing promptBytes
 * (100, 200, ..., 1000) and proportional sleep before endSpan (so larger
 * "prompts" take longer to "complete"). We then parse spans.jsonl, extract
 * (input_tokens, duration_ms) pairs, and compute the Pearson correlation
 * coefficient. We assert r > 0.5 (strong positive) — well above noise.
 *
 * Why a sleep loop instead of a synthetic duration field: TokenTracker is the
 * unit under test and it derives duration from Date.now() at start/end. Forcing
 * real wall-clock separation keeps the test honest.
 *
 * Run: node --test tests/integration/span-duration-correlates.js
 */

import test from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const REPO_ROOT = path.resolve(__dirname, '..', '..');

const trackerMod = await import(
  pathToFileURL(path.join(REPO_ROOT, 'dist', 'telemetry', 'token-tracker.js')).href
);
const { TokenTracker } = trackerMod;

function tmpDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-span-correlation-'));
}

/**
 * Pearson correlation coefficient between two equally-sized numeric arrays.
 * Returns NaN if either array is constant (zero variance).
 */
function pearson(xs, ys) {
  if (xs.length !== ys.length || xs.length < 2) return NaN;
  const n = xs.length;
  const meanX = xs.reduce((a, b) => a + b, 0) / n;
  const meanY = ys.reduce((a, b) => a + b, 0) / n;
  let num = 0;
  let denX = 0;
  let denY = 0;
  for (let i = 0; i < n; i++) {
    const dx = xs[i] - meanX;
    const dy = ys[i] - meanY;
    num += dx * dy;
    denX += dx * dx;
    denY += dy * dy;
  }
  if (denX === 0 || denY === 0) return NaN;
  return num / Math.sqrt(denX * denY);
}

function sleepBusy(ms) {
  // Coarse busy-wait — keeps the test deterministic without async hop.
  const end = Date.now() + ms;
  while (Date.now() < end) { /* spin */ }
}

test('span duration correlates positively with input_tokens (r > 0.5)', () => {
  const dir = tmpDir();
  const spansJsonl = path.join(dir, 'spans.jsonl');
  const tracker = new TokenTracker(spansJsonl);

  const N = 10;
  for (let i = 1; i <= N; i++) {
    const promptBytes = 100 * i; // 100..1000 bytes
    const toolUseId = `sim-${i}`;
    tracker.startSpan({
      name: 'sim.dispatch',
      toolUseId,
      model: 'claude-opus-4-7',
      agentType: 'general-purpose',
      promptBytes,
    });
    // Sleep scales with i: 5ms baseline + 3ms per step → reliably distinct durations.
    sleepBusy(5 + i * 3);
    tracker.endSpan({ toolUseId, responseBytes: 50 });
  }

  const raw = fs.readFileSync(spansJsonl, 'utf8').trim();
  const lines = raw.split('\n').filter(Boolean);
  assert.equal(lines.length, N, `expected ${N} spans emitted`);

  const inputs = [];
  const durations = [];
  for (const line of lines) {
    const wrapper = JSON.parse(line);
    const span = wrapper.resourceSpans[0].scopeSpans[0].spans[0];
    const attrs = span.attributes;
    const inputAttr = attrs.find((a) => a.key === 'gen_ai.usage.input_tokens');
    assert.ok(inputAttr, 'input_tokens attribute present');
    const inputTokens = Number(inputAttr.value.intValue);
    const durationNs = BigInt(span.endTimeUnixNano) - BigInt(span.startTimeUnixNano);
    const durationMs = Number(durationNs / 1_000_000n);
    inputs.push(inputTokens);
    durations.push(durationMs);
  }

  const r = pearson(inputs, durations);
  assert.ok(
    Number.isFinite(r) && r > 0.5,
    `expected Pearson r > 0.5, got ${r} (inputs=${JSON.stringify(inputs)} durations=${JSON.stringify(durations)})`
  );
});
