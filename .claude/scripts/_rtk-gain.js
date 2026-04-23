'use strict';
/**
 * Shared helper: normalize `rtk gain --all --format json` output.
 *
 * rtk emits { summary: { total_saved, avg_savings_pct, total_input,
 * total_output, total_commands }, daily, weekly, monthly }. Different
 * rtk versions (and earlier mustard scripts) assumed top-level
 * `saved_tokens`/`total_saved` — neither is correct on current rtk.
 * This helper is the single source of truth.
 */

const { execFileSync } = require('child_process');

function getRtkGain(opts) {
  const timeout = (opts && opts.timeout) || 3000;
  let raw;
  try {
    raw = execFileSync('rtk', ['gain', '--all', '--format', 'json'], {
      encoding: 'utf8',
      timeout,
      stdio: ['ignore', 'pipe', 'ignore'],
      windowsHide: true,
    });
  } catch {
    return null;
  }
  let data;
  try {
    data = JSON.parse(raw);
  } catch {
    return null;
  }
  const s = (data && data.summary) || data || {};
  const saved = Number(s.total_saved ?? s.saved_tokens ?? s.savedTokens ?? 0) || 0;
  const original = Number(s.total_input ?? s.total_original ?? 0) || 0;
  const pct = Number(s.avg_savings_pct ?? s.savings_pct ?? s.savingsPct ?? 0) || 0;
  const commands = Number(s.total_commands ?? s.commands ?? 0) || 0;
  if (saved <= 0 && commands <= 0) return null;
  return {
    saved,
    originalTotal: original,
    pct,
    commands,
    byCommand: (data && data.by_command) || null,
    daily: (data && Array.isArray(data.daily)) ? data.daily : [],
    weekly: (data && Array.isArray(data.weekly)) ? data.weekly : [],
  };
}

module.exports = { getRtkGain };
