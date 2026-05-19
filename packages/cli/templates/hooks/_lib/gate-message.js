'use strict';
/**
 * gate-message — shared helper for legible gate block/warn messages.
 *
 * Every gate hook emits a message in a single standard shape so a reader
 * always knows: what was violated, why it matters, and how to get past it.
 *
 *   [GATE] {what}. {why}. Saída: {how to bypass / fix}.
 *
 * Keep messages short (1-3 lines). A long message on every block becomes
 * noise — legibility, not verbosity.
 *
 * Fail-open: pure string assembly, defensive against missing/empty inputs.
 * Never throws — a gate calling this can always rely on getting a string.
 */

/** Coerce any value to a trimmed string; empty/invalid → ''. */
function clean(value) {
  if (value === null || value === undefined) return '';
  try {
    return String(value).trim();
  } catch (_) {
    return '';
  }
}

/**
 * Format a gate message.
 *
 * @param {object} parts
 * @param {string} [parts.gate]  Gate label, e.g. "Close Gate" (defaults to "GATE").
 * @param {string} parts.what    What was violated.
 * @param {string} [parts.why]   Why it matters (optional).
 * @param {string} [parts.exit]  How to bypass or fix — env var or concrete action (optional).
 * @returns {string}
 */
function formatGateMessage(parts) {
  const p = parts && typeof parts === 'object' ? parts : {};
  const gate = clean(p.gate) || 'GATE';
  const what = clean(p.what);
  const why = clean(p.why);
  const exit = clean(p.exit);

  // Each segment ends with a period; assemble only the segments present.
  const seg = [];
  if (what) seg.push(what);
  if (why) seg.push(why);

  let body = seg.join('. ');
  if (body && !/[.!?…]$/.test(body)) body += '.';

  let msg = `[${gate}] ${body}`.trim();
  if (exit) {
    let tail = exit;
    if (!/[.!?…]$/.test(tail)) tail += '.';
    msg += ` Saída: ${tail}`;
  }
  return msg;
}

module.exports = { formatGateMessage };
