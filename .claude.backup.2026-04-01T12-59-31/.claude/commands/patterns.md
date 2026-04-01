<!-- mustard:generated at:2026-03-25T00:00:00.000Z role:general -->
# Patterns: Templates

> Recurring code patterns across hooks, scripts, and commands.

## P1. Hook Stdin/Stdout Protocol

All hooks read JSON from stdin, process, and either exit silently (approve) or write JSON to stdout (block/deny/context).

```js
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  const data = JSON.parse(input);
  // ... process
  process.exit(0); // approve (silent)
});
```

Ref: `hooks/bash-safety.js`, `hooks/file-guard.js`, `hooks/enforce-registry.js`

## P2. PreToolUse Block Response

Hooks that block return a specific JSON structure with `permissionDecision`:

```js
console.log(JSON.stringify({
  hookSpecificOutput: {
    hookEventName: 'PreToolUse',
    permissionDecision: 'block', // or 'deny'
    permissionDecisionReason: 'Reason message'
  }
}));
```

Ref: `hooks/enforce-registry.js` (line 100-109), `hooks/bash-safety.js` (line 44-49)

## P3. PostToolUse Approve/Block Response

PostToolUse hooks use `decision` field (not `permissionDecision`):

```js
process.stdout.write(JSON.stringify({ decision: 'approve' }));
// or
process.stdout.write(JSON.stringify({ decision: 'block', reason: '...' }));
```

Ref: `hooks/guard-verify.js` (line 60-95)

## P4. Fail-Open Error Handling

Every hook wraps the main logic in try/catch and exits 0 on error — never blocking due to hook bugs:

```js
} catch (err) {
  process.stderr.write(`[hook-name] Error: ${err.message}\n`);
  process.exit(0); // fail-open
}
```

Ref: `hooks/bash-safety.js` (line 56-59), `hooks/file-guard.js` (line 57-60)

## P5. Regex-Based Dangerous Command Detection

`bash-safety.js` uses an array of `{ re, msg }` objects tested sequentially against the command string:

```js
const DANGEROUS = [
  { re: /\brm\s+(-\w*r\w*f|...)\b/i, msg: 'Recursive force delete blocked' },
  // ...
];
for (const { re, msg } of DANGEROUS) {
  if (re.test(cmd)) { /* deny */ }
}
```

Ref: `hooks/bash-safety.js` (line 14-28)

## P6. File Pattern Blocking

`file-guard.js` blocks access to sensitive files using regex patterns tested against both full path and basename:

```js
const BLOCKED_PATTERNS = [/credentials/i, /\.pem$/i, /\.key$/i, ...];
for (const pattern of BLOCKED_PATTERNS) {
  if (pattern.test(normalized) || pattern.test(basename)) { /* deny */ }
}
```

Ref: `hooks/file-guard.js` (line 16-25)

## P7. Role Detection via Scoring

`sync-detect.js` assigns numeric weights (HIGH=10, MEDIUM=5, LOW=3) to file/dep signals, accumulating scores per role. Highest score wins:

| Weight | Signal Type | Example |
|--------|------------|---------|
| HIGH (10) | Config files | `.csproj` with Sdk.Web, `next.config.*` |
| MEDIUM (5) | Package deps | `react` in package.json, `express` |
| LOW (3) | Directories | `Controllers/`, `app/` + `components/` |

Ref: `scripts/sync-detect.js` (line 210-327)

## P8. SHA-256 Source Hashing for Incremental Scan

`sync-detect.js` computes deterministic hashes by sorting files and updating hash with both path and content:

```js
const hash = crypto.createHash('sha256');
for (const file of files.sort()) {
  hash.update(file);    // path (rename-sensitive)
  hash.update(content); // content
}
return hash.digest('hex');
```

Ref: `scripts/sync-detect.js` (line 643-659)

## P9. FIFO Queue with Type-Match Preference (Subagent Tracker)

Subagent tracker uses a queue to correlate Task tool calls with SubagentStart events. PreToolUse captures description; SubagentStart consumes by type-match first, FIFO fallback:

```js
const typeIdx = queue.findIndex(q => q.type === agentType);
if (typeIdx >= 0) { /* consume type-matched entry */ }
else { /* FIFO: consume first */ }
```

Ref: `hooks/subagent-tracker.js` (line 80-95)

## P10. ANSI Statusline with Git Cache

`statusline.js` caches git status in a temp file with 5s TTL to avoid repeated `git` calls:

```js
const GIT_CACHE_FILE = path.join(os.tmpdir(), 'claude-statusline-git.json');
const GIT_CACHE_TTL = 5000;
```

Ref: `scripts/statusline.js` (line 17-18, 296-326)

## P11. Command SKILL.md Structure

All slash commands follow the same structure: H1 title, trigger section, description, procedure/action, rules, and `ULTRATHINK` footer:

```markdown
# /command-name - Title
## Trigger
`/command-name <args>`
## Description / ## Procedure / ## Action
...
## Rules
...
ULTRATHINK
```

Ref: `commands/mustard/feature/SKILL.md`, `commands/mustard/bugfix/SKILL.md`

## P12. Foundation Skill YAML Frontmatter

Foundation skills use YAML frontmatter with `name`, `description`, and optional `disable-model-invocation`:

```yaml
---
name: skill-name
description: "What it does. When to use it."
disable-model-invocation: true
---
<!-- mustard:generated -->
```

Ref: `skills/commit-workflow/SKILL.md`, `skills/pipeline-execution/SKILL.md`
