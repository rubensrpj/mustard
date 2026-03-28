<!-- mustard:generated at:2026-03-25T00:00:00.000Z role:general -->
# Settings Wiring Examples

## Example 1: PreToolUse with Multiple Matchers

```json
{
  "PreToolUse": [
    {
      "matcher": "Bash",
      "hooks": [{ "type": "command", "command": "node \"$CLAUDE_PROJECT_DIR\"/.claude/hooks/bash-safety.js", "timeout": 5 }]
    },
    {
      "matcher": "Read|Write|Edit",
      "hooks": [{ "type": "command", "command": "node \"$CLAUDE_PROJECT_DIR\"/.claude/hooks/file-guard.js", "timeout": 5 }]
    },
    {
      "matcher": "Skill",
      "hooks": [{ "type": "command", "command": "node \"$CLAUDE_PROJECT_DIR\"/.claude/hooks/enforce-registry.js", "timeout": 5 }]
    }
  ]
}
```
Ref: `settings.json` (lines 74-114)

## Example 2: PostToolUse with Sequential Hooks

Two hooks run sequentially on the same matcher — format first, then verify:

```json
{
  "PostToolUse": [{
    "matcher": "Write|Edit",
    "hooks": [
      { "type": "command", "command": "node \"$CLAUDE_PROJECT_DIR\"/.claude/hooks/auto-format.js", "timeout": 15 },
      { "type": "command", "command": "node \"$CLAUDE_PROJECT_DIR\"/.claude/hooks/guard-verify.js", "timeout": 5 }
    ]
  }]
}
```
Ref: `settings.json` (lines 117-133)

## Example 3: Permission Configuration

```json
{
  "permissions": {
    "allow": [
      "Read", "Edit", "Write", "Glob", "Grep", "Skill", "Task",
      "Bash(git:*)", "Bash(node:*)", "Bash(npm:*)", "Bash(dotnet:*)"
    ],
    "deny": [
      "Bash(rm -rf:*)", "Bash(git push --force:*)", "Bash(git reset --hard:*)",
      "Bash(shutdown:*)", "Bash(reboot:*)"
    ]
  }
}
```
Ref: `settings.json` (lines 8-70)
