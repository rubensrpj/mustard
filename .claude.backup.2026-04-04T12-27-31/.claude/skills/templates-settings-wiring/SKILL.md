---
name: templates-settings-wiring
description: "Pattern for wiring hooks, permissions, and statusline in settings.json.
  Use when adding a new hook to settings, changing permissions, updating lifecycle
  event matchers, configuring statusline, or the user says 'register hook',
  'add permission', 'wire up', 'configure settings'."
---
<!-- mustard:generated at:2026-03-25T00:00:00.000Z role:general -->

# Settings.json Wiring Pattern

The `settings.json` file is the central configuration for Claude Code hook lifecycle, permissions, and statusline.

## Pattern

### Lifecycle Events

| Event | Matcher Examples | Purpose |
|-------|-----------------|---------|
| `PreToolUse` | `Bash`, `Read\|Write\|Edit`, `Skill`, `Task` | Guard before tool execution |
| `PostToolUse` | `Write\|Edit` | Validate/format after tool execution |
| `SessionStart` | `startup` | Clean stale state |
| `SessionEnd` | `prompt_input_exit\|clear\|other` | Prune state files |
| `PreCompact` | `auto\|manual` | Snapshot before compaction |
| `SubagentStart` | (no matcher) | Register agent |
| `SubagentStop` | (no matcher) | Deregister agent |

### Hook Registration Structure

```json
{
  "matcher": "ToolName|OtherTool",
  "hooks": [{
    "type": "command",
    "command": "node \"$CLAUDE_PROJECT_DIR\"/.claude/hooks/{hook}.js",
    "timeout": 5
  }]
}
```

### Key Rules
- `$CLAUDE_PROJECT_DIR` resolves to the project root at runtime
- Always quote the path: `\"$CLAUDE_PROJECT_DIR\"`
- Timeout: 3-5s for simple checks, 10-15s for format/compile
- Multiple hooks under same matcher run sequentially
- Matcher uses `|` for OR: `"Write|Edit"`, `"Read|Write|Edit"`

### Permissions

- `allow`: array of tool patterns (`"Bash(git:*)"`, `"Read"`)
- `deny`: array of blocked patterns (`"Bash(rm -rf:*)"`)
- Deny overrides allow

## Example

```json
{
  "hooks": {
    "PreToolUse": [{
      "matcher": "Bash",
      "hooks": [{
        "type": "command",
        "command": "node \"$CLAUDE_PROJECT_DIR\"/.claude/hooks/my-hook.js",
        "timeout": 5
      }]
    }]
  }
}
```
Ref: `settings.json`

## References

For full code examples with variants:
> Read `references/examples.md`
