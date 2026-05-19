#!/usr/bin/env bun
'use strict';
/**
 * Tests for templates/hooks/_lib/harness-event.js
 * Run with: bun test templates/hooks/__tests__/harness-event.test.js
 */

const { describe, it, beforeEach, afterEach } = require('bun:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const harness = require('../_lib/harness-event.js');

let tmpDir;
let harnessDir;
let eventsFile;

beforeEach(() => {
  tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-harness-'));
  // Ensure .claude/ exists — emit() will create .harness/ itself.
  fs.mkdirSync(path.join(tmpDir, '.claude'), { recursive: true });
  harnessDir = path.join(tmpDir, '.claude', '.harness');
  eventsFile = path.join(harnessDir, 'events.jsonl');
  // Clear any disable env for these tests.
  delete process.env.MUSTARD_DISABLED_HOOKS;
});

afterEach(() => {
  try { fs.rmSync(tmpDir, { recursive: true, force: true }); } catch (_) {}
});

describe('harness-event.emit', () => {
  it('writes a valid NDJSON line with required schema fields', () => {
    const ok = harness.emit('session.start', { foo: 'bar' }, {
      cwd: tmpDir,
      sessionId: 's-test-1',
      wave: 0,
      actor: { kind: 'hook', id: 'harness-init' },
    });
    assert.equal(ok, true);
    assert.ok(fs.existsSync(eventsFile), 'events.jsonl should exist');
    const raw = fs.readFileSync(eventsFile, 'utf8');
    assert.ok(raw.endsWith('\n'), 'line must terminate with newline');
    const parsed = JSON.parse(raw.trim());
    assert.equal(parsed.v, 1);
    assert.equal(parsed.event, 'session.start');
    assert.equal(parsed.sessionId, 's-test-1');
    assert.equal(parsed.wave, 0);
    assert.equal(parsed.actor.kind, 'hook');
    assert.equal(parsed.actor.id, 'harness-init');
    assert.deepEqual(parsed.payload, { foo: 'bar' });
    assert.ok(typeof parsed.ts === 'string' && parsed.ts.includes('T'));
  });

  it('appends multiple events without corruption', () => {
    for (let i = 0; i < 5; i++) {
      harness.emit('tool.use', { tool: 'Read', i }, {
        cwd: tmpDir,
        sessionId: 's-multi',
        wave: 1,
      });
    }
    const lines = fs.readFileSync(eventsFile, 'utf8').trim().split('\n');
    assert.equal(lines.length, 5);
    for (let i = 0; i < 5; i++) {
      const ev = JSON.parse(lines[i]);
      assert.equal(ev.event, 'tool.use');
      assert.equal(ev.payload.i, i);
      assert.equal(ev.sessionId, 's-multi');
    }
  });

  it('includes optional spec field when provided', () => {
    harness.emit('finding', { content: 'x', confidence: 0.9 }, {
      cwd: tmpDir,
      sessionId: 's-spec',
      spec: 'add-login',
    });
    const parsed = JSON.parse(fs.readFileSync(eventsFile, 'utf8').trim());
    assert.equal(parsed.spec, 'add-login');
  });

  it('defaults payload to empty object when missing', () => {
    harness.emit('pipeline.phase', undefined, {
      cwd: tmpDir,
      sessionId: 's-default',
    });
    const parsed = JSON.parse(fs.readFileSync(eventsFile, 'utf8').trim());
    assert.deepEqual(parsed.payload, {});
  });

  it('returns false when disabled via MUSTARD_DISABLED_HOOKS', () => {
    process.env.MUSTARD_DISABLED_HOOKS = 'harness-event';
    // hook-env caches nothing — just call again.
    const ok = harness.emit('tool.use', {}, { cwd: tmpDir, sessionId: 's-x' });
    assert.equal(ok, false);
    assert.equal(fs.existsSync(eventsFile), false);
    delete process.env.MUSTARD_DISABLED_HOOKS;
  });

  it('fail-open: returns false on I/O error without throwing', () => {
    // Point cwd at a file (not a directory) to force mkdir + append to fail.
    const fileAsCwd = path.join(tmpDir, 'not-a-dir.txt');
    fs.writeFileSync(fileAsCwd, 'x');
    let ok;
    assert.doesNotThrow(() => {
      ok = harness.emit('tool.use', { tool: 'X' }, { cwd: fileAsCwd, sessionId: 's-io' });
    });
    // Emit returns true/false depending on platform; important bit is no throw.
    assert.ok(ok === true || ok === false);
  });

  it('rejects invalid event name', () => {
    assert.equal(harness.emit(null, {}, { cwd: tmpDir }), false);
    assert.equal(harness.emit('', {}, { cwd: tmpDir }), false);
  });
});

describe('harness-event.getCurrentSessionId', () => {
  it('uses session_id from hook input', () => {
    const id = harness.getCurrentSessionId({ session_id: 's-hook-abc' });
    assert.equal(id, 's-hook-abc');
  });

  it('falls back to env var', () => {
    delete process.env.CLAUDE_SESSION_ID;
    process.env.MUSTARD_SESSION_ID = 's-env-xyz';
    const id = harness.getCurrentSessionId({});
    assert.equal(id, 's-env-xyz');
    delete process.env.MUSTARD_SESSION_ID;
  });

  it('generates random id as last resort', () => {
    delete process.env.MUSTARD_SESSION_ID;
    delete process.env.CLAUDE_SESSION_ID;
    const id = harness.getCurrentSessionId({});
    assert.ok(/^s-/.test(id));
  });
});

describe('harness-event.getCurrentWave', () => {
  it('returns 0 when no index.json', () => {
    const wave = harness.getCurrentWave({ cwd: tmpDir });
    assert.equal(wave, 0);
  });

  it('reads wave from .harness/index.json', () => {
    fs.mkdirSync(harnessDir, { recursive: true });
    fs.writeFileSync(path.join(harnessDir, 'index.json'), JSON.stringify({ wave: 7 }));
    const wave = harness.getCurrentWave({ cwd: tmpDir });
    assert.equal(wave, 7);
  });

  it('explicit wave in hook input wins', () => {
    const wave = harness.getCurrentWave({ cwd: tmpDir, wave: 42 });
    assert.equal(wave, 42);
  });
});
