# L0 Enforcement Hook

> Hook that verifies mandatory delegation.
> **v2.2** - Integrated with JavaScript hooks and memory MCP.

## Related Hooks

| Hook | File | Function |
|------|------|----------|
| L0+L2 | `enforce-pipeline.js` | Asks confirmation for Edit/Write |
| L1 | `enforce-grepai.js` | Blocks Grep/Glob |

## Concept

This hook verifies if Claude is trying to implement code directly instead of delegating to Task tool.

## Trigger

Activates when Claude tries to use:
- `Write` to create code
- `Edit` to modify code
- `Bash` for code operations

## Verification

```
1. Check if inside a Task (subagent)
2. If NO → Block and suggest delegation
3. If YES → Allow
```

## Block Message

```
⛔ L0 ENFORCEMENT

Main Claude should not implement code directly.

Detected action: {Write/Edit/Bash}
File: {path}

Delegate to Task tool with native type:

| Type | subagent_type | model | Prompt |
|------|---------------|-------|--------|
| Bug | general-purpose | opus | bugfix.md |
| Feature | general-purpose | opus | orchestrator.md |
| Backend | general-purpose | opus | backend.md |
| Frontend | general-purpose | opus | frontend.md |
| Database | general-purpose | opus | database.md |
| Explore | Explore | haiku | (native) |

Example:
Task({
  subagent_type: "general-purpose",
  model: "opus",
  description: "Backend implementation",
  prompt: `
# You are the BACKEND SPECIALIST
[content from prompts/backend.md]
# TASK: Implement...
  `
})
```

## Exceptions

Allowed without delegation:
- Configuration files (.json, .yaml)
- Documentation (.md)
- CI/CD scripts
- Files in mustard/ folder

## Implementation

```javascript
// Hook PreToolUse
function preToolUse(tool, params) {
  const isCodeTool = ['Write', 'Edit'].includes(tool) ||
                     (tool === 'Bash' && isCodeOperation(params));

  const isInsideAgent = context.isSubagent;

  const isException = isConfigFile(params.path) ||
                      isDocFile(params.path) ||
                      isMustardFile(params.path);

  if (isCodeTool && !isInsideAgent && !isException) {
    return {
      blocked: true,
      message: L0_ENFORCEMENT_MESSAGE
    };
  }

  return { blocked: false };
}
```

## Configuration

In `settings.json`:

```json
{
  "hooks": {
    "preToolUse": {
      "l0Enforcement": {
        "enabled": true,
        "exceptions": [
          "*.json",
          "*.yaml",
          "*.md",
          "mustard/**"
        ]
      }
    }
  }
}
```

## Allowed Native Types

Claude Code accepts **only 4** `subagent_type` types:

| Type | Usage |
|------|-------|
| `Explore` | Quick codebase exploration |
| `Plan` | Implementation planning |
| `general-purpose` | **MAIN** - Implementation, bug fixes, reviews |
| `Bash` | Terminal commands |

**Never use custom types** like "orchestrator", "backend-specialist", etc.
They **do not work** and cause errors.
