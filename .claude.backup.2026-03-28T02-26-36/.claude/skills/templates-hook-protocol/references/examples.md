<!-- mustard:generated at:2026-03-25T00:00:00.000Z role:general -->
# Hook Protocol Examples

## Example 1: PreToolUse — Regex-Based Blocking (bash-safety.js)

Simple pattern: array of `{ re, msg }` tested against command string.

```js
const DANGEROUS = [
  { re: /\brm\s+(-\w*r\w*f|--no-preserve-root)\b/i, msg: 'Recursive delete blocked' },
  { re: /\bgit\s+push\s+(-\w*f|--force)\b/i, msg: 'Force push blocked' },
];
// ... stdin reading ...
for (const { re, msg } of DANGEROUS) {
  if (re.test(cmd)) {
    console.log(JSON.stringify({
      hookSpecificOutput: {
        hookEventName: 'PreToolUse',
        permissionDecision: 'deny',
        permissionDecisionReason: `[bash-safety] ${msg}.`
      }
    }));
    process.exit(0);
  }
}
```
Ref: `hooks/bash-safety.js`

## Example 2: PreToolUse — File Existence Check (enforce-registry.js)

Validates that a required file exists and has valid content before allowing a skill.

```js
const registryPath = path.join(process.cwd(), '.claude', 'entity-registry.json');
if (!fs.existsSync(registryPath)) {
  console.log(JSON.stringify({
    hookSpecificOutput: {
      hookEventName: 'PreToolUse',
      permissionDecision: 'block',
      permissionDecisionReason: 'Entity registry not found. Run /sync-registry first.'
    }
  }));
  return;
}
const registry = JSON.parse(fs.readFileSync(registryPath, 'utf8'));
// validate version, entities, patterns...
```
Ref: `hooks/enforce-registry.js`

## Example 3: PostToolUse — Content Validation (guard-verify.js)

Checks the content being written against architectural rules.

```js
const CRITICAL_RULES = [
  { pattern: /\bDbContext\b/i, scope: /Services?[/\\]/, msg: 'DbContext in Services' },
];
// ...
const violations = [];
for (const rule of CRITICAL_RULES) {
  if (!rule.scope.test(relPath)) continue;
  if (!rule.pattern.test(newContent)) continue;
  violations.push(rule.msg);
}
if (violations.length > 0) {
  process.stdout.write(JSON.stringify({
    decision: 'block',
    reason: violations.map(v => `CRITICAL: ${v}`).join('\n')
  }));
} else {
  process.stdout.write(JSON.stringify({ decision: 'approve' }));
}
```
Ref: `hooks/guard-verify.js`
