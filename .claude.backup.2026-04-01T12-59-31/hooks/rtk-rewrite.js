#!/usr/bin/env node
/**
 * RTK REWRITE: PreToolUse hook that rewrites Bash commands through RTK
 *
 * If RTK (Rust Token Killer) is available in PATH, transparently prepends
 * `rtk ` to every Bash command, reducing token consumption by 60-90% on
 * CLI outputs.
 *
 * RTK availability is cached in a temp file (60s TTL) to avoid spawning
 * which/where on every command invocation.
 *
 * Fail-open: exits 0 on any error so Claude is never blocked by this hook.
 *
 * @version 1.0.0
 */

const { execFileSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');

const CACHE_FILE = path.join(os.tmpdir(), 'rtk-available.json');
const CACHE_TTL_MS = 60_000;

/**
 * Commands RTK knows how to optimize. For anything else, pass through
 * unchanged to avoid unnecessary overhead.
 * Source: https://github.com/rtk-ai/rtk (supported commands)
 */
const RTK_COMMANDS = new Set([
  // Git
  'git',
  // Package managers
  'npm', 'pnpm', 'yarn', 'bun', 'cargo', 'pip', 'pip3', 'bundle', 'gem',
  'composer', 'go', 'poetry', 'nuget',
  // Test runners
  'pytest', 'vitest', 'jest', 'mocha', 'rspec', 'rake',
  'playwright', 'cypress', 'nunit3-console', 'xunit.console',
  // Build / lint
  'eslint', 'biome', 'tsc', 'rustc', 'clippy', 'make', 'cmake', 'gradle',
  'mvn', 'dotnet', 'msbuild', 'nuget',
  // Bundlers / dev servers
  'next', 'vite', 'webpack', 'turbo', 'nx', 'lerna', 'esbuild', 'rollup',
  'parcel', 'rspack',
  // CSS / preprocessors
  'tailwindcss', 'sass', 'postcss', 'less',
  // File / search
  'ls', 'tree', 'find', 'grep', 'rg', 'cat', 'head', 'tail', 'wc',
  'diff', 'sort', 'uniq',
  // Network
  'curl', 'wget',
  // Containers
  'docker', 'kubectl', 'podman', 'docker-compose',
  // DB
  'psql', 'mysql', 'sqlite3', 'mongosh',
  // ORM / migration tools
  'prisma', 'drizzle-kit', 'typeorm', 'sequelize',
  // Misc
  'env', 'printenv', 'gh',
]);

/**
 * Returns true if `rtk` is available in PATH, using a cached result when
 * the cache is still within TTL.
 */
function isRtkAvailable() {
  try {
    if (fs.existsSync(CACHE_FILE)) {
      const raw = fs.readFileSync(CACHE_FILE, 'utf8');
      const cached = JSON.parse(raw);
      if (Date.now() - cached.ts < CACHE_TTL_MS) {
        return cached.available;
      }
    }
  } catch (_) {
    // Cache read failed — fall through to fresh check
  }

  let available = false;
  try {
    if (process.platform === 'win32') {
      execFileSync('where', ['rtk'], { stdio: 'ignore' });
    } else {
      execFileSync('which', ['rtk'], { stdio: 'ignore' });
    }
    available = true;
  } catch (_) {
    available = false;
  }

  try {
    fs.writeFileSync(CACHE_FILE, JSON.stringify({ available, ts: Date.now() }), 'utf8');
  } catch (_) {
    // Cache write failed — non-fatal
  }

  return available;
}

/**
 * Extracts the base command name from a shell command string.
 * Handles: env vars (FOO=bar cmd), paths (/usr/bin/cmd), sudo, npx/bunx wrappers.
 */
function extractBaseCommand(cmd) {
  const trimmed = cmd.trim();
  if (!trimmed) return null;

  // Split on first pipe/semicolon/&& to get the first command
  const firstCmd = trimmed.split(/[|;&]/)[0].trim();

  // Tokenize respecting quotes
  const tokens = firstCmd.match(/(?:[^\s"']+|"[^"]*"|'[^']*')+/g) || [];

  for (const token of tokens) {
    // Skip env variable assignments (FOO=bar)
    if (/^[A-Za-z_]\w*=/.test(token)) continue;
    // Skip sudo/env prefixes
    if (token === 'sudo' || token === 'env') continue;
    // Skip npx/bunx — let the actual command through
    if (token === 'npx' || token === 'bunx') continue;
    // Extract basename from paths (/usr/bin/git → git)
    const base = path.basename(token);
    return base;
  }
  return null;
}

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => (input += chunk));
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    const cmd = data.tool_input?.command || '';

    // Already prefixed or RTK not available — pass through
    if (cmd.startsWith('rtk ') || !isRtkAvailable()) {
      process.exit(0);
    }

    // Extract the base command (first word, ignoring env vars and paths)
    const baseCmd = extractBaseCommand(cmd);
    if (!baseCmd || !RTK_COMMANDS.has(baseCmd)) {
      process.exit(0);
    }

    console.log(JSON.stringify({
      hookSpecificOutput: {
        hookEventName: 'PreToolUse',
        permissionDecision: 'allow',
        updatedInput: { command: 'rtk ' + cmd }
      }
    }));
    process.exit(0);
  } catch (err) {
    process.stderr.write(`[rtk-rewrite] Error: ${err.message}\n`);
    process.exit(0);
  }
});
