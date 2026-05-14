'use strict';
/**
 * prompt-cache-detect — pure analyzer for the PREFIX-STABLE / VARIABLE protocol.
 *
 * The agent-prompt template encloses its stable preamble between two HTML
 * comment markers:
 *
 *   <!-- PREFIX-STABLE -->
 *   ...stable block (CONTEXT/REFERENCE/SKILLS/RECIPE/ROLE/EFFICIENCY)...
 *   <!-- VARIABLE -->
 *   ...volatile block (spec slice, diff, retry context, TASK)...
 *
 * The Anthropic API charges 10% of input cost for cache-hit prefixes once a
 * dispatch shares ≥1024 tokens of identical bytes with a recent call. We
 * approximate that threshold with 1024 characters here (one character ≈ one
 * token in mixed prose+code; underestimates rarely, overestimates rarely).
 *
 * No side effects on import. Used by tooling and by future hook
 * `prompt-prefix-emit.js` (not part of Wave 1).
 */

const crypto = require('crypto');

const PREFIX_MARKER = '<!-- PREFIX-STABLE -->';
const VARIABLE_MARKER = '<!-- VARIABLE -->';
const CACHEABLE_MIN_CHARS = 1024;

/**
 * @typedef {object} PromptAnalysis
 * @property {number}  prefix_len        Character count of the stable prefix (0 if no marker).
 * @property {string}  prefix_hash       sha256 hex of the stable prefix ('' if no marker).
 * @property {number}  variable_len      Character count of the variable tail.
 * @property {boolean} prefix_cacheable  true iff marker present AND prefix_len >= 1024.
 */

/**
 * Analyze a rendered agent prompt and report whether its prefix is cacheable.
 *
 * @param {string} text
 * @returns {PromptAnalysis}
 */
function analyzePrompt(text) {
  const s = typeof text === 'string' ? text : '';
  const pIdx = s.indexOf(PREFIX_MARKER);

  if (pIdx < 0) {
    return {
      prefix_len: 0,
      prefix_hash: '',
      variable_len: s.length,
      prefix_cacheable: false,
    };
  }

  const afterMarker = pIdx + PREFIX_MARKER.length;
  const vIdx = s.indexOf(VARIABLE_MARKER, afterMarker);
  const prefixEnd = vIdx < 0 ? s.length : vIdx;
  const prefix = s.slice(0, prefixEnd);
  const variable = vIdx < 0 ? '' : s.slice(vIdx);

  const hash = crypto.createHash('sha256').update(prefix).digest('hex');

  return {
    prefix_len: prefix.length,
    prefix_hash: hash,
    variable_len: variable.length,
    prefix_cacheable: prefix.length >= CACHEABLE_MIN_CHARS,
  };
}

module.exports = {
  analyzePrompt,
  PREFIX_MARKER,
  VARIABLE_MARKER,
  CACHEABLE_MIN_CHARS,
};
