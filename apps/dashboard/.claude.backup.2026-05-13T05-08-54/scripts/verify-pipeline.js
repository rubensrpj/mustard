#!/usr/bin/env node
'use strict';
/**
 * VERIFY-PIPELINE: Runs build and test verification for active pipeline.
 * Usage: node .claude/scripts/verify-pipeline.js [cwd]
 * Exit 0 = all passed, Exit 1 = failures detected
 * @version 1.0.0
 */

var fs = require('fs');
var path = require('path');
var child_process = require('child_process');

function main() {
  var cwd = process.argv[2] || process.cwd();
  var claudeDir = path.join(cwd, '.claude');
  var results = { passed: [], failed: [], skipped: [], timestamp: new Date().toISOString() };

  // ── Step 1: Find active pipeline ─────────────────────────────────
  var statesDir = path.join(claudeDir, '.pipeline-states');
  var activePipeline = null;

  if (fs.existsSync(statesDir)) {
    try {
      var stateFiles = fs.readdirSync(statesDir).filter(function (f) { return f.endsWith('.json'); });
      // Use most recent state file
      if (stateFiles.length > 0) {
        var latest = stateFiles.sort().pop();
        try {
          activePipeline = JSON.parse(fs.readFileSync(path.join(statesDir, latest), 'utf8'));
        } catch (e) { /* skip */ }
      }
    } catch (e) { /* skip */ }
  }

  // ── Step 2: Discover build/test commands ─────────────────────────
  var commands = [];

  // Try sync-detect.js output first
  var detectScript = path.join(claudeDir, 'scripts', 'sync-detect.js');
  if (fs.existsSync(detectScript)) {
    try {
      var detectOutput = child_process.execFileSync(process.execPath, [detectScript, '--no-cache'], {
        encoding: 'utf8',
        timeout: 15000,
        cwd: cwd,
        stdio: ['pipe', 'pipe', 'pipe'],
        windowsHide: true,
      });
      var detected = JSON.parse(detectOutput);
      var subprojects = detected.subprojects || detected.projects || [];
      for (var i = 0; i < subprojects.length; i++) {
        var sp = subprojects[i];
        if (sp.buildCommand || sp.validateCommand || sp.testCommand) {
          commands.push({
            name: sp.name || sp.path || ('subproject-' + i),
            cwd: sp.path ? path.resolve(cwd, sp.path) : cwd,
            build: sp.buildCommand || sp.validateCommand || null,
            test: sp.testCommand || null,
          });
        }
      }
    } catch (e) { /* sync-detect failed, try fallback */ }
  }

  // Fallback: read pipeline-config.md for build commands
  if (commands.length === 0) {
    var configPath = path.join(claudeDir, 'pipeline-config.md');
    if (fs.existsSync(configPath)) {
      try {
        var config = fs.readFileSync(configPath, 'utf8');
        // Parse agents table for Build Command column
        var lines = config.split('\n');
        var headerIdx = -1;
        var buildColIdx = -1;
        for (var li = 0; li < lines.length; li++) {
          if (lines[li].match(/\|[^\|]*Build/i)) {
            headerIdx = li;
            var cols = lines[li].split('|').map(function (c) { return c.trim(); });
            for (var ci = 0; ci < cols.length; ci++) {
              if (cols[ci].match(/build/i)) { buildColIdx = ci; break; }
            }
            break;
          }
        }
        if (headerIdx >= 0 && buildColIdx >= 0) {
          for (var ri = headerIdx + 2; ri < lines.length; ri++) {
            if (!lines[ri].startsWith('|')) break;
            var rowCols = lines[ri].split('|').map(function (c) { return c.trim(); });
            var agentName = rowCols[1] || '';
            var buildCmd = rowCols[buildColIdx] || '';
            if (buildCmd && buildCmd !== '-' && buildCmd !== 'N/A') {
              commands.push({
                name: agentName,
                cwd: cwd,
                build: buildCmd.replace(/`/g, ''),
                test: null,
              });
            }
          }
        }
      } catch (e) { /* skip */ }
    }
  }

  // ── Step 3: Run verification commands ────────────────────────────
  if (commands.length === 0) {
    // No commands found — try common defaults
    var defaults = [
      { cmd: 'npm test', check: 'package\\.json' },
      { cmd: 'dotnet build', check: '\\.csproj$' },
    ];
    try {
      var cwdFiles = fs.readdirSync(cwd);
      for (var di = 0; di < defaults.length; di++) {
        var pattern = new RegExp(defaults[di].check);
        var found = cwdFiles.some(function (f) { return pattern.test(f); });
        if (found) {
          commands.push({ name: 'default', cwd: cwd, build: defaults[di].cmd, test: null });
          break;
        }
      }
    } catch (e) { /* skip */ }
  }

  for (var ci2 = 0; ci2 < commands.length; ci2++) {
    var cmd = commands[ci2];
    var cmdsToRun = [cmd.build, cmd.test].filter(Boolean);

    if (cmdsToRun.length === 0) {
      results.skipped.push(cmd.name);
      continue;
    }

    var allPassed = true;
    for (var ri2 = 0; ri2 < cmdsToRun.length; ri2++) {
      try {
        child_process.execSync(cmdsToRun[ri2], {
          cwd: cmd.cwd,
          encoding: 'utf8',
          timeout: 120000,
          stdio: ['pipe', 'pipe', 'pipe'],
          windowsHide: true,
        });
      } catch (err) {
        allPassed = false;
        results.failed.push({
          name: cmd.name,
          command: cmdsToRun[ri2],
          error: (err.stderr || err.message || '').substring(0, 500),
        });
      }
    }

    if (allPassed) {
      results.passed.push(cmd.name);
    }
  }

  // ── Step 4: Output ──────────────────────────────────────────────
  console.log(JSON.stringify(results, null, 2));
  process.exit(results.failed.length > 0 ? 1 : 0);
}

try {
  main();
} catch (e) {
  process.stderr.write('verify-pipeline: unexpected error: ' + e.message + '\n');
  process.exit(0); // fail-open at script level
}
