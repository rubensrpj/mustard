---
name: mustard-tactical-fix
description: An internal flow — dispatched by the orchestrator router (CLAUDE.md § Intent Routing), not chosen directly by the user. Creates a sub-spec linked to a parent when REVIEW or QA surfaces a small adjacent fix. Preserves SDD purity — parent spec stays frozen after approve. Weak fallback only: use when the router did not engage and a small adjacent fix needs a sub-spec under a parent.
source: manual
---
<!-- mustard:generated -->
# /tactical-fix - Sub-Spec for Tactical Fix

## Trigger

`/mustard:tactical-fix <parent> "<descrição>" [--scope touch|light|full]`

- `<parent>` — slug of parent spec (`.claude/spec/<parent>/`).
- `<descrição>` — short natural-language description (seeds slug + body).
- `--scope` — default `light` (≤100 LOC). `touch` ≤30 LOC throwaway; `full` only if it needed a full PRD.

## When to use

Qualifies when ALL hold: ≤100 LOC, no public-contract change (schema, API, exported types, CLI flags), no pending design decision, no new dependency. Anything outside → regular follow-up OR fresh `/mustard:feature`.

## Action

```bash
mustard-rt run tactical-fix-create --parent <parent> --description "<descrição>" --scope <scope>
```

Binary handles slug derivation (`YYYY-MM-DD-<kebab>`), directory creation (aborts if exists), `spec.md` generation as **pure narrative** (Contexto with `[[<parent>]]` link, Critérios de Aceitação placeholder, Arquivos placeholder), the `meta.json` sidecar carrying `parent` + `lang` (inherited from the parent's `meta.json`, default `en-US`) + `stage: Analyze` / `outcome: Active`, and `spec.link` event emission. The `parent` lives in `meta.json` — never as a `### Parent:` header in the markdown.

Then print:

```
Sub-spec created at .claude/spec/<slug>/spec.md
Parent: <parent>
Edit the spec (Contexto, Critérios de Aceitação, Arquivos) and run /mustard:spec, then pick the letter for <slug>, to start the pipeline.
```

## INVIOLABLE RULES

- Fail-open on parent existence — sub-spec still created if `<parent>` missing; only dashboard navigation degrades.
- Never mutate the parent spec — link is one-way (child → parent via `meta.json#parent` + `spec.link` event; never a `### Parent:` header in the markdown).
- One call = one sub-spec.
- No "light mode" pipeline — sub-spec passes through normal pipeline (same gates, same QA, same CLOSE).
- Do NOT auto-approve — user reviews seed and runs `/mustard:spec`.

## Related

- `pipeline-config.md § Tactical Fix Discovery` — qualification rule.
- `/mustard:review § Tactical Fix Discovery` — review-time surfacing.
- `/mustard:qa § Tactical Fix Discovery após QA Pass` — QA-time surfacing.
