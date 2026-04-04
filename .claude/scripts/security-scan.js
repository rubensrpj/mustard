#!/usr/bin/env node
'use strict';
/**
 * SECURITY-SCAN: Scans project for secrets, env exposure, and security misconfigurations.
 * Usage: node .claude/scripts/security-scan.js [directory] [--json]
 * Exit 0 = clean, Exit 1 = findings detected
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');

// ── Secret Patterns (14) ────────────────────────────────────────────
const SECRET_PATTERNS = [
  { name: 'AWS Access Key',      re: /AKIA[0-9A-Z]{16}/g },
  { name: 'AWS Secret Key',      re: /(?:aws_secret_access_key|aws_secret)\s*[:=]\s*["']?[A-Za-z0-9/+=]{40}/gi },
  { name: 'GitHub Token',        re: /gh[pousr]_[A-Za-z0-9_]{36,255}/g },
  { name: 'GitLab Token',        re: /glpat-[A-Za-z0-9\-_]{20,}/g },
  { name: 'Stripe Secret Key',   re: /sk_(?:live|test)_[A-Za-z0-9]{24,}/g },
  { name: 'Stripe Publishable',  re: /pk_(?:live|test)_[A-Za-z0-9]{24,}/g },
  { name: 'Slack Token',         re: /xox[bpras]-[A-Za-z0-9\-]{10,}/g },
  { name: 'SendGrid Key',        re: /SG\.[A-Za-z0-9_\-]{22,}\.[A-Za-z0-9_\-]{43}/g },
  { name: 'Twilio SID',          re: /AC[a-f0-9]{32}/g },
  { name: 'Private Key',         re: /-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----/g },
  { name: 'JWT Token',           re: /eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}/g },
  { name: 'Connection String',   re: /(?:Server|Data Source|Host)=[^;\n]+;[^;\n]*(?:Password|Pwd)=[^;\n]+/gi },
  { name: 'Bearer Token Hardcoded', re: /["']Bearer\s+[A-Za-z0-9_\-\.]{20,}["']/g },
  { name: 'Generic Secret Assignment', re: /(?:secret|password|passwd|api_key|apikey|token|auth_token)\s*[:=]\s*["'][^"']{8,}["']/gi },
];

// File name patterns that commonly trigger false positives on generic patterns
// (seeds with hashed passwords, error code constants, test fixtures, etc.)
const FP_FILE_PATTERNS = [
  /[Ss]eeder/,          // DatabaseSeeder.cs, UserSeeder.cs
  /[Ss]eed[s]?\./,      // Seeds.cs, seed.ts
  /ErrorCode/i,         // ApiExceptionErrorCodes.cs, ErrorCodes.ts
  /Exception.*Code/i,   // ExceptionCodes, ExceptionErrorCodes
  /\.d\.ts$/,           // Type declaration files
  /\.test\./,           // Test files
  /\.spec\./,           // Spec files
];

// ── Ignore lists ────────────────────────────────────────────────────
const IGNORE_DIRS = new Set([
  'node_modules', '.git', 'dist', 'bin', 'obj', '.next', 'vendor',
  '__pycache__', '.nuxt', '.output', 'build', 'coverage', '.claude',
  'migrations', '.vs', '.idea', 'packages',
]);

const SCAN_EXTS = new Set([
  '.js', '.ts', '.jsx', '.tsx', '.json', '.yaml', '.yml',
  '.env', '.cs', '.py', '.go', '.rb', '.sh', '.cfg', '.conf',
  '.ini', '.toml', '.xml', '.properties', '.tf', '.tfvars',
]);

const MAX_FILE_SIZE = 512 * 1024; // 512KB — skip large files
const MAX_DEPTH = 8;

// ── Scanner ─────────────────────────────────────────────────────────
function scanDir(dir, results, depth) {
  if (depth === undefined) depth = 0;
  if (depth > MAX_DEPTH) return;
  let entries;
  try { entries = fs.readdirSync(dir, { withFileTypes: true }); }
  catch { return; }

  for (const entry of entries) {
    if (entry.name.startsWith('.') && entry.name !== '.env' && entry.name !== '.env.local' && entry.name !== '.env.production') continue;
    if (IGNORE_DIRS.has(entry.name)) continue;
    const fullPath = path.join(dir, entry.name);

    if (entry.isDirectory()) {
      scanDir(fullPath, results, depth + 1);
    } else if (SCAN_EXTS.has(path.extname(entry.name).toLowerCase()) || entry.name.startsWith('.env')) {
      scanFile(fullPath, results);
    }
  }
}

function scanFile(filePath, results) {
  let stat;
  try { stat = fs.statSync(filePath); } catch { return; }
  if (stat.size > MAX_FILE_SIZE) return;

  let content;
  try { content = fs.readFileSync(filePath, 'utf8'); } catch { return; }

  // Check if file matches false-positive suppression patterns
  const baseName = path.basename(filePath);
  const isFpFile = FP_FILE_PATTERNS.some(re => re.test(baseName));

  // Secret pattern matching
  for (const { name, re } of SECRET_PATTERNS) {
    re.lastIndex = 0;
    const match = re.exec(content);
    if (match) {
      // Skip generic patterns on known false-positive files
      if (isFpFile && name === 'Generic Secret Assignment') continue;
      // Find line number
      const beforeMatch = content.substring(0, match.index);
      const line = (beforeMatch.match(/\n/g) || []).length + 1;
      results.secrets.push({
        file: filePath,
        pattern: name,
        line,
        preview: match[0].substring(0, 8) + '...',
      });
    }
  }

  results.filesScanned++;
}

// ── .env exposure check ─────────────────────────────────────────────
function checkEnvExposure(cwd, results) {
  const envFiles = ['.env', '.env.local', '.env.production', '.env.staging'];
  const gitignorePath = path.join(cwd, '.gitignore');
  let gitignoreContent = '';
  try { gitignoreContent = fs.readFileSync(gitignorePath, 'utf8'); } catch {}

  for (const envFile of envFiles) {
    const envPath = path.join(cwd, envFile);
    if (fs.existsSync(envPath)) {
      // Check if in .gitignore
      const isIgnored = gitignoreContent.split('\n').some(function(line) {
        const trimmed = line.trim();
        return trimmed === envFile || trimmed === '.env*' || trimmed === '.env' || trimmed === '/' + envFile;
      });
      if (!isIgnored) {
        results.envExposure.push({
          file: envFile,
          issue: envFile + ' exists but is NOT in .gitignore — may be committed to repo',
        });
      }
    }
  }
}

// ── Hook permission check ───────────────────────────────────────────
function checkHookPermissions(cwd, results) {
  const settingsPath = path.join(cwd, '.claude', 'settings.json');
  if (!fs.existsSync(settingsPath)) return;

  let settings;
  try { settings = JSON.parse(fs.readFileSync(settingsPath, 'utf8')); } catch { return; }

  // Check for overly broad allow patterns
  const allows = (settings.permissions && settings.permissions.allow) || [];
  for (const rule of allows) {
    if (typeof rule === 'string') {
      if (rule.includes('rm -rf') || rule.includes('--force') || rule.includes('chmod 777')) {
        results.permissions.push({
          rule,
          issue: 'Dangerous command pattern allowed in permissions',
        });
      }
    }
  }
}

// ── Main ────────────────────────────────────────────────────────────
function main() {
  const args = process.argv.slice(2);
  const jsonOutput = args.includes('--json');
  const cwd = args.find(function(a) { return !a.startsWith('--'); }) || process.cwd();

  const results = {
    secrets: [],
    envExposure: [],
    permissions: [],
    filesScanned: 0,
    scanDir: cwd,
    timestamp: new Date().toISOString(),
  };

  scanDir(cwd, results);
  checkEnvExposure(cwd, results);
  checkHookPermissions(cwd, results);

  const totalFindings = results.secrets.length + results.envExposure.length + results.permissions.length;

  if (jsonOutput) {
    console.log(JSON.stringify(results, null, 2));
  } else {
    // Human-readable output
    console.log('\nSecurity Scan -- ' + results.filesScanned + ' files scanned');
    console.log('--------------------------------------------------');

    if (results.secrets.length > 0) {
      console.log('\n[CRITICAL] SECRETS DETECTED (' + results.secrets.length + '):');
      for (const s of results.secrets) {
        const rel = path.relative(cwd, s.file);
        console.log('  ' + rel + ':' + s.line + ' -- ' + s.pattern + ' (' + s.preview + ')');
      }
    }

    if (results.envExposure.length > 0) {
      console.log('\n[WARNING] ENV EXPOSURE (' + results.envExposure.length + '):');
      for (const e of results.envExposure) {
        console.log('  ' + e.file + ' -- ' + e.issue);
      }
    }

    if (results.permissions.length > 0) {
      console.log('\n[ADVISORY] PERMISSION ISSUES (' + results.permissions.length + '):');
      for (const p of results.permissions) {
        console.log('  ' + p.rule + ' -- ' + p.issue);
      }
    }

    if (totalFindings === 0) {
      console.log('\nNo security issues found.');
    }

    console.log('');
  }

  process.exit(totalFindings > 0 ? 1 : 0);
}

main();
