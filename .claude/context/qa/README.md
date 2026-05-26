# QA Agent — Context Extensibility Guide

## What is this directory?

This directory contains the core identity and checklist for the **QA Specialist** agent (Wave 10).

| File | Purpose |
|------|---------|
| `qa.core.md` | Identity, responsibilities, checklist, return format, rules |

## How to add custom QA context

The sync pipeline concatenates all `.md` files in this directory into `qa.context.md` before dispatching the QA agent.

To extend the QA agent's context for your project:

1. Add a file to this directory (e.g., `qa.custom.md`)
2. The file will be automatically included on the next context-compile run
3. Common extensions:
   - Custom AC command patterns for your stack
   - Environment setup steps required before running ACs
   - Stack-specific test runner commands

## What the QA agent does NOT do

- Does NOT modify code
- Does NOT review code style
- Does NOT interpret results — pass/fail is binary by exit code

## Pipeline position

```
ANALYZE → PLAN → /approve → EXECUTE → [QA] → CLOSE
```

The QA phase runs after all EXECUTE agents complete. If QA fails, control returns to EXECUTE for the failing criteria. Maximum 3 QA iterations before escalating to user.
