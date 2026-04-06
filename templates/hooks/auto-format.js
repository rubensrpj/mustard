#!/usr/bin/env node
'use strict';
/**
 * AUTO-FORMAT: PostToolUse hook for Write|Edit
 *
 * Detects file extension and runs the appropriate formatter:
 * - .ts/.tsx/.js/.jsx/.json/.css/.md → npx prettier --write
 * - .cs → dotnet format --include
 *
 * Fail-safe: skips if formatter not available.
 * Synchronous execution (blocks until format completes).
 *
 * @version 1.0.0
 */

const { execSync } = require('child_process');
const path = require('path');
const fs = require('fs');
const { shouldRun } = require('./_lib/hook-env.js');

const PRETTIER_EXTS = new Set(['.ts', '.tsx', '.js', '.jsx', '.json', '.css', '.md', '.html', '.scss']);
const DOTNET_EXTS = new Set(['.cs']);

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    if (!shouldRun('auto-format')) { process.exit(0); }
    const data = JSON.parse(input);
    const tool = data.tool_name || '';

    if (!['Write', 'Edit'].includes(tool)) {
      process.exit(0);
    }

    const filePath = data.tool_input?.file_path || '';
    if (!filePath) {
      process.exit(0);
    }

    // Verify file exists
    if (!fs.existsSync(filePath)) {
      process.exit(0);
    }

    const ext = path.extname(filePath).toLowerCase();
    const cwd = data.cwd || process.cwd();

    if (PRETTIER_EXTS.has(ext)) {
      formatWithPrettier(filePath, cwd);
    } else if (DOTNET_EXTS.has(ext)) {
      formatWithDotnet(filePath, cwd);
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write(`[auto-format] Error: ${err.message}\n`);
    process.exit(0);
  }
});

function formatWithPrettier(filePath, cwd) {
  try {
    // Check if prettier is available (look for config or node_modules)
    const hasPrettier =
      fs.existsSync(path.join(cwd, 'node_modules', '.bin', 'prettier')) ||
      fs.existsSync(path.join(cwd, '.prettierrc')) ||
      fs.existsSync(path.join(cwd, '.prettierrc.js')) ||
      fs.existsSync(path.join(cwd, '.prettierrc.json')) ||
      fs.existsSync(path.join(cwd, 'prettier.config.js'));

    // Also check parent directories (monorepo)
    const parentCwd = path.dirname(cwd);
    const hasParentPrettier =
      fs.existsSync(path.join(parentCwd, 'node_modules', '.bin', 'prettier'));

    if (!hasPrettier && !hasParentPrettier) {
      return;
    }

    execSync(`npx prettier --write "${filePath}"`, {
      cwd,
      stdio: ['pipe', 'pipe', 'pipe'],
      timeout: 15000,
      windowsHide: true,
    });
  } catch {
    // Formatter not available or failed — skip silently
  }
}

function formatWithDotnet(filePath, cwd) {
  try {
    // Find nearest .sln or .csproj
    let searchDir = path.dirname(filePath);
    let projectFile = null;

    for (let i = 0; i < 5; i++) {
      const files = fs.readdirSync(searchDir);
      const sln = files.find(f => f.endsWith('.sln'));
      const csproj = files.find(f => f.endsWith('.csproj'));
      if (sln) {
        projectFile = path.join(searchDir, sln);
        break;
      }
      if (csproj) {
        projectFile = path.join(searchDir, csproj);
        break;
      }
      const parent = path.dirname(searchDir);
      if (parent === searchDir) break;
      searchDir = parent;
    }

    if (!projectFile) return;

    execSync(`dotnet format "${projectFile}" --include "${filePath}" --no-restore`, {
      cwd: path.dirname(projectFile),
      stdio: ['pipe', 'pipe', 'pipe'],
      timeout: 15000,
      windowsHide: true,
    });
  } catch {
    // Formatter not available or failed — skip silently
  }
}
