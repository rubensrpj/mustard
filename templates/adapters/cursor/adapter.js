#!/usr/bin/env node
'use strict';
/**
 * CURSOR-ADAPTER: Translates Cursor IDE hook format to Claude Code hook protocol.
 * Reuses existing Mustard hooks without duplication.
 *
 * Usage: node .cursor/hooks/adapter.js <hook-name>
 * Example: node .cursor/hooks/adapter.js bash-safety
 *
 * Cursor sends its own JSON format on stdin; this adapter:
 * 1. Maps Cursor event format → Claude Code hook protocol
 * 2. Spawns the target Mustard hook with translated input
 * 3. Maps the Claude Code response → Cursor response format
 *
 * Status: EXPERIMENTAL — Cursor hook format is not yet standardized.
 * @version 1.0.0
 */

var fs = require('fs');
var path = require('path');
var child_process = require('child_process');

// ── Event mapping ───────────────────────────────────────────────────
var EVENT_MAP = {
  'pre_tool':      'PreToolUse',
  'post_tool':     'PostToolUse',
  'pre_tool_use':  'PreToolUse',
  'post_tool_use': 'PostToolUse',
  'session_start': 'SessionStart',
  'session_end':   'SessionEnd',
  'pre_compact':   'PreCompact',
};

function mapEvent(cursorEvent) {
  if (!cursorEvent) return 'PreToolUse';
  return EVENT_MAP[cursorEvent.toLowerCase()] || cursorEvent;
}

// ── Format translation ──────────────────────────────────────────────
function cursorToClaudeCode(cursorData) {
  return {
    hook_event_name: mapEvent(cursorData.event || cursorData.hook_event_name),
    tool_name: cursorData.tool || cursorData.tool_name || cursorData.action || '',
    tool_input: cursorData.params || cursorData.input || cursorData.tool_input || {},
    cwd: cursorData.workspace || cursorData.cwd || process.cwd(),
    session_id: cursorData.session_id || cursorData.sessionId || '',
  };
}

function claudeCodeToCursor(claudeResponse) {
  if (!claudeResponse) return { action: 'allow' };

  var hook = claudeResponse.hookSpecificOutput || {};

  // PreToolUse response
  if (hook.permissionDecision) {
    return {
      action: hook.permissionDecision === 'allow' ? 'allow' : 'block',
      reason: hook.permissionDecisionReason || '',
      updatedInput: hook.updatedInput || undefined,
    };
  }

  // PostToolUse response
  if (claudeResponse.decision) {
    return {
      action: claudeResponse.decision === 'approve' ? 'allow' : 'block',
      reason: claudeResponse.reason || '',
    };
  }

  // SessionStart/SubagentStart (additionalContext)
  if (hook.additionalContext) {
    return {
      action: 'allow',
      context: hook.additionalContext,
    };
  }

  return { action: 'allow' };
}

// ── Main ────────────────────────────────────────────────────────────
var input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', function (chunk) { input += chunk; });
process.stdin.on('end', function () {
  try {
    var hookName = process.env.MUSTARD_HOOK || process.argv[2];
    if (!hookName) {
      process.stderr.write('[cursor-adapter] No hook name specified. Usage: adapter.js <hook-name>\n');
      process.exit(0);
    }

    // Resolve hook path — look in .claude/hooks/ relative to project
    var cwd = process.cwd();
    var hookPath = path.join(cwd, '.claude', 'hooks', hookName);
    if (!hookPath.endsWith('.js')) hookPath += '.js';

    if (!fs.existsSync(hookPath)) {
      process.stderr.write('[cursor-adapter] Hook not found: ' + hookPath + '\n');
      process.exit(0);
    }

    // Parse Cursor input
    var cursorData = {};
    if (input.trim()) {
      try { cursorData = JSON.parse(input); } catch (e) { cursorData = {}; }
    }

    // Translate to Claude Code format
    var claudeInput = cursorToClaudeCode(cursorData);

    // Spawn the Mustard hook
    var result = child_process.execFileSync(process.execPath, [hookPath], {
      input: JSON.stringify(claudeInput),
      encoding: 'utf8',
      timeout: 15000,
      stdio: ['pipe', 'pipe', 'pipe'],
      env: Object.assign({}, process.env, {
        CLAUDE_PROJECT_DIR: cwd,
      }),
    });

    // Translate response back to Cursor format
    if (result && result.trim()) {
      var claudeResponse = JSON.parse(result.trim());
      var cursorResponse = claudeCodeToCursor(claudeResponse);
      process.stdout.write(JSON.stringify(cursorResponse) + '\n');
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write('[cursor-adapter] ' + err.message + '\n');
    process.exit(0); // fail-open
  }
});
process.stdin.resume();
