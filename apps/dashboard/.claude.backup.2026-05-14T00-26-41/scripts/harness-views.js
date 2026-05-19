'use strict';
/**
 * HARNESS-VIEWS: Pure functions that derive views from the harness event log.
 *
 * Each view is a read-only projection over an array of events (NDJSON lines
 * already parsed). The only function that touches disk is
 * `buildCrossSessionTimeline`, which streams `.harness/sessions/*.jsonl` via
 * `readline` — documented below.
 *
 * All helpers are defensive: invalid events are skipped, never thrown.
 *
 * @version 1.0.0
 */

const fs = require('fs');
const path = require('path');
const readline = require('readline');

const DEFAULT_AGENT_SUMMARY_CHARS = 800;
const DEFAULT_FINDING_CONFIDENCE = 0.7;
const DEFAULT_AGENT_EVENT_LIMIT = 40;
const DEFAULT_CROSS_SESSION_LIMIT = 3;

/**
 * buildAgentVisibility — recent events of a given wave plus high-confidence findings.
 * Used by SubagentStart to inject "what happened so far" into a new agent.
 *
 * @param {object[]} events
 * @param {object}   opts
 *   - wave            (number)  filter target wave; if omitted, uses max wave seen
 *   - maxChars        (number)  truncate agent.stop summaries (default 800)
 *   - minConfidence   (number)  finding confidence floor (default 0.7)
 *   - eventLimit      (number)  cap total events returned (default 40)
 * @returns {object}  { wave, events: [...], findings: [...] }
 */
function buildAgentVisibility(events, opts) {
  const options = opts || {};
  const list = Array.isArray(events) ? events : [];
  const maxChars = Number.isFinite(options.maxChars) ? options.maxChars : DEFAULT_AGENT_SUMMARY_CHARS;
  const minConfidence = Number.isFinite(options.minConfidence) ? options.minConfidence : DEFAULT_FINDING_CONFIDENCE;
  const eventLimit = Number.isFinite(options.eventLimit) ? options.eventLimit : DEFAULT_AGENT_EVENT_LIMIT;

  let wave = options.wave;
  if (!Number.isFinite(wave)) {
    wave = 0;
    for (const ev of list) {
      if (ev && typeof ev.wave === 'number' && ev.wave > wave) wave = ev.wave;
    }
  }

  const waveEvents = [];
  const findings = [];

  for (const ev of list) {
    if (!ev || typeof ev !== 'object') continue;
    const evWave = typeof ev.wave === 'number' ? ev.wave : 0;
    if (evWave === wave) {
      const cloned = truncateSummary(ev, maxChars);
      waveEvents.push(cloned);
    }
    if (ev.event === 'finding') {
      const payload = ev.payload || {};
      const conf = typeof payload.confidence === 'number' ? payload.confidence : 0;
      if (conf >= minConfidence) findings.push(ev);
    }
  }

  // Sort findings: confidence desc, then recency desc (by ts string — ISO sorts lexicographically).
  findings.sort((a, b) => {
    const confA = (a.payload && typeof a.payload.confidence === 'number') ? a.payload.confidence : 0;
    const confB = (b.payload && typeof b.payload.confidence === 'number') ? b.payload.confidence : 0;
    if (confB !== confA) return confB - confA;
    const tsA = a.ts || '';
    const tsB = b.ts || '';
    return tsB > tsA ? 1 : tsB < tsA ? -1 : 0;
  });

  // Dedup findings by a hash derived from the first 60 chars of normalised content.
  // We keep the first occurrence after sorting (highest confidence / most recent).
  const seenHashes = new Set();
  const dedupedFindings = [];
  for (const fev of findings) {
    const content = (fev.payload && typeof fev.payload.content === 'string')
      ? fev.payload.content
      : '';
    const key = content.toLowerCase().replace(/\s+/g, ' ').trim().slice(0, 60);
    if (seenHashes.has(key)) continue;
    seenHashes.add(key);
    dedupedFindings.push(fev);
  }

  // Keep the most recent events within the limit.
  const trimmed = waveEvents.slice(-eventLimit);
  return { wave, events: trimmed, findings: dedupedFindings };
}

/**
 * buildPipelineState — derives the current phase + dispatch failures + metrics for a spec.
 *
 * Metrics are aggregated from tool.use events in the log (Wave 4 — no sidecar).
 *
 * @param {object[]} events
 * @param {object}   opts  { spec }
 * @returns {object}  {
 *   spec, phase, lastEventAt,
 *   dispatchFailures: [...], decisions: [...], lessons: [...],
 *   metrics: { apiCalls, toolBreakdown, retries, agentCount, startedAt }
 * }
 */
function buildPipelineState(events, opts) {
  const options = opts || {};
  const spec = options.spec || null;
  const list = Array.isArray(events) ? events : [];

  let phase = null;
  let lastEventAt = null;
  let startedAt = null;
  const dispatchFailures = [];
  const decisions = [];
  const lessons = [];

  // Aggregated metrics derived from log (replaces .metrics.json sidecar)
  const metrics = {
    apiCalls: 0,
    toolBreakdown: {},
    retries: 0,
    agentCount: 0,
    startedAt: null,
  };

  for (const ev of list) {
    if (!ev || typeof ev !== 'object') continue;
    if (spec && ev.spec !== spec) continue;

    if (ev.ts) {
      if (!startedAt) { startedAt = ev.ts; metrics.startedAt = ev.ts; }
      lastEventAt = ev.ts;
    }

    if (ev.event === 'pipeline.phase') {
      const payload = ev.payload || {};
      if (payload.to) phase = payload.to;
      else if (payload.from) phase = payload.from;
    } else if (ev.event === 'dispatch.failure') {
      dispatchFailures.push(ev);
    } else if (ev.event === 'decision') {
      decisions.push(ev);
    } else if (ev.event === 'lesson') {
      lessons.push(ev);
    } else if (ev.event === 'tool.use') {
      const payload = ev.payload || {};
      const tool = payload.tool || 'unknown';
      if (tool !== 'Read') {
        metrics.apiCalls += 1;
        metrics.toolBreakdown[tool] = (metrics.toolBreakdown[tool] || 0) + 1;
      }
    } else if (ev.event === 'agent.start') {
      metrics.agentCount += 1;
    }
  }

  // Retries are real dispatch failures (is_error=true with API/infra keyword),
  // not heuristic keyword hits on tool_input. Group by phase so worstPhase
  // remains meaningful.
  metrics.retries = dispatchFailures.length;
  metrics.dispatchFailuresByPhase = {};
  for (const ev of dispatchFailures) {
    const ph = (ev.payload && ev.payload.phase) || 'UNKNOWN';
    metrics.dispatchFailuresByPhase[ph] = (metrics.dispatchFailuresByPhase[ph] || 0) + 1;
  }

  return { spec, phase, lastEventAt, dispatchFailures, decisions, lessons, metrics };
}

/**
 * buildSessionSummary — roll-up used by SessionEnd fold into knowledge.json.
 *
 * @param {object[]} events
 * @returns {object}  { sessionId, startedAt, endedAt, agentCount, toolCount, specs, findings, decisions, lessons }
 */
function buildSessionSummary(events) {
  const list = Array.isArray(events) ? events : [];
  const summary = {
    sessionId: null,
    startedAt: null,
    endedAt: null,
    agentCount: 0,
    toolCount: 0,
    specs: [],
    findings: [],
    decisions: [],
    lessons: [],
  };

  const specSet = new Set();

  for (const ev of list) {
    if (!ev || typeof ev !== 'object') continue;
    if (!summary.sessionId && ev.sessionId) summary.sessionId = ev.sessionId;
    if (ev.ts) {
      if (!summary.startedAt) summary.startedAt = ev.ts;
      summary.endedAt = ev.ts;
    }
    if (ev.spec) specSet.add(ev.spec);

    switch (ev.event) {
      case 'agent.start':
        summary.agentCount += 1;
        break;
      case 'tool.use':
        summary.toolCount += 1;
        break;
      case 'finding':
        summary.findings.push(ev);
        break;
      case 'decision':
        summary.decisions.push(ev);
        break;
      case 'lesson':
        summary.lessons.push(ev);
        break;
      default:
        break;
    }
  }

  summary.specs = Array.from(specSet);
  return summary;
}

/**
 * buildCrossSessionTimeline — reads `.harness/sessions/*.jsonl` from disk,
 * most-recent-first by mtime, returning up to `limit` per-session summaries.
 *
 * NOTE: this is the only view that touches disk. Uses `readline` to stream
 * each file line-by-line so memory stays bounded regardless of log size.
 *
 * @param {string} sessionsDir  absolute path to `.harness/sessions`
 * @param {object} opts         { limit = 3 }
 * @returns {Promise<object[]>} array of session summaries (most recent first)
 */
async function buildCrossSessionTimeline(sessionsDir, opts) {
  const options = opts || {};
  const limit = Number.isFinite(options.limit) ? options.limit : DEFAULT_CROSS_SESSION_LIMIT;

  if (!sessionsDir || !fs.existsSync(sessionsDir)) return [];

  let files = [];
  try {
    files = fs.readdirSync(sessionsDir)
      .filter(f => f.endsWith('.jsonl'))
      .map(f => {
        const full = path.join(sessionsDir, f);
        let mtime = 0;
        try { mtime = fs.statSync(full).mtimeMs; } catch (_) {}
        return { file: full, mtime };
      })
      .sort((a, b) => b.mtime - a.mtime)
      .slice(0, limit);
  } catch (_) {
    return [];
  }

  const results = [];
  for (const entry of files) {
    try {
      const events = await streamJsonl(entry.file);
      const summary = buildSessionSummary(events);
      summary.file = entry.file;
      summary.mtime = entry.mtime;

      // Wave 7: enrich with epic metadata per spec
      // Read children_specs from disk to mark epics
      summary.epicInfo = {};
      for (const specName of (summary.specs || [])) {
        try {
          const stateFile = path.join(path.dirname(path.dirname(path.dirname(entry.file))),
            '.pipeline-states', specName + '.json');
          if (fs.existsSync(stateFile)) {
            const st = JSON.parse(fs.readFileSync(stateFile, 'utf8'));
            const children = Array.isArray(st.children_specs) ? st.children_specs : [];
            if (children.length > 0) {
              // Count how many children are CLOSE phase
              let closedCount = 0;
              for (const child of children) {
                try {
                  const cf = path.join(path.dirname(stateFile), child + '.json');
                  if (fs.existsSync(cf)) {
                    const cs = JSON.parse(fs.readFileSync(cf, 'utf8'));
                    const ph = cs.phaseName || cs.phase || '';
                    if (String(ph).toUpperCase() === 'CLOSE') closedCount++;
                  }
                } catch (_) {}
              }
              summary.epicInfo[specName] = { total: children.length, closed: closedCount, children };
            }
          }
        } catch (_) {}
      }

      results.push(summary);
    } catch (_) {
      // skip malformed file
    }
  }
  return results;
}

/**
 * Stream an NDJSON file line-by-line, collecting parsed objects.
 * Invalid lines are skipped silently.
 */
function streamJsonl(filePath) {
  return new Promise((resolve) => {
    const events = [];
    let stream;
    try {
      stream = fs.createReadStream(filePath, { encoding: 'utf8' });
    } catch (_) {
      return resolve(events);
    }
    const rl = readline.createInterface({ input: stream, crlfDelay: Infinity });
    rl.on('line', (line) => {
      const trimmed = line && line.trim();
      if (!trimmed) return;
      try {
        events.push(JSON.parse(trimmed));
      } catch (_) {}
    });
    rl.on('close', () => resolve(events));
    rl.on('error', () => resolve(events));
    stream.on('error', () => resolve(events));
  });
}

/**
 * Parse an NDJSON file synchronously into an array of events.
 * Helper for callers that prefer a blocking API.
 *
 * @param {string} filePath  Absolute path to the .jsonl file.
 * @param {object} [opts]
 *   - skipEvents {string[]}  Event types to exclude during parsing (memory savings).
 *                            e.g. ['tool.use'] skips heartbeat events.
 *                            Filtering is per-line — excluded lines are never added to RAM.
 * @returns {object[]}
 */
function readEventsSync(filePath, opts) {
  const out = [];
  const skipSet = (opts && Array.isArray(opts.skipEvents))
    ? new Set(opts.skipEvents)
    : null;
  try {
    if (!fs.existsSync(filePath)) return out;
    const raw = fs.readFileSync(filePath, 'utf8');
    for (const line of raw.split(/\r?\n/)) {
      const trimmed = line && line.trim();
      if (!trimmed) continue;
      try {
        const ev = JSON.parse(trimmed);
        if (skipSet && skipSet.has(ev.event)) continue;
        out.push(ev);
      } catch (_) {}
    }
  } catch (_) {}
  return out;
}

function truncateSummary(ev, maxChars) {
  if (!ev || ev.event !== 'agent.stop') return ev;
  const payload = ev.payload || {};
  const summary = payload.summary;
  if (typeof summary !== 'string' || summary.length <= maxChars) return ev;
  const next = Object.assign({}, ev, {
    payload: Object.assign({}, payload, { summary: summary.slice(0, maxChars) + '…' }),
  });
  return next;
}

// ── Wave 7: buildSpecTree ─────────────────────────────────────────────────────

const MAX_SPEC_TREE_DEPTH = 3;

/**
 * buildSpecTree — derives the spec parent/child hierarchy.
 *
 * Combines spec.link events from the harness log with on-disk .pipeline-states/*.json
 * to build a recursive tree (max depth 3).
 *
 * @param {object[]} events
 * @param {object}   opts
 *   - rootSpec   {string}  Name of the root spec (epic)
 *   - cwd        {string}  Project root (default: process.cwd())
 * @returns {object}  { spec, phase, parent_spec, children: [...] } or { error: string }
 */
function buildSpecTree(events, opts) {
  const options = opts || {};
  const rootSpec = typeof options.rootSpec === 'string' ? options.rootSpec.trim() : '';
  const cwd = typeof options.cwd === 'string' ? options.cwd : process.cwd();

  if (!rootSpec) return { error: 'rootSpec is required' };

  const statesDir = path.join(cwd, '.claude', '.pipeline-states');

  // Build a map of parent→children from spec.link events (supplements disk state)
  const linkChildren = {}; // parent → Set<child>
  const linkParent = {};   // child → parent
  const list = Array.isArray(events) ? events : [];
  for (const ev of list) {
    if (!ev || ev.event !== 'spec.link') continue;
    const p = ev.payload || {};
    if (typeof p.parent === 'string' && typeof p.child === 'string') {
      if (!linkChildren[p.parent]) linkChildren[p.parent] = new Set();
      linkChildren[p.parent].add(p.child);
      linkParent[p.child] = p.parent;
    }
  }

  // Read a .pipeline-states file safely
  function readState(specName) {
    try {
      const f = path.join(statesDir, specName + '.json');
      if (!fs.existsSync(f)) return null;
      return JSON.parse(fs.readFileSync(f, 'utf8'));
    } catch (_) { return null; }
  }

  // Cycle detection: track ancestry chain
  // Returns { error: 'cycle-detected', ... } which callers must propagate.
  function buildNode(specName, depth, ancestorSet) {
    if (depth > MAX_SPEC_TREE_DEPTH) return { spec: specName, phase: null, truncated: true, children: [] };

    // Cycle: specName is already an ancestor — we're looping back
    if (ancestorSet.has(specName)) {
      return { error: 'cycle-detected', cycle_member: specName };
    }

    const state = readState(specName);
    const phase = (state && (state.phaseName || state.phase)) || null;
    const parentSpec = (state && state.parent_spec) || linkParent[specName] || null;

    // Gather children from disk state + event log
    const childrenSet = new Set();
    if (state && Array.isArray(state.children_specs)) {
      for (const c of state.children_specs) childrenSet.add(c);
    }
    if (linkChildren[specName]) {
      for (const c of linkChildren[specName]) childrenSet.add(c);
    }

    const newAncestors = new Set(ancestorSet);
    newAncestors.add(specName);

    const children = [];
    for (const childName of childrenSet) {
      const childNode = buildNode(childName, depth + 1, newAncestors);
      // Propagate cycle errors immediately up to the root
      if (childNode && childNode.error && childNode.error.includes('cycle')) {
        return { error: 'cycle-detected', parent: specName, child: childName };
      }
      children.push(childNode);
    }

    const node = { spec: specName, phase, children };
    if (parentSpec) node.parent_spec = parentSpec;
    return node;
  }

  // Check root exists (disk or events)
  const rootState = readState(rootSpec);
  const rootInEvents = linkChildren[rootSpec] || linkParent[rootSpec];
  if (!rootState && !rootInEvents) {
    return { error: 'spec not found' };
  }

  return buildNode(rootSpec, 1, new Set());
}

// ── Wave 8: buildEpicSummary ──────────────────────────────────────────────────

/**
 * buildEpicSummary — derive a summary view for an epic and its children.
 *
 * Reads events (in-memory) + pipeline-states from disk to compose:
 *   { epic, children: [{spec, phase}], findings, decisions, lessons,
 *     metrics: { toolCallsTotal, agentsTotal, durationMs, startedAt, endedAt },
 *     folded: boolean }
 *
 * @param {object[]} events  Parsed harness events (from events.jsonl).
 * @param {object}   opts
 *   - epic   {string}  Root spec name.
 *   - cwd    {string}  Project root (default: process.cwd()).
 * @returns {object}
 */
function buildEpicSummary(events, opts) {
  const options = opts || {};
  const epic = typeof options.epic === 'string' ? options.epic.trim() : '';
  const cwd = typeof options.cwd === 'string' ? options.cwd : process.cwd();

  if (!epic) return { error: 'epic is required' };

  const statesDir = path.join(cwd, '.claude', '.pipeline-states');

  // Read root state to discover children
  let rootState = null;
  try {
    const rootFile = path.join(statesDir, epic + '.json');
    if (fs.existsSync(rootFile)) {
      rootState = JSON.parse(fs.readFileSync(rootFile, 'utf8'));
    }
  } catch (_) {}

  const children = (rootState && Array.isArray(rootState.children_specs))
    ? rootState.children_specs
    : [];

  // Build children info from disk
  const childrenInfo = children.map(childName => {
    let phase = null;
    try {
      const cf = path.join(statesDir, childName + '.json');
      if (fs.existsSync(cf)) {
        const cs = JSON.parse(fs.readFileSync(cf, 'utf8'));
        phase = cs.phaseName || cs.phase || null;
      }
    } catch (_) {}
    return { spec: childName, phase };
  });

  // Determine if folded
  const rootPhase = String((rootState && (rootState.phaseName || rootState.phase)) || '').toUpperCase();
  const list = Array.isArray(events) ? events : [];

  // Check epic.fold event
  const folded = list.some(ev =>
    ev.event === 'epic.fold' && ev.payload && ev.payload.epic === epic
  ) || rootPhase === 'CLOSE';

  // Collect events for epic + children
  const specSet = new Set([epic, ...children]);
  const epicEvents = list.filter(ev => ev.spec && specSet.has(ev.spec));

  const findings = [];
  const decisions = [];
  const lessons = [];
  let toolCallsTotal = 0;
  let agentsTotal = 0;
  let minTs = null;
  let maxTs = null;

  for (const ev of epicEvents) {
    if (ev.ts) {
      if (!minTs || ev.ts < minTs) minTs = ev.ts;
      if (!maxTs || ev.ts > maxTs) maxTs = ev.ts;
    }
    switch (ev.event) {
      case 'finding': findings.push(ev); break;
      case 'decision': decisions.push(ev); break;
      case 'lesson': lessons.push(ev); break;
      case 'tool.use': toolCallsTotal++; break;
      case 'agent.start': agentsTotal++; break;
      default: break;
    }
  }

  let durationMs = 0;
  if (minTs && maxTs) {
    try {
      durationMs = new Date(maxTs).getTime() - new Date(minTs).getTime();
      if (!Number.isFinite(durationMs) || durationMs < 0) durationMs = 0;
    } catch (_) { durationMs = 0; }
  }

  return {
    epic,
    children: childrenInfo,
    findings,
    decisions,
    lessons,
    metrics: {
      toolCallsTotal,
      agentsTotal,
      durationMs,
      startedAt: minTs,
      endedAt: maxTs,
    },
    folded,
  };
}

// ── Wave 11: buildSlopeReport ─────────────────────────────────────────────────

/**
 * buildSlopeReport — count anti-slope warns across the last N sessions.
 *
 * Reads events from the provided array (current session) plus archived session
 * files in sessionsDir (up to lookback_sessions - 1 more sessions).
 *
 * Counts events: duplication.warn, convention.warn, regression.warn.
 * Returns { duplication, convention, regression, top_paths, sessions_scanned }.
 *
 * @param {object[]} events            Current-session events already parsed.
 * @param {object}   opts
 *   - lookback_sessions {number}      How many sessions to include (default 5).
 *   - sessionsDir       {string|null} Path to .harness/sessions/ for archived sessions.
 * @returns {object}
 */
function buildSlopeReport(events, opts) {
  const options = opts || {};
  const lookback = Number.isFinite(options.lookback_sessions) ? options.lookback_sessions : 5;
  const sessionsDir = options.sessionsDir || null;

  let allEvents = Array.isArray(events) ? events.slice() : [];

  // Load archived sessions synchronously (already have streamJsonl for async;
  // use the sync readEventsSync helper here to avoid async complexity at call site).
  if (sessionsDir && fs.existsSync(sessionsDir)) {
    try {
      const files = fs.readdirSync(sessionsDir)
        .filter(f => f.endsWith('.jsonl'))
        .map(f => {
          const full = path.join(sessionsDir, f);
          let mtime = 0;
          try { mtime = fs.statSync(full).mtimeMs; } catch (_) {}
          return { file: full, mtime };
        })
        .sort((a, b) => b.mtime - a.mtime)
        .slice(0, lookback - 1); // already have current session above

      for (const entry of files) {
        const sessionEvents = readEventsSync(entry.file);
        allEvents = allEvents.concat(sessionEvents);
      }
    } catch (_) {}
  }

  let duplication = 0;
  let convention = 0;
  let regression = 0;
  const pathCounts = {}; // normalized file path → warn count

  for (const ev of allEvents) {
    if (!ev || typeof ev !== 'object') continue;
    const evName = ev.event || '';
    const payload = ev.payload || {};
    const filePath = (payload.file || '').replace(/\\/g, '/');

    if (evName === 'duplication.warn') {
      duplication += 1;
      if (filePath) pathCounts[filePath] = (pathCounts[filePath] || 0) + 1;
    } else if (evName === 'convention.warn') {
      convention += 1;
      if (filePath) pathCounts[filePath] = (pathCounts[filePath] || 0) + 1;
    } else if (evName === 'regression.warn') {
      regression += 1;
      if (filePath) pathCounts[filePath] = (pathCounts[filePath] || 0) + 1;
    }
  }

  // Top 5 paths by warn count
  const top_paths = Object.entries(pathCounts)
    .sort((a, b) => b[1] - a[1])
    .slice(0, 5)
    .map(([file, count]) => ({ file, count }));

  return {
    duplication,
    convention,
    regression,
    top_paths,
    sessions_scanned: Math.min(lookback, 1 + (sessionsDir ? 1 : 0)), // approx
  };
}

/**
 * buildPRMetrics — DORA-style metrics derived from pr.opened, pr.merged,
 * review.start, and review.complete events.
 *
 * Returns:
 *   {
 *     window: { from, to, days },
 *     totals: { opened, merged, reviewsStarted, reviewsCompleted },
 *     leadTimeMs: { count, p50, p90, max }   // pr.opened → pr.merged per spec/branch
 *     reviewTimeMs: { count, p50, p90, max } // review.start → review.complete per spec
 *     prSize: { count, p50, p90, max }       // lines from payload.linesChanged when present
 *     openByDay: [{ date, count }]
 *     mergedByDay: [{ date, count }]
 *   }
 *
 * Pairing strategy:
 *   - Lead time: match latest pr.opened to first subsequent pr.merged with same
 *     `payload.spec` (preferred) or `payload.branch` (fallback). Unmatched events
 *     are counted in totals only.
 *   - Review time: match review.start to review.complete by `payload.spec` or
 *     `payload.target` within the window.
 */
function buildPRMetrics(events, opts) {
  const o = (opts && typeof opts === 'object') ? opts : {};
  const days = Number.isFinite(o.days) ? o.days : 30;
  const now = o.now ? new Date(o.now) : new Date();
  const from = new Date(now.getTime() - days * 24 * 60 * 60 * 1000);

  function inWindow(ts) {
    if (!ts) return false;
    const t = new Date(ts).getTime();
    return Number.isFinite(t) && t >= from.getTime() && t <= now.getTime();
  }

  const opened = [];
  const merged = [];
  const reviewStart = [];
  const reviewComplete = [];

  for (const e of (Array.isArray(events) ? events : [])) {
    if (!e || typeof e !== 'object') continue;
    if (!inWindow(e.ts)) continue;
    switch (e.event) {
      case 'pr.opened':         opened.push(e);         break;
      case 'pr.merged':         merged.push(e);         break;
      case 'review.start':      reviewStart.push(e);    break;
      case 'review.complete':   reviewComplete.push(e); break;
      default: break;
    }
  }

  function pairKey(e) {
    const p = e.payload || {};
    return p.spec || p.branch || null;
  }

  // Pair opened → merged
  const leadTimes = [];
  const usedMergeIdx = new Set();
  // Sort opened by ts asc to give earliest opener first chance
  opened.sort((a, b) => new Date(a.ts) - new Date(b.ts));
  const sortedMerged = [...merged].sort((a, b) => new Date(a.ts) - new Date(b.ts));
  for (const op of opened) {
    const k = pairKey(op);
    if (!k) continue;
    for (let i = 0; i < sortedMerged.length; i++) {
      if (usedMergeIdx.has(i)) continue;
      const m = sortedMerged[i];
      if (new Date(m.ts) < new Date(op.ts)) continue;
      if (pairKey(m) !== k) continue;
      const dt = new Date(m.ts) - new Date(op.ts);
      if (dt >= 0) leadTimes.push(dt);
      usedMergeIdx.add(i);
      break;
    }
  }

  // Pair review.start → review.complete
  const reviewTimes = [];
  const usedComplete = new Set();
  reviewStart.sort((a, b) => new Date(a.ts) - new Date(b.ts));
  const sortedComplete = [...reviewComplete].sort((a, b) => new Date(a.ts) - new Date(b.ts));
  for (const rs of reviewStart) {
    const k = pairKey(rs);
    if (!k) continue;
    for (let i = 0; i < sortedComplete.length; i++) {
      if (usedComplete.has(i)) continue;
      const rc = sortedComplete[i];
      if (new Date(rc.ts) < new Date(rs.ts)) continue;
      if (pairKey(rc) !== k) continue;
      const dt = new Date(rc.ts) - new Date(rs.ts);
      if (dt >= 0) reviewTimes.push(dt);
      usedComplete.add(i);
      break;
    }
  }

  // PR sizes (linesChanged from pr.opened payload when present)
  const sizes = opened
    .map(e => Number((e.payload || {}).linesChanged))
    .filter(n => Number.isFinite(n) && n > 0);

  function pct(arr, p) {
    if (!arr.length) return null;
    const sorted = [...arr].sort((a, b) => a - b);
    const idx = Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length));
    return sorted[idx];
  }
  function maxOf(arr) { return arr.length ? Math.max.apply(null, arr) : null; }

  function bucketByDay(arr) {
    const map = new Map();
    for (const e of arr) {
      const d = (e.ts || '').slice(0, 10);
      if (!d) continue;
      map.set(d, (map.get(d) || 0) + 1);
    }
    return [...map.entries()].sort().map(([date, count]) => ({ date, count }));
  }

  return {
    window: { from: from.toISOString(), to: now.toISOString(), days },
    totals: {
      opened: opened.length,
      merged: merged.length,
      reviewsStarted: reviewStart.length,
      reviewsCompleted: reviewComplete.length,
    },
    leadTimeMs: {
      count: leadTimes.length,
      p50: pct(leadTimes, 50),
      p90: pct(leadTimes, 90),
      max: maxOf(leadTimes),
    },
    reviewTimeMs: {
      count: reviewTimes.length,
      p50: pct(reviewTimes, 50),
      p90: pct(reviewTimes, 90),
      max: maxOf(reviewTimes),
    },
    prSize: {
      count: sizes.length,
      p50: pct(sizes, 50),
      p90: pct(sizes, 90),
      max: maxOf(sizes),
    },
    openedByDay: bucketByDay(opened),
    mergedByDay: bucketByDay(merged),
  };
}

module.exports = {
  buildAgentVisibility,
  buildPipelineState,
  buildSessionSummary,
  buildCrossSessionTimeline,
  buildSpecTree,
  buildEpicSummary,
  buildSlopeReport,
  buildPRMetrics,
  readEventsSync,
  streamJsonl,
  DEFAULT_AGENT_SUMMARY_CHARS,
  DEFAULT_FINDING_CONFIDENCE,
};

// ── CLI helper (Wave 3 / Wave 6) ─────────────────────────────────────────────
// Usage:
//   node harness-views.js --view agent-visibility [--wave N] [--compact] [--query text]
//   node harness-views.js --view pipeline-state --spec <spec> [--compact] [--query text]
//   node harness-views.js --view session-summary [--compact] [--query text]
//   node harness-views.js --view cross-session-timeline [--limit N] [--compact] [--query text]
//   node harness-views.js --view epic-summary --spec <epic> [--compact]
//   node harness-views.js --view slope-report [--lookback 5]
//
// Reads events.jsonl from .claude/.harness/events.jsonl in cwd (or --cwd).
// Prints JSON to stdout. Exit 0 always (fail-open).
//
// --compact  Returns a lean summary instead of the full projection.
// --query    Case-insensitive text filter applied to content/summary/description fields.
if (require.main === module) {
  (async () => {
    try {
      const args = process.argv.slice(2);
      function getArg(name) {
        const idx = args.indexOf('--' + name);
        return idx >= 0 ? args[idx + 1] : null;
      }
      function hasFlag(name) {
        return args.includes('--' + name);
      }

      const view = getArg('view');
      const cwdArg = getArg('cwd') || process.cwd();
      const compact = hasFlag('compact');
      const queryRaw = getArg('query');
      const query = queryRaw ? queryRaw.toLowerCase() : null;
      const harnessEventsPath = path.join(cwdArg, '.claude', '.harness', 'events.jsonl');
      const sessionsDir = path.join(cwdArg, '.claude', '.harness', 'sessions');

      if (!view) {
        process.stderr.write('Usage: node harness-views.js --view <name> [options]\n');
        process.stderr.write('Views: agent-visibility, pipeline-state, session-summary, cross-session-timeline, spec-tree, epic-summary, slope-report, pr-metrics\n');
        process.stderr.write('Flags: --compact  --query <text>  --cwd <path>  --spec <name>  --days <n>\n');
        process.exit(0);
      }

      let result;

      if (view === 'pipeline-state') {
        const spec = getArg('spec') || null;
        const events = readEventsSync(harnessEventsPath);
        const full = buildPipelineState(events, { spec });
        if (compact) {
          result = compactPipelineState(full, query);
        } else if (query) {
          result = filterPipelineState(full, query);
        } else {
          result = full;
        }

      } else if (view === 'agent-visibility') {
        const waveArg = getArg('wave');
        const wave = waveArg !== null ? parseInt(waveArg, 10) : undefined;
        const events = readEventsSync(harnessEventsPath);
        const full = buildAgentVisibility(events, { wave });
        if (compact) {
          result = compactAgentVisibility(full, query);
        } else if (query) {
          result = filterAgentVisibility(full, query);
        } else {
          result = full;
        }

      } else if (view === 'session-summary') {
        const events = readEventsSync(harnessEventsPath);
        const full = buildSessionSummary(events);
        if (compact) {
          result = compactSessionSummary(full, query);
        } else if (query) {
          result = filterSessionSummary(full, query);
        } else {
          result = full;
        }

      } else if (view === 'cross-session-timeline') {
        const limitArg = getArg('limit');
        const limit = limitArg !== null ? parseInt(limitArg, 10) : 3;
        const full = await buildCrossSessionTimeline(sessionsDir, { limit });
        if (compact) {
          result = compactCrossSessionTimeline(full, query);
        } else {
          result = full;
        }

      } else if (view === 'spec-tree') {
        const spec = getArg('spec') || null;
        if (!spec) {
          result = { error: '--spec is required for spec-tree view' };
        } else {
          const events = readEventsSync(harnessEventsPath);
          const full = buildSpecTree(events, { rootSpec: spec, cwd: cwdArg });
          if (compact) {
            result = compactSpecTree(full);
          } else {
            result = full;
          }
        }

      } else if (view === 'epic-summary') {
        const spec = getArg('spec') || null;
        if (!spec) {
          result = { error: '--spec is required for epic-summary view' };
        } else {
          const events = readEventsSync(harnessEventsPath);
          const full = buildEpicSummary(events, { epic: spec, cwd: cwdArg });
          if (compact) {
            result = compactEpicSummary(full);
          } else {
            result = full;
          }
        }

      } else if (view === 'slope-report') {
        const lookbackArg = getArg('lookback');
        const lookback_sessions = lookbackArg !== null ? parseInt(lookbackArg, 10) : 5;
        const events = readEventsSync(harnessEventsPath);
        result = buildSlopeReport(events, { lookback_sessions, sessionsDir });

      } else if (view === 'pr-metrics') {
        const daysArg = getArg('days');
        const days = daysArg !== null ? parseInt(daysArg, 10) : 30;
        const events = readEventsSync(harnessEventsPath);
        result = buildPRMetrics(events, { days });

      } else {
        result = { error: 'Unknown view: ' + view };
      }

      process.stdout.write(JSON.stringify(result, null, 2) + '\n');
    } catch (err) {
      // Fail-open: print error as JSON so callers can detect it, but exit 0.
      process.stdout.write(JSON.stringify({ error: err.message }) + '\n');
    }
    process.exit(0);
  })();
}

// ── Wave 6: Compact + Query helpers ──────────────────────────────────────────

/**
 * Returns true if any of the given text strings contain the query substring
 * (case-insensitive). query must already be lowercased.
 */
function matchesQuery(query, ...texts) {
  if (!query) return true;
  for (const t of texts) {
    if (typeof t === 'string' && t.toLowerCase().includes(query)) return true;
  }
  return false;
}

/**
 * Extract a short text from an event for query matching.
 * Checks payload.content, payload.summary, payload.description, payload.title.
 */
function eventText(ev) {
  if (!ev || !ev.payload) return '';
  const p = ev.payload;
  return [p.content, p.summary, p.description, p.title]
    .filter(v => typeof v === 'string')
    .join(' ');
}

/**
 * compact agent-visibility: [{type, desc, conf?}]
 */
function compactAgentVisibility(full, query) {
  const items = [];
  for (const ev of (full.events || [])) {
    if (ev.event !== 'agent.start') continue;
    const type = (ev.actor && ev.actor.type) || 'unknown';
    const desc = (ev.payload && ev.payload.description) || '';
    if (query && !matchesQuery(query, type, desc)) continue;
    items.push({ type, desc });
  }
  for (const fev of (full.findings || [])) {
    const type = (fev.actor && fev.actor.type) || 'unknown';
    const content = (fev.payload && fev.payload.content) || '';
    const conf = (fev.payload && typeof fev.payload.confidence === 'number')
      ? fev.payload.confidence
      : undefined;
    if (query && !matchesQuery(query, type, content)) continue;
    const item = { type, desc: content };
    if (conf !== undefined) item.conf = conf;
    items.push(item);
  }
  return items;
}

function filterAgentVisibility(full, query) {
  if (!query) return full;
  return Object.assign({}, full, {
    events: (full.events || []).filter(ev => matchesQuery(query, eventText(ev))),
    findings: (full.findings || []).filter(ev => matchesQuery(query, eventText(ev))),
  });
}

/**
 * compact pipeline-state: {phase, metrics, specs}
 */
function compactPipelineState(full, query) {
  const decisions = (full.decisions || []).filter(ev => !query || matchesQuery(query, eventText(ev)));
  const lessons = (full.lessons || []).filter(ev => !query || matchesQuery(query, eventText(ev)));
  return {
    phase: full.phase,
    metrics: full.metrics,
    specs: full.spec ? [full.spec] : [],
    decisions: decisions.map(ev => ({ text: eventText(ev) })),
    lessons: lessons.map(ev => ({ text: eventText(ev) })),
  };
}

function filterPipelineState(full, query) {
  if (!query) return full;
  return Object.assign({}, full, {
    decisions: (full.decisions || []).filter(ev => matchesQuery(query, eventText(ev))),
    lessons: (full.lessons || []).filter(ev => matchesQuery(query, eventText(ev))),
  });
}

/**
 * compact session-summary: top 10 findings + decisions + lessons
 */
function compactSessionSummary(full, query) {
  const filterItems = (arr) => (arr || [])
    .filter(ev => !query || matchesQuery(query, eventText(ev)))
    .slice(0, 10)
    .map(ev => ({ text: eventText(ev) }));

  return {
    sessionId: full.sessionId,
    startedAt: full.startedAt,
    endedAt: full.endedAt,
    agentCount: full.agentCount,
    specs: full.specs,
    findings: filterItems(full.findings),
    decisions: filterItems(full.decisions),
    lessons: filterItems(full.lessons),
  };
}

function filterSessionSummary(full, query) {
  if (!query) return full;
  return Object.assign({}, full, {
    findings: (full.findings || []).filter(ev => matchesQuery(query, eventText(ev))),
    decisions: (full.decisions || []).filter(ev => matchesQuery(query, eventText(ev))),
    lessons: (full.lessons || []).filter(ev => matchesQuery(query, eventText(ev))),
  });
}

// ── Wave 7: compactSpecTree ───────────────────────────────────────────────────

/**
 * compact spec-tree: only spec names + phases (no metadata).
 * Recursively flattens the tree into compact nodes.
 */
function compactSpecTree(node) {
  if (!node || typeof node !== 'object') return null;
  if (node.error) return node;
  const out = { spec: node.spec, phase: node.phase || null };
  if (node.truncated) out.truncated = true;
  if (Array.isArray(node.children) && node.children.length > 0) {
    out.children = node.children.map(compactSpecTree).filter(Boolean);
  } else {
    out.children = [];
  }
  return out;
}

// ── Wave 8: compactEpicSummary ────────────────────────────────────────────────

/**
 * compact epic-summary: one-liner { epic, status, findings_count, tool_calls }
 */
function compactEpicSummary(full) {
  if (!full || full.error) return full;
  return {
    epic: full.epic,
    status: full.folded ? 'folded' : 'active',
    findings_count: (full.findings || []).length,
    decisions_count: (full.decisions || []).length,
    lessons_count: (full.lessons || []).length,
    tool_calls: (full.metrics && full.metrics.toolCallsTotal) || 0,
    children: (full.children || []).map(c => ({ spec: c.spec, phase: c.phase })),
  };
}

/**
 * compact cross-session-timeline: [{session, date, spec|epic, decisions_count}]
 * Wave 7: if spec has children_specs, renders as "epic=X (N/T children CLOSED)"
 */
function compactCrossSessionTimeline(full, query) {
  const arr = Array.isArray(full) ? full : [];
  return arr
    .filter(s => {
      if (!query) return true;
      const specs = (s.specs || []).join(' ');
      return matchesQuery(query, specs, s.sessionId || '');
    })
    .map(s => {
      const epicInfo = s.epicInfo || {};
      const specsOut = (s.specs || []).map(specName => {
        const ei = epicInfo[specName];
        if (ei) {
          return { epic: specName, children: `${ei.closed}/${ei.total} children CLOSED` };
        }
        return { spec: specName };
      });
      return {
        session: s.sessionId || null,
        date: s.startedAt || null,
        specs: specsOut,
        decisions_count: (s.decisions || []).length,
      };
    });
}
