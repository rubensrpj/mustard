#!/usr/bin/env node
'use strict';
/**
 * MCP-BUDGET: SessionStart hook that warns about excessive MCP tool counts.
 * Advisory only — never blocks.
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const os = require('os');
const { shouldRun } = require('./_lib/hook-env.js');

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    if (!shouldRun('mcp-budget')) { process.exit(0); }

    const data = JSON.parse(input);
    const cwd = data.cwd || process.cwd();

    let serverCount = 0;
    let disabledCount = 0;

    // Count MCP servers from config files (project-level then user-level)
    const mcpConfigs = [
      path.join(cwd, '.claude', 'mcp.json'),
      path.join(os.homedir(), '.claude', 'mcp.json'),
    ];

    for (const configPath of mcpConfigs) {
      try {
        if (fs.existsSync(configPath)) {
          const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
          const servers = config.mcpServers || {};
          serverCount += Object.keys(servers).length;
        }
      } catch (e) { /* skip malformed config */ }
    }

    // Subtract disabled servers from settings files
    const settingsConfigs = [
      path.join(cwd, '.claude', 'settings.json'),
      path.join(os.homedir(), '.claude', 'settings.json'),
    ];

    for (const settingsPath of settingsConfigs) {
      try {
        if (fs.existsSync(settingsPath)) {
          const settings = JSON.parse(fs.readFileSync(settingsPath, 'utf8'));
          const disabled = settings.disabledMcpServers || [];
          disabledCount += disabled.length;
        }
      } catch (e) { /* skip malformed config */ }
    }

    const activeServers = Math.max(0, serverCount - disabledCount);

    const warnings = [];

    if (activeServers > 10) {
      warnings.push(
        activeServers + ' MCP servers active (recommend <10) — each server adds tool descriptions that consume context window tokens'
      );
    }

    // Estimate tools (~8 per server average)
    const estimatedTools = activeServers * 8;
    if (estimatedTools > 80) {
      warnings.push(
        'Estimated ~' + estimatedTools + ' MCP tools active (recommend <80) — consider using disabledMcpServers in settings.json'
      );
    }

    if (warnings.length > 0) {
      console.log(JSON.stringify({
        hookSpecificOutput: {
          hookEventName: 'SessionStart',
          additionalContext: '[MCP Budget Advisory] ' + warnings.join('. ') + '.'
        }
      }));
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write('[mcp-budget] ' + err.message + '\n');
    process.exit(0); // fail-open
  }
});
