---
name: templates-hook-protocol
description: "Pattern for writing Claude Code JavaScript hooks with stdin/stdout JSON protocol,
  fail-open error handling, and correct response formats. Use when creating a new hook,
  adding a guard, writing a PreToolUse or PostToolUse handler, or the user says
  'add hook', 'new guard', 'block command', 'validate file access'."
---
<!-- mustard:generated at:2026-03-25T00:00:00.000Z role:general -->

# Hook Protocol Pattern

All Claude Code hooks are Node.js scripts that read JSON from stdin and write JSON to stdout.

## Pattern

### File Convention
- Location: `hooks/{hook-name}.js`
- Shebang: `#!/usr/bin/env node`
- JSDoc: version, purpose, what it blocks/allows
- Module system: CommonJS (`require`)
- Dependencies: Node.js built-ins only (`fs`, `path`, `child_process`)

### Stdin/Stdout Contract

1. Read full stdin as UTF-8 string
2. Parse JSON — contains `tool_name`, `tool_input`, `hook_event_name`, `cwd`
3. Process: check tool name, extract relevant input, apply rules
4. Respond:
   - **Approve**: `process.exit(0)` silently (no stdout)
   - **PreToolUse block**: `console.log(JSON.stringify({ hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: 'block', permissionDecisionReason: '...' } }))`
   - **PostToolUse block**: `process.stdout.write(JSON.stringify({ decision: 'block', reason: '...' }))`
5. **Always fail-open**: wrap in try/catch, exit 0 on error

### Key Rules
- PreToolUse uses `permissionDecision` (block/deny/allow)
- PostToolUse uses `decision` (approve/block)
- NEVER mix the two formats
- ALWAYS normalize Windows paths before matching (`replace(/\\/g, '/')`)

## Example

```js
#!/usr/bin/env node
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  try {
    const data = JSON.parse(input);
    if (data.tool_name !== 'Bash') { process.exit(0); }
    const cmd = data.tool_input?.command || '';
    if (/dangerous-pattern/.test(cmd)) {
      console.log(JSON.stringify({
        hookSpecificOutput: {
          hookEventName: 'PreToolUse',
          permissionDecision: 'deny',
          permissionDecisionReason: '[hook-name] Blocked: reason'
        }
      }));
    }
    process.exit(0);
  } catch (err) {
    process.stderr.write(`[hook-name] Error: ${err.message}\n`);
    process.exit(0);
  }
});
```
Ref: `hooks/bash-safety.js`

## References

For full code examples with variants:
> Read `references/examples.md`
