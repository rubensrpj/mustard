'use strict';
/**
 * HOOK-ENV: Shared runtime controls for Mustard hooks.
 * Environment variables:
 *   MUSTARD_HOOK_PROFILE  — minimal | standard (default) | strict
 *   MUSTARD_DISABLED_HOOKS — comma-separated hook names to skip
 * @version 1.0.0
 */

const PROFILES = {
  minimal: new Set(['bash-safety', 'file-guard']),
  standard: null,
  strict: null,
};

function shouldRun(hookName) {
  const profile = (process.env.MUSTARD_HOOK_PROFILE || 'standard').toLowerCase();
  const disabled = (process.env.MUSTARD_DISABLED_HOOKS || '')
    .split(',')
    .map(s => s.trim().toLowerCase())
    .filter(Boolean);

  if (disabled.includes(hookName.toLowerCase())) return false;

  const allowed = PROFILES[profile];
  if (allowed && !allowed.has(hookName.toLowerCase())) return false;

  return true;
}

function isStrictMode() {
  return (process.env.MUSTARD_HOOK_PROFILE || '').toLowerCase() === 'strict';
}

// ── Layer 1: Re-entrancy guard (env flag per hook) ──────────────────
function acquireGuard(hookName) {
  var envKey = 'MUSTARD_HOOK_RUNNING_' + hookName.toUpperCase().replace(/-/g, '_');
  if (process.env[envKey] === '1') return false;
  process.env[envKey] = '1';
  return true;
}

// ── Layer 2: Depth counter ──────────────────────────────────────────
function checkDepth(maxDepth) {
  if (maxDepth === undefined) maxDepth = 3;
  var depth = parseInt(process.env.MUSTARD_HOOK_DEPTH || '0', 10);
  if (depth >= maxDepth) return false;
  process.env.MUSTARD_HOOK_DEPTH = String(depth + 1);
  return true;
}

// ── Layer 3: Self-delegation detection ──────────────────────────────
function isSelfDelegation(data) {
  var parentSession = process.env.MUSTARD_SESSION_ID;
  var childSession = data && data.session_id;
  if (parentSession && childSession && parentSession === childSession) return true;
  // Also detect if task description references hook internals
  var desc = (data && data.tool_input && data.tool_input.description || '').toLowerCase();
  if (desc.includes('subagent-tracker') || desc.includes('hook-env') || desc.includes('hook evaluation')) return true;
  return false;
}

// ── Layer 4: Hook phase gating ──────────────────────────────────────
function isInHookPhase() {
  return process.env.MUSTARD_IN_HOOK_PHASE === '1';
}

// ── Layer 5: Combined guard ─────────────────────────────────────────
function guardedRun(hookName, data, maxDepth) {
  if (!shouldRun(hookName)) return false;
  if (!acquireGuard(hookName)) return false;
  if (!checkDepth(maxDepth)) return false;
  if (isInHookPhase()) return false;
  return true;
}

module.exports = { shouldRun, isStrictMode, acquireGuard, checkDepth, isSelfDelegation, isInHookPhase, guardedRun };
