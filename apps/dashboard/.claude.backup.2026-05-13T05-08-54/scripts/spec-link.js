#!/usr/bin/env node
'use strict';
/**
 * SPEC-LINK: Links a child spec to a parent spec (Wave 7 — parent/child hierarchy).
 *
 * CLI:
 *   node spec-link.js --parent <epic> --child <sub1> --reason "<why split>"
 *
 * Exported:
 *   linkSpec({ parent, child, reason, cwd? }) → boolean
 *
 * Effects:
 *   1. Emits event { event: "spec.link", payload: { parent, child, reason } } to harness log.
 *   2. Updates .pipeline-states/{parent}.json: adds child to children_specs (idempotent).
 *   3. Updates .pipeline-states/{child}.json: sets parent_spec (creates placeholder if missing).
 *
 * Guards:
 *   - Fail-open: exits 0 on any error (warns to stderr).
 *   - Node built-ins only. No npm deps.
 *   - Idempotent: re-linking same parent/child is a no-op.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');

let harnessEvent = null;
try {
  harnessEvent = require('./../hooks/_lib/harness-event.js');
} catch (_) {}

/**
 * Read a pipeline-state file, returning parsed object or null.
 */
function readState(filePath) {
  try {
    if (!fs.existsSync(filePath)) return null;
    return JSON.parse(fs.readFileSync(filePath, 'utf8'));
  } catch (_) {
    return null;
  }
}

/**
 * Write a pipeline-state file safely. Returns true on success.
 */
function writeState(filePath, obj) {
  try {
    const dir = path.dirname(filePath);
    if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(filePath, JSON.stringify(obj, null, 2) + '\n', 'utf8');
    return true;
  } catch (err) {
    process.stderr.write(`[spec-link] warn: could not write ${filePath}: ${err.message}\n`);
    return false;
  }
}

/**
 * Core logic. Returns true if all effects succeeded (partial success also returns true
 * as long as the event was emitted — fail-open design).
 *
 * @param {object} opts
 *   - parent  {string}  Epic spec name
 *   - child   {string}  Child spec name
 *   - reason  {string}  Why split (recorded in event)
 *   - cwd     {string}  Project root (default: process.cwd())
 * @returns {boolean}
 */
function linkSpec(opts) {
  const options = opts || {};
  const parent = typeof options.parent === 'string' ? options.parent.trim() : '';
  const child = typeof options.child === 'string' ? options.child.trim() : '';
  const reason = typeof options.reason === 'string' ? options.reason.trim() : '';
  const cwd = typeof options.cwd === 'string' ? options.cwd : process.cwd();

  if (!parent || !child) {
    process.stderr.write('[spec-link] warn: --parent and --child are required\n');
    return false;
  }

  // ── 1. Emit spec.link event ──────────────────────────────────────────────────
  let emitted = false;
  try {
    if (harnessEvent && typeof harnessEvent.emit === 'function') {
      emitted = harnessEvent.emit(
        'spec.link',
        { parent, child, reason },
        { cwd, actor: { kind: 'script', id: 'spec-link' } }
      );
    }
  } catch (_) {}

  // ── 2. Update parent pipeline-state ─────────────────────────────────────────
  const statesDir = path.join(cwd, '.claude', '.pipeline-states');
  const parentFile = path.join(statesDir, parent + '.json');
  let parentState = readState(parentFile);

  if (parentState === null) {
    // Parent state doesn't exist — create minimal placeholder
    parentState = {
      spec: parent,
      parent_spec: null,
      children_specs: [],
    };
  }

  // Ensure Wave 7 fields exist with safe defaults
  if (!parentState.parent_spec && parentState.parent_spec !== null) parentState.parent_spec = null;
  if (!Array.isArray(parentState.children_specs)) parentState.children_specs = [];

  // Idempotent: only add if not already present
  if (!parentState.children_specs.includes(child)) {
    parentState.children_specs.push(child);
  }

  writeState(parentFile, parentState);

  // ── 3. Update child pipeline-state ──────────────────────────────────────────
  const childFile = path.join(statesDir, child + '.json');
  let childState = readState(childFile);

  if (childState === null) {
    // Create minimal placeholder for child
    childState = {
      spec: child,
      parent_spec: parent,
      children_specs: [],
    };
  } else {
    if (!Array.isArray(childState.children_specs)) childState.children_specs = [];
    childState.parent_spec = parent;
  }

  writeState(childFile, childState);

  return true;
}

module.exports = { linkSpec };

// ── CLI ───────────────────────────────────────────────────────────────────────
if (require.main === module) {
  (function () {
    try {
      const args = process.argv.slice(2);

      function getArg(name) {
        const idx = args.indexOf('--' + name);
        return idx >= 0 ? args[idx + 1] : null;
      }

      const parent = getArg('parent');
      const child = getArg('child');
      const reason = getArg('reason') || '';
      const cwd = getArg('cwd') || process.cwd();

      if (!parent || !child) {
        process.stderr.write('Usage: node spec-link.js --parent <epic> --child <sub> --reason "<text>" [--cwd <path>]\n');
        process.exit(0);
      }

      const ok = linkSpec({ parent, child, reason, cwd });
      if (ok) {
        process.stdout.write(JSON.stringify({ ok: true, parent, child, reason }) + '\n');
      } else {
        process.stdout.write(JSON.stringify({ ok: false, parent, child, reason }) + '\n');
      }
    } catch (err) {
      process.stderr.write(`[spec-link] error: ${err.message}\n`);
    }
    process.exit(0);
  })();
}
