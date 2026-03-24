#!/usr/bin/env node
/**
 * SUBAGENT TRACKER: Tracks active subagents for statusline display
 *
 * Handles 4 events:
 * - PreToolUse(Task): queues description + type before agent starts
 * - SubagentStart:    writes agent state file (consumes from queue)
 * - SubagentStop:     removes agent state file + prunes stale queue
 * - SessionStart:     cleans up stale state from previous sessions
 *
 * State dir: .claude/.agent-state/{agent_id}.json
 * Queue:     .claude/.agent-state/_queue.json
 *
 * @version 2.0.0
 */

const fs = require('fs');
const path = require('path');

const QUEUE_FILE = '_queue.json';
const QUEUE_STALE_MS = 60_000; // 60 seconds

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => (input += chunk));
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    const event = data.hook_event_name;
    const projectDir = data.cwd || process.cwd();
    const stateDir = path.join(projectDir, '.claude', '.agent-state');

    if (event === 'PreToolUse' && data.tool_name === 'Task') {
      handlePreToolUse(data, stateDir);
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
 */
function handlePreToolUse(data, stateDir) {
  const toolInput = data.tool_input || {};
  const description = toolInput.description || '';
  const subagentType = toolInput.subagent_type || 'unknown';

  if (!description && !subagentType) return;

  ensureDir(stateDir);

  const queue = readQueue(stateDir);
  queue.push({
    description,
    type: subagentType,
    queued_at: new Date().toISOString(),
  });
  writeQueue(stateDir, queue);
}

function handleStart(data, stateDir) {
  const agentId = data.agent_id || `unknown-${Date.now()}`;
  const agentType = data.agent_type || 'unknown';

  ensureDir(stateDir);

  // Try to consume a matching entry from the queue
  let description = '';
  const queue = readQueue(stateDir);

  if (queue.length > 0) {
    // Prefer type-match first
    const typeIdx = queue.findIndex((q) => q.type === agentType);
    if (typeIdx >= 0) {
      description = queue[typeIdx].description;
      queue.splice(typeIdx, 1);
    } else {
      // FIFO fallback
      description = queue[0].description;
      queue.shift();
    }
    writeQueue(stateDir, queue);
  }

  fs.writeFileSync(
    path.join(stateDir, `${agentId}.json`),
    JSON.stringify({
      type: agentType,
      description,
      started_at: new Date().toISOString(),
      session_id: data.session_id,
    }),
  );

  // Inject useful context into the subagent
  const response = {
    hookSpecificOutput: {
      hookEventName: 'SubagentStart',
      additionalContext: `[Tracker] Agent "${agentType}" registered. Follow all CLAUDE.md rules.`,
    },
  };
  console.log(JSON.stringify(response));
}

function handleStop(data, stateDir) {
  const agentId = data.agent_id || '';
  const stateFile = path.join(stateDir, `${agentId}.json`);

  try {
    if (fs.existsSync(stateFile)) {
      fs.unlinkSync(stateFile);
    }
  } catch {}

  // Prune stale queue entries (>60s old)
  pruneQueue(stateDir);

  // Clean empty directory
  try {
    if (fs.existsSync(stateDir)) {
      const remaining = fs.readdirSync(stateDir).filter((f) => f.endsWith('.json'));
      if (remaining.length === 0) {
        fs.rmdirSync(stateDir);
      }
    }
  } catch {}
}

function handleSessionStart(data, stateDir) {
  // Clean up stale state files from previous/crashed sessions.
  // Threshold is 10 minutes: agent tasks rarely exceed this, and anything
  // older on a new SessionStart is certainly from a dead session (ghost).
  const STALE_MS = 10 * 60 * 1000; // 10 minutes
  try {
    if (!fs.existsSync(stateDir)) return;
    const files = fs.readdirSync(stateDir).filter(f => f.endsWith('.json') && f !== QUEUE_FILE);
    const now = Date.now();

    for (const f of files) {
      const filePath = path.join(stateDir, f);
      try {
        const content = JSON.parse(fs.readFileSync(filePath, 'utf8'));
        const fileAge = now - new Date(content.started_at || 0).getTime();
        // Remove if: stale (>10min) OR no session_id (legacy) OR different session
        if (fileAge > STALE_MS || !content.session_id || content.session_id !== data.session_id) {
          fs.unlinkSync(filePath);
        }
      } catch {
        // Corrupt file — remove it
        try { fs.unlinkSync(filePath); } catch {}
      }
    }

    // Prune stale queue entries
    pruneQueue(stateDir);

    // Clean empty directory
    try {
      const remaining = fs.readdirSync(stateDir);
      if (remaining.length === 0) fs.rmdirSync(stateDir);
    } catch {}
  } catch {}
}

// ── Queue helpers ──

function readQueue(stateDir) {
  const queueFile = path.join(stateDir, QUEUE_FILE);
  try {
    if (fs.existsSync(queueFile)) {
      return JSON.parse(fs.readFileSync(queueFile, 'utf8'));
    }
  } catch {}
  return [];
}

function writeQueue(stateDir, queue) {
  const queueFile = path.join(stateDir, QUEUE_FILE);
  try {
    if (queue.length === 0) {
      if (fs.existsSync(queueFile)) fs.unlinkSync(queueFile);
    } else {
      fs.writeFileSync(queueFile, JSON.stringify(queue));
    }
  } catch {}
}

function pruneQueue(stateDir) {
  const queue = readQueue(stateDir);
  if (queue.length === 0) return;

  const now = Date.now();
  const fresh = queue.filter((q) => {
    const age = now - new Date(q.queued_at).getTime();
    return age < QUEUE_STALE_MS;
  });

  if (fresh.length !== queue.length) {
    writeQueue(stateDir, fresh);
  }
}

function ensureDir(dir) {
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }
}
