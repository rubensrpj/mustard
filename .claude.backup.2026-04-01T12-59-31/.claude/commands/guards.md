<!-- mustard:generated at:2026-03-25T00:00:00.000Z role:general -->
# Guards: Templates

> DO/DON'T rules for the Mustard templates subproject.

## Hook Development

| Rule | Type |
|------|------|
| DO read all stdin before processing (`on('end', ...)`) | DO |
| DO fail-open: `catch → stderr + exit(0)` | DO |
| DO normalize Windows paths (`\` to `/`) before pattern matching | DO |
| DO use `process.exit(0)` for approve (silent exit) | DO |
| DO write JSON to stdout for block/deny/context responses | DO |
| DON'T use any npm dependencies — only Node.js built-ins | DON'T |
| DON'T throw unhandled exceptions — always wrap in try/catch | DON'T |
| DON'T block on parse errors — treat as approve | DON'T |
| DON'T use `console.log` for debugging — use `process.stderr.write` | DON'T |

## Hook Response Protocol

| Rule | Type |
|------|------|
| DO use `permissionDecision: 'block'` or `'deny'` for PreToolUse hooks | DO |
| DO use `decision: 'approve'` or `'block'` for PostToolUse hooks | DO |
| DO include `hookEventName` matching the hook's lifecycle event | DO |
| DON'T mix PreToolUse and PostToolUse response formats | DON'T |

## Settings & Wiring

| Rule | Type |
|------|------|
| DO register every new hook in `settings.json` under the correct lifecycle event | DO |
| DO set a `timeout` for every hook registration (3-15 seconds) | DO |
| DO use `$CLAUDE_PROJECT_DIR` in hook command paths | DO |
| DON'T add hooks without a `matcher` — every hook must declare what it matches | DON'T |

## Commands (SKILL.md)

| Rule | Type |
|------|------|
| DO end every command SKILL.md with `ULTRATHINK` | DO |
| DO include `## Trigger` with exact invocation syntax | DO |
| DO include `## Rules` section with explicit constraints | DO |
| DON'T create commands that implement code directly — delegate via Task | DON'T |

## Scripts

| Rule | Type |
|------|------|
| DO use `execSync` with `stdio: ['pipe','pipe','pipe']` and `windowsHide: true` | DO |
| DO set timeouts on all `execSync` calls | DO |
| DO handle missing files/dirs gracefully (try/catch around fs ops) | DO |
| DON'T import external packages — all scripts must be self-contained | DON'T |

## Skills (SKILL.md)

| Rule | Type |
|------|------|
| DO include YAML frontmatter with `name` and `description` | DO |
| DO write "pushy" descriptions with casual trigger phrases | DO |
| DO add `<!-- mustard:generated -->` after the closing `---` | DO |
| DO keep SKILL.md under 500 lines (ideally under 200) | DO |
| DON'T put `<!-- mustard:generated -->` before the opening `---` | DON'T |
| DON'T use generic descriptions — be specific about what and when | DON'T |

## Generated Files

| Rule | Type |
|------|------|
| DO start every generated file with `<!-- mustard:generated at:{ISO} role:{role} -->` | DO |
| DO include H1 title + blockquote description | DO |
| DO keep under 200 lines per file | DO |
| DO reference real files with `Ref: path/file.ext` | DO |
| DON'T include generic information — only data traced from real code | DON'T |
| DON'T overwrite files without `<!-- mustard:generated` header (manual files) | DON'T |
