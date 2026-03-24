#!/usr/bin/env node
/**
 * STATUSLINE v4: Concise 2-line status bar for Claude Code
 *
 * Line 1: Module │ Git │ Context bar % tokens │ Duration │ Lines +/- │ Model │ Version
 * Line 2: Pipeline (if active)
 * Line 3: Active agents (if any) — max 3 shown, single line
 *
 * @version 4.0.0
 */

const { execSync } = require('child_process');
const path = require('path');
const fs = require('fs');
const os = require('os');

const GIT_CACHE_FILE = path.join(os.tmpdir(), 'claude-statusline-git.json');
const GIT_CACHE_TTL = 5000;
const PIPELINE_STALE_MS = 4 * 60 * 60 * 1000;
const MAX_AGENTS_SHOWN = 3;

const PHASE_NAMES = { 1: 'ANALYSIS', 2: 'SPEC', 3: 'IMPLEMENT', 3.5: 'VALIDATE', 4: 'REVIEW', 5: 'COMPLETE' };
const TERMINAL_STATUSES = new Set(['implemented', 'completed', 'validated', 'cancelled']);
const ACTIVE_STATUSES = new Set(['specifying', 'approved', 'implementing', 'validating', 'reviewing']);

const AGENT_COLOR_PALETTE = [
  '\x1b[34m', '\x1b[35m', '\x1b[36m', '\x1b[96m', '\x1b[94m', '\x1b[95m',
  '\x1b[93m', '\x1b[33m', '\x1b[31m', '\x1b[91m', '\x1b[92m', '\x1b[32m',
];

function getAgentColor(name) {
  let hash = 5381;
  for (let i = 0; i < name.length; i++) {
    hash = ((hash << 5) + hash + name.charCodeAt(i)) >>> 0;
  }
  return AGENT_COLOR_PALETTE[hash % AGENT_COLOR_PALETTE.length];
}

// ── ANSI helpers ──
const C = {
  reset: '\x1b[0m', bold: '\x1b[1m', dim: '\x1b[2m',
  red: '\x1b[31m', green: '\x1b[32m', yellow: '\x1b[33m', blue: '\x1b[34m',
  magenta: '\x1b[35m', cyan: '\x1b[36m', white: '\x1b[37m', gray: '\x1b[90m',
  brightRed: '\x1b[91m', brightGreen: '\x1b[92m', brightYellow: '\x1b[93m',
  brightCyan: '\x1b[96m', brightMagenta: '\x1b[95m',
};
const sep = ` ${C.dim}\u2502${C.reset} `;

// ── Main ──
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => (input += chunk));
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    const cwd = data.workspace?.current_dir || data.cwd || process.cwd();
    const projectDir = data.workspace?.project_dir || '';
    const stateDir = path.join(projectDir || cwd, '.claude', '.agent-state');
    const agents = getActiveAgents(stateDir);

    // ══════════════════════════════════════════
    // LINE 1
    // ══════════════════════════════════════════
    const line1 = [];

    // Module (with OSC 8 GitHub link)
    line1.push(buildModuleSegment(cwd, projectDir));

    // Git
    const git = getGitCached(cwd);
    if (git?.branch) line1.push(buildGitSegment(git));

    // Context window
    const ctx = buildContextSegment(data);
    if (ctx) line1.push(ctx);

    // Duration
    const dur = buildDurationSegment(data);
    if (dur) line1.push(dur);

    // Lines +/-
    const la = data.cost?.total_lines_added ?? 0;
    const lr = data.cost?.total_lines_removed ?? 0;
    if (la > 0 || lr > 0) {
      const parts = [];
      if (la > 0) parts.push(`${C.green}+${la}${C.reset}`);
      if (lr > 0) parts.push(`${C.red}-${lr}${C.reset}`);
      line1.push(parts.join(''));
    }

    // Model
    const rawModel = data.model?.display_name || data.model?.id || 'Claude';
    const modelShort = rawModel.replace(/^Claude\s*/i, '').replace(/^claude-/i, '');
    line1.push(`${C.blue}${modelShort}${C.reset}`);

    // Version
    if (data.version) line1.push(`${C.dim}v${data.version}${C.reset}`);

    console.log(line1.join(sep));

    // ══════════════════════════════════════════
    // LINE 2: Pipeline (only if active)
    // ══════════════════════════════════════════
    const pipe = buildPipelineSegment(projectDir || cwd, agents.length > 0);
    if (pipe) console.log(pipe);

    // ══════════════════════════════════════════
    // LINE 3: Active agents (only if any)
    // ══════════════════════════════════════════
    if (agents.length > 0) {
      const shown = agents.slice(0, MAX_AGENTS_SHOWN);
      const parts = shown.map(a => {
        const color = getAgentColor(a.type);
        const elapsed = formatElapsed(a.started_at);
        const desc = a.description
          ? `${C.white}${a.description}${C.reset} ${C.dim}(${a.type})${C.reset}`
          : `${color}${a.type}${C.reset}`;
        return `${color}\u25B8${C.reset} ${desc} ${C.gray}${elapsed}${C.reset}`;
      });
      if (agents.length > MAX_AGENTS_SHOWN) {
        parts.push(`${C.dim}+${agents.length - MAX_AGENTS_SHOWN} more${C.reset}`);
      }
      console.log(parts.join(sep));
    }
  } catch {
    console.log('Claude');
  }
});

// ── Segment builders ──

function buildModuleSegment(cwd, projectDir) {
  let moduleName = path.basename(cwd);
  if (projectDir && cwd !== projectDir) {
    const rel = cwd.replace(projectDir, '').replace(/^[/\\]/, '');
    const segs = rel.split(/[/\\]/).filter(Boolean);
    moduleName = segs.slice(0, 2).join('/') || moduleName;
  }

  let repoLink = null;
  try {
    let remote = execSync('git remote get-url origin', {
      cwd, encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'],
    }).trim();
    if (remote) {
      remote = remote.replace(/^git@github\.com:/, 'https://github.com/').replace(/\.git$/, '');
      if (remote.startsWith('https://')) repoLink = remote;
    }
  } catch {}

  if (repoLink) {
    return `\x1b]8;;${repoLink}\x07${C.bold}${C.white}${moduleName}${C.reset}\x1b]8;;\x07`;
  }
  return `${C.bold}${C.white}${moduleName}${C.reset}`;
}

function buildGitSegment(git) {
  const indicators = [];
  if (git.staged > 0) indicators.push(`${C.green}+${git.staged}`);
  if (git.modified > 0) indicators.push(`${C.yellow}~${git.modified}`);
  if (git.untracked > 0) indicators.push(`${C.red}?${git.untracked}`);
  const statusStr = indicators.length > 0
    ? ` ${indicators.join('')}${C.reset}`
    : ` ${C.green}\u2713${C.reset}`;
  return `${C.cyan}\u2387 ${git.branch}${C.reset}${statusStr}`;
}

function buildContextSegment(data) {
  const ctxRem = data.context_window?.remaining_percentage;
  if (ctxRem == null) return null;

  const pct = Math.round(ctxRem);
  const exceeds = data.exceeds_200k_tokens === true;
  const color = exceeds ? C.brightRed
    : pct < 20 ? C.brightRed
    : pct < 40 ? C.red
    : pct < 60 ? C.yellow
    : C.green;

  const barLen = 10;
  const used = Math.round(((100 - pct) / 100) * barLen);
  const bar = `${color}${'\u2588'.repeat(used)}${C.dim}${'\u2591'.repeat(barLen - used)}${C.reset}`;

  const inTok = data.context_window?.total_input_tokens || 0;
  const outTok = data.context_window?.total_output_tokens || 0;
  const totalK = Math.floor((inTok + outTok) / 1000);

  let ctx = `${bar} ${color}${pct}%${C.reset} ${C.gray}${totalK}k${C.reset}`;

  if (exceeds) ctx += ` ${C.brightRed}${C.bold}\u26A0 >200k${C.reset}`;

  const ctxSize = data.context_window?.context_window_size;
  if (ctxSize && ctxSize > 200000) {
    ctx += ` ${C.brightMagenta}[${Math.floor(ctxSize / 1000)}k]${C.reset}`;
  }

  return ctx;
}

function buildDurationSegment(data) {
  const durMs = data.cost?.total_duration_ms ?? 0;
  if (durMs <= 0) return null;

  const m = Math.floor(durMs / 60000);
  const s = Math.floor((durMs % 60000) / 1000);
  const t = m > 0 ? `${m}m${s > 0 ? s + 's' : ''}` : `${s}s`;
  let str = `${C.gray}${t}${C.reset}`;

  const apiMs = data.cost?.total_api_duration_ms ?? 0;
  if (apiMs > 0 && durMs > 0) {
    str += ` ${C.dim}(api ${Math.round((apiMs / durMs) * 100)}%)${C.reset}`;
  }
  return str;
}

function buildPipelineSegment(dir, hasActiveAgents) {
  try {
    const statesDir = path.join(dir, '.claude', '.pipeline-states');
    const legacyFile = path.join(dir, '.claude', '.pipeline-state.json');
    const pipelines = [];

    // Read directory-based states
    if (fs.existsSync(statesDir)) {
      const files = fs.readdirSync(statesDir).filter(f => f.endsWith('.json'));
      for (const f of files) {
        try {
          const raw = JSON.parse(fs.readFileSync(path.join(statesDir, f), 'utf8'));
          if (!TERMINAL_STATUSES.has(raw.status)) pipelines.push(raw);
        } catch {}
      }
    }

    // Backward compat: legacy single file
    if (pipelines.length === 0 && fs.existsSync(legacyFile)) {
      try {
        const raw = JSON.parse(fs.readFileSync(legacyFile, 'utf8'));
        if (!TERMINAL_STATUSES.has(raw.status)) pipelines.push(raw);
      } catch {}
    }

    if (pipelines.length === 0) return null;

    // Filter stale, spec-completed, and all-completed pipelines
    const active = pipelines.filter(raw => {
      // Cross-check with actual spec — if spec says completed/done, pipeline is stale
      if (raw.specName && isSpecCompleted(dir, raw.specName)) return false;
      if (ACTIVE_STATUSES.has(raw.status) && hasActiveAgents) return true;
      if (raw.tasks?.length > 0 && raw.tasks.every(t => t.status === 'completed') && !hasActiveAgents) return false;
      // Without active agents, active statuses are likely orphaned — apply stale TTL
      if (!hasActiveAgents) {
        const ts = raw.updatedAt || raw.implementedAt || raw.approvedAt || raw.startedAt;
        if (ts && (Date.now() - new Date(ts).getTime()) > PIPELINE_STALE_MS) return false;
        // No timestamp at all + no agents = orphaned
        if (!ts) return false;
      }
      return true;
    });

    if (active.length === 0) return null;

    // Sort by updatedAt desc, show most recent
    active.sort((a, b) => {
      const ta = a.updatedAt ? new Date(a.updatedAt).getTime() : 0;
      const tb = b.updatedAt ? new Date(b.updatedAt).getTime() : 0;
      return tb - ta;
    });

    const mostRecent = active[0];
    const spec = mostRecent.spec || mostRecent.feature || '?';
    const phase = mostRecent.phase || '?';
    const phaseName = mostRecent.phaseName || PHASE_NAMES[phase] || '';
    let result = `${C.cyan}${spec}${C.reset} ${C.yellow}P${phase} ${phaseName}${C.reset}`;
    if (active.length > 1) result += ` ${C.dim}+${active.length - 1}${C.reset}`;
    return result;
  } catch { return null; }
}

// ── Spec helpers ──

function isSpecCompleted(dir, specName) {
  // Check if spec exists in completed/
  const completedDir = path.join(dir, '.claude', 'spec', 'completed', specName);
  if (fs.existsSync(completedDir)) return true;
  // Check if active spec has completed/done status in first 5 lines
  const activeSpec = path.join(dir, '.claude', 'spec', 'active', specName, 'spec.md');
  try {
    if (!fs.existsSync(activeSpec)) return false;
    const head = fs.readFileSync(activeSpec, 'utf8').slice(0, 500);
    return /Status:\s*(completed|done)\b/i.test(head);
  } catch { return false; }
}

// ── Data readers ──

function getGitCached(cwd) {
  try {
    if (fs.existsSync(GIT_CACHE_FILE)) {
      const stat = fs.statSync(GIT_CACHE_FILE);
      if (Date.now() - stat.mtimeMs < GIT_CACHE_TTL) {
        const cached = JSON.parse(fs.readFileSync(GIT_CACHE_FILE, 'utf8'));
        if (cached.cwd === cwd) return cached;
      }
    }
  } catch {}

  try {
    const branch = execSync('git rev-parse --abbrev-ref HEAD', {
      cwd, encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'],
    }).trim();
    if (!branch) return null;

    const raw = execSync('git status --porcelain', {
      cwd, encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'],
    });
    const lines = raw ? raw.split('\n').filter(l => l) : [];
    const result = {
      cwd, branch,
      staged: lines.filter(l => /^[MADRC]/.test(l)).length,
      modified: lines.filter(l => /^.[MD]/.test(l)).length,
      untracked: lines.filter(l => l.startsWith('??')).length,
    };

    try { fs.writeFileSync(GIT_CACHE_FILE, JSON.stringify(result)); } catch {}
    return result;
  } catch { return null; }
}

const AGENT_STALE_MS = 15 * 60 * 1000; // 15 minutes — ghost guard

function getActiveAgents(stateDir) {
  try {
    if (!fs.existsSync(stateDir)) return [];
    const files = fs.readdirSync(stateDir).filter(f => f.endsWith('.json') && f !== '_queue.json');
    if (files.length === 0) return [];

    const now = Date.now();
    const agents = files
      .map(f => { try { return JSON.parse(fs.readFileSync(path.join(stateDir, f), 'utf8')); } catch { return null; } })
      .filter(Boolean)
      // Discard ghost entries: agents whose started_at is older than 15 minutes.
      // A real in-progress agent always has a fresh timestamp; stale files are
      // leftovers from crashed/force-killed sessions that SessionEnd never cleaned.
      .filter(a => {
        if (!a.started_at) return false;
        return (now - new Date(a.started_at).getTime()) < AGENT_STALE_MS;
      });

    agents.sort((a, b) => {
      const ta = a.started_at ? new Date(a.started_at).getTime() : 0;
      const tb = b.started_at ? new Date(b.started_at).getTime() : 0;
      return ta - tb;
    });
    return agents;
  } catch { return []; }
}

function formatElapsed(isoTimestamp) {
  if (!isoTimestamp) return '';
  const elapsed = Date.now() - new Date(isoTimestamp).getTime();
  if (elapsed < 0) return '0s';
  const totalSec = Math.floor(elapsed / 1000);
  const m = Math.floor(totalSec / 60);
  const s = totalSec % 60;
  if (m > 0) return `${m}m${s.toString().padStart(2, '0')}s`;
  return `${s}s`;
}
