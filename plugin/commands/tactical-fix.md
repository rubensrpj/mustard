---
description: An internal flow — dispatched by the orchestrator router (CLAUDE.md § Intent Routing), not chosen directly by the user. Creates a sub-spec linked to a parent when REVIEW or QA surfaces a small adjacent fix. Preserves SDD purity — parent spec stays frozen after approve. Weak fallback only: use when the router did not engage and a small adjacent fix needs a sub-spec under a parent.
user-invocable: false
---
<!-- mustard:generated -->
# /tactical-fix — Sub-Spec for a Tactical Fix

`/mustard:tactical-fix <parent> "<descrição>" [--scope touch|light|full]`

- `<parent>` — slug of the parent spec (`.claude/spec/<parent>/`).
- `<descrição>` — short natural-language description (seeds the slug + body).
- `--scope` — default `light` (≤100 LOC). `touch` ≤30 LOC throwaway; `full` only if it needs a full PRD. **It is a free `String`, not a closed vocabulary:** whatever you pass is written RAW into `meta.json#scope`, unvalidated — `--scope ligth` creates a spec whose scope is `ligth`, and nothing downstream that reads it recognises the value. The three names above are a convention this file keeps, enforced nowhere.
- The `scope` in the router's `pipeline.kind` payload is a DIFFERENT field with a different vocabulary: `light` / `full` / `lean`, and a tactical-fix always emits `lean`. It is **ceremony vocabulary** — how much process the unit gets — read only by the dashboard's session telemetry (`apps/dashboard/src/lib/dashboard.ts`, the `scope` of the earliest `pipeline.kind` event). It is not the domain `Scope` enum, which parses `full` / `light` / `touch` and governs the spec's phases and gates. Same word, two ledgers; `lean` is valid in the first and unknown to the second.

**Qualification** (≤100 LOC, no public-contract change, no pending design decision, no new dependency) → `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Tactical Fix Discovery`. Outside it → regular follow-up or a fresh `/mustard:feature`.

## Action

```bash
mustard-rt run tactical-fix-create --parent <parent> --description "<descrição>" --scope <scope>
```

The binary derives the slug (`YYYY-MM-DD-<kebab>`), creates the directory (aborts if it exists), generates `spec.md` as **pure narrative** (Contexto with a `[[<parent>]]` link, Critérios de Aceitação + Arquivos placeholders), writes the `meta.json` sidecar (`parent` + inherited `lang` + `stage: Analyze` / `outcome: Active`), and emits `spec.link`. The `parent` lives in `meta.json` — never a `### Parent:` header.

**The sidecar records `base: null`, and that is deliberate:** a tactical fix has no base of its own because it never cuts a branch of its own — it rides the PARENT's work branch, where the parent's spec, waves and code already live. A sub-spec that recorded a base would be claiming an integration path it does not have.

**What the binary prints is a JSON report**, pretty-printed on stdout: `parent`, `slug`, `spec_dir`, `spec_md`, `meta_json`, `link_emitted`, and `error` (`dir_exists` when the directory was already there — nothing was overwritten). Read it; do not print it raw. The three lines below are what YOU say afterwards, from the fields it returned — they are your report to the user, not the command's output:

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

`${CLAUDE_PLUGIN_ROOT}/commands/pr.md` § 2 and § 3a — review/QA-time surfacing of candidates, inside the PR door that carries both steps.
