# /approve - Approve Spec

## Trigger

`/approve`

## Description

Approves the current spec and enables the implementation phase.

## Prerequisites

- Active pipeline (created via /feature or /bugfix)
- Spec presented awaiting approval

## Action

1. Marks pipeline as "approved"
2. Enables implementation start
3. Claude proceeds automatically

## Alternative Flow

If the spec is not satisfactory, the user can:
- Give text feedback for adjustments
- Use /complete to cancel

```
User: /approve

Claude: ✅ Spec approved! Starting implementation...
```
