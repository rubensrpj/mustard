# Spec Language Reference

> Detail for `/feature` + `/bugfix` — language + tone resolution, spec narrative rules, EN-only code policy.

## Mustard `mustard.json` Policy — 3 Dimensions

Mustard splits language/tone decisions into three independent dimensions. Each one has a single source of truth, and they never bleed into each other.

### (1) Spec language — `mustard.json#specLang`

The narrative locale of a spec (`spec.md`, wave plans, memory notes) is **BCP-47** only — `pt-BR`, `en-US`, `fr-FR`, `de-DE`, etc. Short codes (`pt`, `en`) are legacy and rejected.

- Headings, prose, bullet points, examples — everything inside a spec uses the configured locale.
- Do **not** mix languages inside a single spec.
- Catalogue locales (those Mustard has translated banners for) are `pt-BR` and `en-US`; other BCP-47 codes are accepted for the spec body, banners fall back to the default catalogue.

### (2) Tone — `mustard.json#tone`

The drafter / banner tone is independent of language and lives in `mustard.json#tone`:

- `didactic` (default) — expands abbreviations on first use, prefers plain words, explains why. Best for user-facing chat.
- `technical` — direct, jargon and abbreviations welcome, no parenthetical glossing.
- `concise` — minimal prose, drops fillers, collapses whitespace.

`mustard-rt run spec-draft` wires `tone` into the prompt that drafts a new spec; agents materialising the spec body honour the instruction.

### (3) Everything else — always EN

Code, comments, doc-comments, templates, refs, ADRs, CONTEXT.md, JSONs, commit messages, log strings — all **English**, regardless of `specLang`. The `Lang` of a spec does not propagate into source code.

`mustard-rt run language-audit` walks the repo and reports drift (PT-BR text in EN-only files) as a soft warning. Pass `--strict` to fail the build on hits.

## Resolution Cascade (per pipeline)

Once per pipeline, in order — stop at first hit:

1. **Spec header** — `### Lang:` line in `spec.md` (re-runs, manual edits).
2. **Project preference** — `specLang` field in `.claude/mustard.json` (BCP-47).
3. **Ask once** — `AskUserQuestion`: *"Spec language: pt-BR | en-US?"*. Persist to `mustard.json#specLang` so future runs skip this step.

No textual heuristic. The user is always either explicit (steps 1-2) or asked once (step 3). Aligns with "Mustard 100% agnostic": never hardcode language signals.

## Persistence

Once resolved, write the chosen value as a header line in `spec.md` (`### Lang: pt-BR` or `### Lang: en-US`). Subsequent phases read it directly. The matching `meta.json` sidecar carries the same value.

## Header Translation Table

> **Hard rule**: when language is `pt-BR`, **all** `## ` body headings must come from the PT column. Do **not** mix EN defaults with PT body. When `en-US`, all headings stay EN. The `### Lang:` line itself is literal — never translated.

| EN (`en-US`) | PT (`pt-BR`) |
|---|---|
| `## PRD` | `## PRD` |
| `## Context` | `## Contexto` |
| `## Users/Stakeholders` | `## Usuários/Stakeholders` |
| `## Success Metric` | `## Métrica de sucesso` |
| `## Non-Goals` | `## Não-Objetivos` |
| `## Acceptance Criteria` | `## Critérios de Aceitação` |
| `## Plan` | `## Plano` |
| `## Summary` | `## Resumo` |
| `## Entity Info` | `## Informações da Entidade` |
| `## Files` | `## Arquivos` |
| `## Tasks` / `## Checklist` | `## Tarefas` |
| `## Dependencies` | `## Dependências` |
| `## Boundaries` | `## Limites` |
| `## Root cause` | `## Causa raiz` |
| `## Concerns` | `## Preocupações` |
| `## Decisions` | `## Decisões não-óbvias` |
| `## Symptom` | `## Sintoma` |

> **Single source in code**: this table is the human-readable reference; the *authoritative* heading-matching logic consumed by parsers/hooks lives in `apps/rt/src/run/spec_sections.rs` (`is_heading` + its section map). When adding or renaming a **parsed** heading, update **both**. Treat the module as truth — every spec parser resolves headings through it so EN and PT specs are recognized identically.
> **Exception**: `## PRD` and `## Plan`/`## Plano` rows are narrative dividers — intentionally absent from `SECTIONS` so no parser resolves them.

## Always EN — covers ALL code

These stay in English regardless of `specLang`:

**Spec metadata (parsed by scripts)**: status values, phase values, scope values, the language marker line itself, hook output prefixes (`[SPEC-SIZE]`, `[HYGIENE]`, `[BOUNDARY WARNING]`).

**Source code (every file the agent writes/edits)**: identifiers (variable, function, class, type, interface, enum names); file paths; shell commands + AC `Command:` content; **comments** in every form (`//`, `#`, `/* */`, `///`, `//!`, `'''`, `"""`, JSDoc, JavaDoc, XML, `<!-- -->`); log / error / exception messages; API string constants the agent introduces (unless replacing an existing localised string).

**Hard rule**: language controls only spec narrative (prose, headings, Concerns). Source code never carries `{spec_lang}`. Agents must not switch writing style based on language.

**Surgical**: never translate pre-existing comments while editing a file (karpathy §3). Only NEW comments the agent writes are in English.

**Why**: `entity-registry.json#description` is populated by `sync-registry`'s description-enricher from doc-comments and feeds `/mustard:knowledge glossary`. EN-only comments = consistent glossary; mixed comments break it.

## Dispatch Propagation

The agent-prompt template receives `{spec_lang}`. The orchestrator reads the spec header and fills it. The CONTEXT block instructs: *"Spec language is `{spec_lang}`. Use it for spec prose, labels, and Concerns you append. Source code (identifiers, comments every form, paths, commands, log messages) stays English regardless. Don't translate pre-existing comments."* Agents appending `## Concerns` or marking `[x]` inherit `{spec_lang}` automatically. Code they write does not.

## Examples

**PT spec header (`Lang: pt-BR`):**

<!-- LANG: pt-allowed -->
```markdown
# Enhancement: adicionar campo email no usuário
### Stage: Plan
### Outcome: Active
### Scope: light
### Checkpoint: 2026-05-08T10:00:00Z
### Lang: pt-BR

## Contexto
O cadastro de usuário hoje só captura nome. O time de marketing precisa enviar
campanhas por email; sem este campo, o backlog está bloqueado.

## Resumo
Adicionar coluna `email` (varchar(255), nullable) na tabela users + endpoint
de update.
```

**EN spec header (`Lang: en-US`):**

```markdown
# Enhancement: add email field to user
### Stage: Plan
### Outcome: Active
### Scope: light
### Checkpoint: 2026-05-08T10:00:00Z
### Lang: en-US

## Context
The user signup currently only captures name. The marketing team needs to send
email campaigns; without this field, the backlog is blocked.

## Summary
Add `email` column (varchar(255), nullable) to users table + update endpoint.
```
