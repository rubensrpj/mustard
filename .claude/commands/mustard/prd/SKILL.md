---
name: mustard-prd
description: Use when the user wants to lapidate a free-text intent into a structured PRD JSON for the dashboard's PRD Builder page. Delegates to mustard-rt run prd-build — JSON-only output, no spec file, no code Read.
source: manual
---
<!-- mustard:generated -->
# /mustard:prd — Intent → PRD JSON

## Trigger

`/mustard:prd <intent>` — `<intent>` is free text from `$ARGUMENTS`.

## Action

Delegate entirely to the Rust subcommand:

```bash
mustard-rt run prd-build --intent "$ARGUMENTS"
```

The binary handles: token extraction (PascalCase via regex), entity confront against `grain.model.json` declaration names (via `read_entity_names`, no source-reading), path existence confront (Glob), scope inference heuristic, slug derivation, and JSON shape assembly (matches the dashboard's `PrdForm`).

Print stdout **verbatim** — it is already pure JSON (camelCase, valid for `JSON.parse`, no markdown fence).

## Example

```bash
claude -p "/mustard:prd add refresh token to login" --output-format json
```

## INVIOLABLE RULES

- NEVER `Task(Explore)`.
- NEVER `Read` source files — the binary uses Grep + Glob only.
- NEVER opine on the idea's quality or priority.
- NEVER write to `.claude/spec/` — this command produces JSON only.
- NEVER mix logs/banners/markdown with the JSON output.
