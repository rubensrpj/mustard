#!/usr/bin/env bun
'use strict';
/**
 * SUBAGENT TRACKER: Tracks active subagents for statusline display
 *
 * Handles 5 events:
 * - PreToolUse(Task):  emits agent.start to harness log + handles explorer dedup
 * - PostToolUse(Task): detects API overload / dispatch failures and flags pipeline state
 * - SubagentStart:     injects agent-visibility context from harness log
 * - SubagentStop:      emits agent.stop to harness log
 * - SessionStart:      cleans up stale counter files from previous sessions
 *
 * Truth source: .claude/.harness/events.jsonl (Wave 4 — all legacy stores removed)
 *
 * @version 4.0.0
 */

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { shouldRun, isSelfDelegation } = require('./_lib/hook-env.js');
const { emitMetric } = require('./_lib/metrics-emit.js');

// ── Harness event bus (Wave 2 dual emission) ─────────────────────────────────
let harnessEmit = null;
let harnessGetSessionId = null;
let harnessGetWave = null;
try {
  const he = require('./_lib/harness-event.js');
  harnessEmit = he.emit;
  harnessGetSessionId = he.getCurrentSessionId;
  harnessGetWave = he.getCurrentWave;
} catch (_) {} // fail-open: harness optional

function emitEvent(eventName, payload, ctx) {
  try {
    if (harnessEmit) harnessEmit(eventName, payload, ctx);
  } catch (_) {} // fail-open: never break hook on emit error
}

const DEDUP_FILE = 'explorer-dedup.json';
const DEDUP_DENY_MS  = 60_000;  // deny window: same type within 60s → block
const DEDUP_CLEAN_MS = 120_000; // prune entries older than 120s when reading

/**
 * Read newest active pipeline-state once and return {spec, phase, wave}.
 * Fail-open: returns nulls on any error. Caller can pass a freshness window
 * (ms) — if the newest state is older than that, it's treated as stale and
 * spec/phase are null (used by PostToolUse to avoid tagging events with a
 * dead pipeline).
 */
function readPipelineState(projectDir, freshnessMs) {
  const out = { spec: null, phase: null, wave: null };
  try {
    const statesDir = path.join(projectDir, '.claude', '.pipeline-states');
    if (!fs.existsSync(statesDir)) return out;
    const files = fs.readdirSync(statesDir)
      .filter(f => f.endsWith('.json') && !f.endsWith('.metrics.json'));
    if (files.length === 0) return out;

    let newest = null;
    let newestMtime = 0;
    for (const f of files) {
      try {
        const fp = path.join(statesDir, f);
        const stat = fs.statSync(fp);
        if (stat.mtimeMs > newestMtime) {
          newestMtime = stat.mtimeMs;
          newest = fp;
        }
      } catch {}
    }
    if (!newest) return out;
    if (typeof freshnessMs === 'number' && (Date.now() - newestMtime) >= freshnessMs) {
      return out;
    }
    const st = JSON.parse(fs.readFileSync(newest, 'utf8'));
    out.spec = st.specName || st.spec || st.name || null;
    out.phase = st.phaseName || st.phase || null;
    // Wave-plan pipelines store the active wave in `currentWave`; single specs
    // use `wave`. Read both so the subtraction emitter works for either layout.
    if (typeof st.wave === 'number') out.wave = st.wave;
    else if (typeof st.currentWave === 'number') out.wave = st.currentWave;
    return out;
  } catch {
    return out;
  }
}

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => (input += chunk));
process.stdin.on('end', () => {
  try {
    if (!shouldRun('subagent-tracker')) { process.exit(0); }
    const data = JSON.parse(input);
    const event = data.hook_event_name;
    const projectDir = data.cwd || process.cwd();
    const stateDir = path.join(projectDir, '.claude', '.agent-state');

    const isDispatchTool = data.tool_name === 'Task' || data.tool_name === 'Agent';
    if (event === 'PreToolUse' && isDispatchTool) {
      handlePreToolUse(data, stateDir);
    } else if (event === 'PostToolUse' && isDispatchTool) {
      handlePostToolUse(data, stateDir);
    } else if (event === 'SubagentStart') {
      handleStart(data, stateDir);
    } else if (event === 'SubagentStop') {
      handleStop(data, stateDir);
    } else if (event === 'SessionStart') {
      handleSessionStart(data, stateDir);
    }

    process.exit(0);
  } catch (err) {
    process.stderr.write(`[subagent-tracker] Error: ${err.message}\n`);
    process.exit(0);
  }
});

/**
 * PreToolUse(Task): Queue description + type before agent spawns.
 * The SubagentStart event doesn't carry description, so we capture it here
 * and match it later via FIFO queue with type-matching preference.
 *
 * Also parses recommended_skills from the Task prompt and increments
 * skillHits.loaded in the active pipeline state. Mustard 2.0 Phase 1:
 * `.subagent-registry.json` writes were removed — agent.start events in the
 * EventStore (or events.jsonl replay log) are the truth source.
 */
function handlePreToolUse(data, stateDir) {
  if (isSelfDelegation(data)) { return; }
  const toolInput = data.tool_input || {};
  const description = toolInput.description || '';
  const subagentType = toolInput.subagent_type || 'unknown';

  if (!description && !subagentType) return;

  // ── Explorer dedup: deny if same subagent_type was dispatched within 60s ──
  if (isExplorerAgent(subagentType)) {
    try {
      const { cache, changed } = readDedupCache(stateDir);
      const lastTs = cache[subagentType];
      const now = Date.now();

      if (lastTs !== undefined && (now - lastTs) < DEDUP_DENY_MS) {
        const secondsAgo = Math.round((now - lastTs) / 1000);
        // Flush stale entries if any were pruned (best-effort, not required for deny path)
        if (changed) writeDedupCache(stateDir, cache);
        process.stdout.write(JSON.stringify({
          permissionDecision: 'deny',
          permissionDecisionReason:
            `[Dedup] ${subagentType} already dispatched ${secondsAgo}s ago. ` +
            `Wait or use a different explorer.`,
        }) + '\n');
        process.exit(0);
      }

      // Record this dispatch
      cache[subagentType] = now;
      writeDedupCache(stateDir, cache);
    } catch {} // fail-open: dedup is advisory — allow on any error
  }

  // ── Emit agent.start event to harness log ───────────────────────────────
  try {
    const projectDir = path.resolve(stateDir, '..', '..');
    const sessionId = harnessGetSessionId ? harnessGetSessionId(data) : null;
    const wave = harnessGetWave ? harnessGetWave(data) : 0;

    // Single read of newest pipeline-state for spec + phase (formerly read twice).
    const ps = readPipelineState(projectDir);
    const currentSpec = ps.spec;
    const currentPhase = ps.phase;

    // Extract model from tool input prompt (best-effort — may be absent)
    const model = (toolInput.model || null);

    // Prefix metrics: hash + bytes of the dispatched prompt. Used by
    // prompt-prefix-stats to group cache-eligible delegations and quantify
    // prompt caching savings. SHA-256 truncated to 16 hex chars (~64 bits) is
    // ample for cache grouping cardinality.
    const promptStr = toolInput.prompt || '';
    const prefix_bytes = Buffer.byteLength(promptStr, 'utf8');
    const prefix_hash = crypto.createHash('sha256').update(promptStr).digest('hex').slice(0, 16);

    emitEvent('agent.start', {
      description,
      model,
      parentAgentId: data.parentAgentId ?? null,
      prefix_hash,
      prefix_bytes,
    }, {
      cwd: projectDir,
      sessionId,
      wave,
      spec: currentSpec,
      actor: { kind: 'agent', id: subagentType, type: subagentType },
    });

    // Descriptive metric: bytes of work isolated into a sub-context via Task.
    // This is NOT savings — it reports how much prompt was delegated rather
    // than running in the parent context. Aggregated as "isolation" so the
    // dashboard can show throughput without inflating the token-saved total.
    try {
      if (prefix_bytes > 0) {
        emitMetric('delegation', {
          tokensAffected: Math.round(prefix_bytes / 4),
          tokensSaved: 0,
          note: 'task-dispatched',
          extras: {
            subagent_type: subagentType,
            model: model || 'inherited',
            category: 'isolation',
            prefix_hash,
            prefix_bytes,
          },
          cwd: projectDir,
        });
      }
    } catch (_) { /* fail-silent */ }

    // ── mustard.subtraction.applied (wave-slice) ───────────────────────────
    // Every EXECUTE dispatch receives only its wave's spec slice, never the
    // full spec. That omission is a real, per-Task economy — but it is only
    // observable HERE, in the hook that sees the dispatch. The orchestrator
    // (an LLM) used to emit it by hand and almost never remembered, so the
    // metric stayed zero. One event per Task: N agents in a wave = N real
    // omissions, each a separate API context.
    try {
      const isExecute = ps.phase === 'EXECUTE' || ps.phase === 3;
      if (isExecute && typeof ps.wave === 'number' && ps.wave >= 1 && ps.spec) {
        let specPath = path.join(projectDir, '.claude', 'spec', 'active', ps.spec, 'spec.md');
        if (!fs.existsSync(specPath)) {
          // wave-plan layout: the spec dir holds per-wave sub-folders.
          const specDir = path.join(projectDir, '.claude', 'spec', 'active', ps.spec);
          const waveRe = new RegExp(`^wave-${ps.wave}-`);
          const sub = fs.readdirSync(specDir).find(d => waveRe.test(d));
          specPath = sub ? path.join(specDir, sub, 'spec.md') : null;
        }
        if (specPath && fs.existsSync(specPath)) {
          const { measure } = require('../scripts/spec-extract.js');
          const m = measure(specPath, ps.wave);
          // slice_bytes === 0 means the wave section was not found — we cannot
          // measure the omission honestly, so we emit nothing.
          if (m && m.slice_bytes !== 0) {
            emitEvent('mustard.subtraction.applied', {
              type: 'wave-slice',
              bytes_omitted: m.omitted_bytes,
              full_bytes: m.full_bytes,
              slice_bytes: m.slice_bytes,
              prompt_bytes: prefix_bytes,
              wave: ps.wave,
              measured: true,
            }, {
              cwd: projectDir,
              sessionId,
              wave,
              spec: ps.spec,
              actor: { kind: 'hook', id: 'subagent-tracker' },
            });
          }
        }
      }
    } catch (_) {} // fail-open: subtraction telemetry is advisory
  } catch (_) {} // fail-open

  // ── skill_hit_rate: parse recommended_skills from Task prompt ─────────────
  // We look for a "Recommended Skills" section header followed by list items,
  // or a `recommended_skills:` YAML-style block.  Conservative regex — false
  // negatives are acceptable; false positives would corrupt the metric.
  try {
    const prompt = toolInput.prompt || '';
    const recommendedSkills = parseRecommendedSkills(prompt);
    if (recommendedSkills.length === 0) return;

    const projectDir = path.resolve(stateDir, '..', '..');

    // Mustard 2.0 Phase 1: `.subagent-registry.json` write removed. The
    // (agentType, recommendedSkills, startedAt) tuple is already carried by the
    // `agent.start` event emitted above in handlePreToolUse — consumers read
    // it via EventStore.query({event:'agent.start'}) or events.jsonl replay.

    // Increment skillHits.loaded in the active pipeline state
    const statesDir = path.join(projectDir, '.claude', '.pipeline-states');
    if (!fs.existsSync(statesDir)) return;
    const stateFiles = fs.readdirSync(statesDir).filter(f => f.endsWith('.json'));
    if (stateFiles.length === 0) return;

    let newestState = null;
    let newestMtime = 0;
    for (const f of stateFiles) {
      try {
        const fp = path.join(statesDir, f);
        const stat = fs.statSync(fp);
        if (stat.mtimeMs > newestMtime) {
          newestMtime = stat.mtimeMs;
          newestState = fp;
        }
      } catch {}
    }
    if (!newestState) return;

    const state = JSON.parse(fs.readFileSync(newestState, 'utf8'));
    if (!state.metrics) state.metrics = { apiCalls: 0, toolBreakdown: {}, retries: 0 };
    if (!state.metrics.skillHits) state.metrics.skillHits = {};
    if (!state.metrics.skillHits[subagentType]) {
      state.metrics.skillHits[subagentType] = { loaded: 0, read: 0 };
    }
    state.metrics.skillHits[subagentType].loaded += recommendedSkills.length;
    fs.writeFileSync(newestState, JSON.stringify(state, null, 2), 'utf8');
  } catch {} // fail-open: skill tracking is advisory
}

/**
 * Parse recommended skills from a Task prompt string.
 * Matches list items under a "Recommended Skills" or "recommended_skills:" header.
 * Returns an array of skill name strings (e.g. ["templates-hook-protocol"]).
 */
function parseRecommendedSkills(prompt) {
  const skills = [];
  // Match a section header then collect "- skill-name" lines until blank or next header
  const sectionMatch = prompt.match(
    /(?:recommended.skills|recommended_skills)\s*[:\-]?\s*\n((?:\s*-\s*[\w\-]+.*\n?)+)/i
  );
  if (sectionMatch) {
    const lines = sectionMatch[1].split('\n');
    for (const line of lines) {
      const m = line.match(/^\s*-\s*([\w][\w\-]*[\w])/);
      if (m) skills.push(m[1]);
    }
  }
  return skills;
}

/**
 * PostToolUse(Task): This is where the Task tool actually returns. We do three things:
 *   1. Emit `agent.stop` with the real tool_response (SubagentStop carries no body)
 *   2. Detect dispatch failures (API overload, HTTP 5xx) and emit `dispatch.failure`
 *      so retries are measured from real signals instead of keyword guesses
 *   3. Flag pipeline-state with `lastDispatchFailure` so /resume can auto-recover
 */
function handlePostToolUse(data, stateDir) {
  try {
    if (isSelfDelegation(data)) { return; }

    const toolInput = data.tool_input || {};
    const toolResponse = data.tool_response || {};
    const subagentType = toolInput.subagent_type || 'unknown';
    const projectDir = path.resolve(stateDir, '..', '..');

    // Resolve spec/phase from newest pipeline-state for event tagging (single read,
    // 10-min freshness window — stale states are ignored to avoid mis-tagging).
    const ps = readPipelineState(projectDir, 10 * 60 * 1000);
    const currentSpec = ps.spec;
    const currentPhase = ps.phase;

    // (1) Emit agent.stop with real summary. tool_response shape varies — most
    // commonly an array of content blocks; serialize defensively and cap size.
    try {
      const responseStr = typeof toolResponse === 'string'
        ? toolResponse
        : JSON.stringify(toolResponse);
      const summary = (responseStr || '').slice(0, 800);
      const sessionId = harnessGetSessionId ? harnessGetSessionId(data) : null;
      const wave = harnessGetWave ? harnessGetWave(data) : 0;
      emitEvent('agent.stop', {
        summary,
        confidence: null,
        durationMs: null,
        toolCount: null,
        isError: toolResponse.is_error === true || undefined,
      }, {
        cwd: projectDir,
        sessionId,
        wave,
        spec: currentSpec,
        actor: { kind: 'agent', id: subagentType, type: subagentType },
      });
    } catch (_) {}

    // (2)(3) Dispatch failure detection — require is_error=true AND a failure
    // keyword so we don't false-positive on agents merely documenting errors.
    const responseTextLower = (typeof toolResponse === 'string'
      ? toolResponse
      : JSON.stringify(toolResponse)).toLowerCase();
    const isDispatchFailure =
      toolResponse.is_error === true &&
      /overload|rate.?limit|\b429\b|\b529\b|throttl|too many requests|tool result missing|\b50[0-4]\b|service unavailable/.test(responseTextLower);

    if (!isDispatchFailure) return;

    // Emit dispatch.failure event — this is the real retry signal that replaces
    // the old keyword-based `retry:true` flag on tool.use events.
    try {
      const sessionId = harnessGetSessionId ? harnessGetSessionId(data) : null;
      const wave = harnessGetWave ? harnessGetWave(data) : 0;
      emitEvent('dispatch.failure', {
        agentType: subagentType,
        description: (toolInput.description || '').slice(0, 200),
        phase: currentPhase,
      }, {
        cwd: projectDir,
        sessionId,
        wave,
        spec: currentSpec,
        actor: { kind: 'hook', id: 'subagent-tracker' },
      });
    } catch (_) {}

    // Flag pipeline-state for /resume auto-recovery
    const statesDir = path.join(projectDir, '.claude', '.pipeline-states');
    if (!fs.existsSync(statesDir)) return;
    const files = fs.readdirSync(statesDir).filter(f => f.endsWith('.json') && !f.endsWith('.metrics.json'));
    if (files.length === 0) return;
    let newest = null, newestMtime = 0;
    for (const f of files) {
      try {
        const fp = path.join(statesDir, f);
        const stat = fs.statSync(fp);
        if (stat.mtimeMs > newestMtime) { newestMtime = stat.mtimeMs; newest = fp; }
      } catch {}
    }
    if (!newest) return;

    const state = JSON.parse(fs.readFileSync(newest, 'utf8'));
    state.lastDispatchFailure = {
      at: new Date().toISOString(),
      reason: 'dispatch_failure',
      agentType: subagentType,
      description: toolInput.description || '',
      prompt: (toolInput.prompt || '').slice(0, 2000),
    };
    fs.writeFileSync(newest, JSON.stringify(state, null, 2), 'utf8');
  } catch {} // fail-open: failure detection is advisory
}

// ── Harness views (Wave 3 — reads derive from event log) ─────────────────────
let harnessViews = null;
try {
  harnessViews = require('../scripts/event-projections.js');
} catch (_) {} // fail-open: views optional

// Wave 5: adaptive context budget per agent type (Melhoria 1)
const AGENT_CTX_BUDGET = {
  Explore: 400,
  Plan: 600,
  'general-purpose': 800,
  // default: 600
};

function handleStart(data, stateDir) {
  const agentType = data.agent_type || 'unknown';
  const budget = AGENT_CTX_BUDGET[agentType] ?? 600;

  // Build additionalContext from harness event log (Wave 4 — log is sole source)
  const projectDir = path.resolve(stateDir, '..', '..');
  let context = `[Tracker] Agent "${agentType}" registered. Follow all CLAUDE.md rules.`;

  try {
    if (harnessViews) {
      const harnessEventsPath = path.join(projectDir, '.claude', '.harness', 'events.jsonl');
      // Wave 5 (Melhoria 3): skip tool.use events — they are heartbeats not relevant to agent view
      const events = harnessViews.readEventsSync(harnessEventsPath, { skipEvents: ['tool.use'] });
      if (events.length > 0) {
        const rawWave = harnessGetWave ? harnessGetWave(data) : 0;
        const waveOpts = rawWave > 0 ? { wave: rawWave } : {};
        const visibility = harnessViews.buildAgentVisibility(events, {
          ...waveOpts,
          maxChars: budget,
        });

        const parts = [];

        // Show active agents in this wave (agent.start without matching agent.stop)
        const stoppedIds = new Set(
          events
            .filter(e => e.event === 'agent.stop')
            .map(e => e.actor && e.actor.id)
            .filter(Boolean)
        );
        const activeStarts = visibility.events.filter(e => {
          if (e.event !== 'agent.start') return false;
          const id = e.actor && e.actor.id;
          return id && !stoppedIds.has(id);
        });

        if (activeStarts.length > 0) {
          parts.push('[Parallel Agents in Wave ' + visibility.wave + ']');
          for (const ev of activeStarts) {
            const aType = (ev.actor && ev.actor.type) || 'unknown';
            const desc = (ev.payload && ev.payload.description) || '';
            parts.push(`- ${aType}: ${desc.slice(0, 120)}`);
          }
        }

        // High-confidence findings from any wave (already deduped + sorted by buildAgentVisibility)
        if (visibility.findings.length > 0) {
          parts.push('[Prior Findings]');
          for (const fev of visibility.findings.slice(0, 5)) {
            const content = (fev.payload && fev.payload.content) || '';
            const conf = (fev.payload && fev.payload.confidence) || 0;
            parts.push(`- [conf=${conf.toFixed(2)}] ${content.slice(0, 200)}`);
          }
        }

        if (parts.length > 0) {
          let visText = parts.join('\n');
          if (visText.length > budget) visText = visText.slice(0, budget - 3) + '...';

          // Wave 6: append escape-hatch hint only when budget allows it
          const hintLine = '\n[Memory] Query more: bun .claude/scripts/event-projections.js --view <name> [--query text]';
          if (visText.length + hintLine.length <= budget) {
            visText += hintLine;
          }

          context += '\n\n[Agent Memory] Findings from prior agents:\n' + visText;
        }
      }
    }
  } catch (_) {} // fail-open: harness view is advisory

  const response = {
    hookSpecificOutput: {
      hookEventName: 'SubagentStart',
      additionalContext: context,
    },
  };
  console.log(JSON.stringify(response));
}

// SubagentStop carries no tool_response body — `agent.stop` is now emitted from
// `handlePostToolUse` (PostToolUse Task) where the real response lives. This
// handler is kept as a no-op to preserve the hook wiring; future enhancements
// (e.g. session-level cleanup) can hook in here.
function handleStop(_data, _stateDir) {}

function handleSessionStart(data, stateDir) {
  // Clean up stale counter files left by tool-use-counter.js from previous sessions.
  // These live in .agent-state/ and use a different naming convention (*.counter.json).
  // Agent state files ({id}.json) and _queue.json are no longer written (Wave 4).
  const STALE_MS = 10 * 60 * 1000; // 10 minutes
  try {
    if (!fs.existsSync(stateDir)) return;
    const files = fs.readdirSync(stateDir).filter(f => f.endsWith('.json'));
    const now = Date.now();

    for (const f of files) {
      const filePath = path.join(stateDir, f);
      try {
        const stat = fs.statSync(filePath);
        // Remove files older than 10 minutes (stale from crashed sessions)
        if (now - stat.mtimeMs > STALE_MS) {
          fs.unlinkSync(filePath);
        }
      } catch {
        try { fs.unlinkSync(filePath); } catch {}
      }
    }

    // Clean empty directory
    try {
      const remaining = fs.readdirSync(stateDir);
      if (remaining.length === 0) fs.rmdirSync(stateDir);
    } catch {}
  } catch {}

  // ── Phase 2: sweep orphan span sidecars (> 10 min mtime). Sidecars live at
  // .claude/.harness/.active-spans/{toolUseId}.json and are normally deleted
  // by TokenTracker.endSpan. Orphans arise from killed Bun processes, crashes,
  // or PostToolUse hooks that never fired.
  try {
    const projectDir = path.resolve(stateDir, '..', '..');
    const sidecarDir = path.join(projectDir, '.claude', '.harness', '.active-spans');
    if (fs.existsSync(sidecarDir)) {
      const now = Date.now();
      for (const f of fs.readdirSync(sidecarDir)) {
        const fp = path.join(sidecarDir, f);
        try {
          const stat = fs.statSync(fp);
          if (now - stat.mtimeMs > STALE_MS) fs.unlinkSync(fp);
        } catch {} // fail-open
      }
    }
  } catch {} // fail-open
}

// ── Explorer dedup helpers ──

/**
 * Returns true when the subagent_type represents an explorer agent.
 * Matches "Explore" (native Claude Code type) and any custom type containing
 * "explorer" (case-insensitive, e.g. "Sialia.Backend-explorer").
 */
function isExplorerAgent(subagentType) {
  if (!subagentType) return false;
  return subagentType === 'Explore' || /explorer/i.test(subagentType);
}

/**
 * Read the dedup cache, pruning entries older than DEDUP_CLEAN_MS.
 * Returns { cache, changed } where changed=true if stale entries were removed.
 * Fail-open: returns empty cache on any I/O error.
 */
function readDedupCache(stateDir) {
  const filePath = path.join(stateDir, DEDUP_FILE);
  try {
    let raw = {};
    if (fs.existsSync(filePath)) {
      raw = JSON.parse(fs.readFileSync(filePath, 'utf8'));
    }
    const now = Date.now();
    let changed = false;
    for (const [key, ts] of Object.entries(raw)) {
      if (now - ts > DEDUP_CLEAN_MS) {
        delete raw[key];
        changed = true;
      }
    }
    return { cache: raw, changed };
  } catch {
    return { cache: {}, changed: false };
  }
}

/**
 * Persist the dedup cache to disk. Fail-open: silently ignores write errors.
 */
function writeDedupCache(stateDir, cache) {
  try {
    fs.writeFileSync(path.join(stateDir, DEDUP_FILE), JSON.stringify(cache), 'utf8');
  } catch {}
}
