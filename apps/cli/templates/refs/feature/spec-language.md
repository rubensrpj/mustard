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

> **Two-layer spec structure (`/feature`)**: `## PRD` and `## Plano` are `##`-level **divider headings** that group subsections. `## PRD` = the *what & why* (Contexto, Usuários/Stakeholders, Métrica de sucesso, Não-Objetivos, Critérios de Aceitação). `## Plano` = the *how* (Resumo, Informações da Entidade, Arquivos, Tarefas, Dependências, Limites). The spec stays a **single file**. PRD/Plano dividers + Usuários/Stakeholders + Métrica de sucesso are narrative-only — no parser consumes them.
>
> **Per-layer narrative rules**: the PRD layer is prose for humans (briefing, who/why, observable success) — no file paths, no method names, no code. The Plano layer is the technical breakdown (entities, files, tasks) and may carry paths and identifiers. Success Metric / Métrica de sucesso states an observable outcome (e.g. *"o cadastro aceita email e o relatório de campanha lista o usuário"*), never an implementation detail. Users/Stakeholders names who is affected and who requested the change.

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

## Contexto Narrative Rules

The `## Contexto` (PT) or `## Context` (EN) section is for **humans rediscovering the work** — the briefing someone returning to the spec next week, or another team member who knows the stack but not this specific module. **Not** an agent input.

**Required structure** (one paragraph, 4-8 lines, in prose):

1. **How the system *should* work** (1-2 lines, explain domain terms on first use — e.g. *tenant*, *UserTenant*, *soft delete*).
2. **What changed or what's expected** (1 line; reference relevant feature/PR if helpful — as context, not jargon).
3. **How the bug/gap violates that expectation** (1-2 lines, in prose — *"foi possível cadastrar X duas vezes"* not *"Repository.GetByCondition respects query filter"*).
4. **Observable impact on user or business** (1 line — not on DB internals).

**Hard rules**: NO tables. NO file paths, line numbers, line citations. NO method/class/variable names. NO "how to fix" (that goes in Plan/Tasks). NO bullet lists. Assume reader **knows the stack** but **NOT this module's specific architecture**. Explain domain terms on first appearance, in plain language.

### Bad example (do NOT do this)

<!-- LANG: pt-allowed -->
```markdown
## Contexto
A feature recente "User reuse" (commit `4f54f2af`) firmou a invariante:
`User.Email` é globalmente único. `UserTenant` é o vínculo que materializa
acesso. O bug viola essa invariante em três caminhos de criação distintos,
todos sem proteção DB-level.
```

Why bad: assumes reader knows `UserTenant`, "query filter", "DB-level". Cites a commit hash. Reads as a compressed technical synthesis, not a narrative.

### Good example

<!-- LANG: pt-allowed -->
```markdown
## Contexto
No Sialia, cada pessoa cadastrada existe **uma única vez** no sistema, mesmo
que ela trabalhe em vários clientes (chamados *tenants*) ou em diferentes
plataformas. A entidade `User` representa a pessoa; `UserTenant` é o vínculo
que diz em quais clientes ela tem acesso.

O bug reportado quebra essa regra: foi possível cadastrar a mesma pessoa
duas vezes dentro do mesmo cliente — o mesmo email aparece em duas linhas
distintas da tabela de usuários. Isso confunde relatórios, gera ambiguidade
no login e pode fazer com que permissões ou dados sejam associados ao
registro errado.
```

Why good: explains *tenant* on first use, says impact in user/business terms, doesn't cite line numbers or method names, reads as a story someone returning to the work can follow.

### Why this matters

Technical detail belongs in `## Root cause`, `## Files`, `## Tasks`. The Contexto's job is **briefing**. A reader scanning the spec should answer *"what's broken and why does it matter?"* from Contexto alone.

## Component Contract (UI specs only)

Append a `## Component Contract` section between `## Files` and `## Tasks` when the spec creates or refactors a UI component. **Skip for non-UI work** — adding it to backend/database specs is bloat.

**When to add**: new component file (`*.tsx`/`*.vue`/`*.svelte`/`*.dart`/`*.swift`); component refactoring (props/variants); form/input creation.

**Template (PT)**:

<!-- LANG: pt-allowed -->
```markdown
## Contrato do Componente
- **Props:** `{prop}: {tipo}` — required vs optional explícito
- **Estados:** loading | empty | error | success | disabled (todos visíveis e testáveis)
- **Variantes:** size (sm/md/lg) | color (primary/secondary/...) | density (compact/regular)
- **Breakpoints:** xs | sm | md | lg | xl — comportamento em cada
- **A11y:** roles ARIA | tab order | aria-* | focus-visible | contrast token
- **DS tokens consumidos:** color.* | spacing.* | typography.* (NÃO valores literais)
- **Microinterações:** hover/focus/active distintos; respeita `prefers-reduced-motion`
```

**Template (EN)**: same shape with English labels — Props/States/Variants/Breakpoints/A11y/DS tokens consumed/Microinteractions.

**Why this section matters (anti-AI-look)**: without an explicit contract, FE agents improvise variants/states/a11y → "AI-look" output (literal colors, missing empty states, no microinteractions). The contract forces explicit decisions before code touches files. → See `refs/stack-templates/fe-craft-check.md` for the full checklist applied at review.
