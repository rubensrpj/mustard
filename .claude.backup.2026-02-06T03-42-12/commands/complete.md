# /complete - Finalize Pipeline

## Trigger

`/complete`

## Description

Finalizes the current pipeline, either completing or canceling.

## Action

1. Moves spec from `spec/active/` to `spec/completed/`
2. Updates entity-registry if needed
3. Clears pipeline state

## When to Use

- After successful implementation and review
- To cancel an ongoing pipeline
- To force close if something went wrong

```
User: /complete

Claude: ✅ Pipeline finalized!
        Spec moved to spec/completed/{name}/
        Entity registry updated.
```
