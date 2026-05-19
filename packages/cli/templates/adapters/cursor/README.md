# Cursor IDE Adapter

> **Status: EXPERIMENTAL** — Cursor's hook system is not yet standardized. This adapter may need updates as Cursor evolves.

## Overview

This adapter allows Cursor IDE to reuse Mustard hooks without duplicating code. It translates between Cursor's hook format and Claude Code's hook protocol.

## How It Works

```
Cursor stdin (JSON) → adapter.js → translates → Mustard hook → translates back → Cursor response
```

## Setup

After running `mustard init`, copy the adapter to your Cursor config:

```bash
mkdir -p .cursor/hooks
cp .claude/adapters/cursor/adapter.js .cursor/hooks/adapter.js
```

Or pass `--cursor` to `mustard init` to do this automatically:

```bash
mustard init --cursor
```

## Usage

In your Cursor hook configuration, reference the adapter with the hook name:

```json
{
  "hooks": {
    "pre_tool": "node .cursor/hooks/adapter.js bash-safety",
    "post_tool": "node .cursor/hooks/adapter.js auto-format"
  }
}
```

## Available Hooks

All Mustard hooks are available through the adapter:

| Hook | Type | Description |
|------|------|-------------|
| bash-safety | PreToolUse | Blocks dangerous shell commands |
| file-guard | PreToolUse | Protects sensitive files |
| review-gate | PreToolUse | Validates git commits |
| enforce-registry | PreToolUse | Entity registry enforcement |
| auto-format | PostToolUse | Auto-formats edited files |
| guard-verify | PostToolUse | Architecture enforcement |

## Event Mapping

| Cursor Event | Claude Code Event |
|-------------|-------------------|
| pre_tool | PreToolUse |
| post_tool | PostToolUse |
| session_start | SessionStart |
| session_end | SessionEnd |

## Environment Variables

- `MUSTARD_HOOK` — Alternative to passing hook name as argument
- `MUSTARD_HOOK_PROFILE` — Controls which hooks run (minimal/standard/strict)
- `MUSTARD_DISABLED_HOOKS` — Comma-separated hooks to skip

## Limitations

- Cursor's hook format is not yet standardized; this adapter may need updates
- Some hooks that rely on Claude Code-specific stdin fields may not work fully
- SessionStart hooks that inject `additionalContext` return it as `context` field
