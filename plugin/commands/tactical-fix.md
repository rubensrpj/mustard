---
description: An internal flow — dispatched by the orchestrator router (CLAUDE.md § Intent Routing), not chosen directly by the user. Creates a sub-spec linked to a parent when REVIEW or QA surfaces a small adjacent fix. Preserves SDD purity — parent spec stays frozen after approve. Weak fallback only: use when the router did not engage and a small adjacent fix needs a sub-spec under a parent.
user-invocable: false
source: manual
---
<!-- mustard:generated -->
# /tactical-fix — Sub-Spec for a Tactical Fix

`/mustard:tactical-fix <parent> "<descrição>" [--scope touch|light|full]`

- `<parent>` — slug of the parent spec (`.claude/spec/<parent>/`).
- `<descrição>` — short natural-language description (seeds the slug + body).
- `--scope` — default `light` (≤100 LOC). `touch` ≤30 LOC throwaway; `full` only if it needs a full PRD.

**Qualification** (≤100 LOC, no public-contract change, no pending design decision, no new dependency) → `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Tactical Fix Discovery`. Outside it → regular follow-up or a fresh `/mustard:feature`.

## Action

```bash
mustard-rt run tactical-fix-create --parent <parent> --description "<descrição>" --scope <scope>
```

The binary derives the slug (`YYYY-MM-DD-<kebab>`), creates the directory (aborts if it exists), generates `spec.md` as **pure narrative** (Contexto with a `[[<parent>]]` link, Critérios de Aceitação + Arquivos placeholders), writes the `meta.json` sidecar (`parent` + inherited `lang` + `stage: Analyze` / `outcome: Active`), and emits `spec.link`. The `parent` lives in `meta.json` — never a `### Parent:` header. Then print:

```
Sub-spec created at .claude/spec/<slug>/spec.md
Parent: <parent>
Edit the spec (Contexto, Critérios de Aceitação, Arquivos) and run /mustard:spec, then pick the letter for <slug>, to start the pipeline.
```

## Inviolable

- Fail-open on parent existence — the sub-spec is still created if `<parent>` is missing (only dashboard navigation degrades).
- Never mutate the parent — the link is one-way (child → parent via `meta.json#parent` + `spec.link`).
- One call = one sub-spec. No "light mode" pipeline — the sub-spec passes through the normal gates / QA / CLOSE.
- Do NOT auto-approve — the user reviews the seed and runs `/mustard:spec`.

## Related

`/mustard:review` and `/mustard:qa` § Tactical Fix Discovery — review/QA-time surfacing of candidates.
