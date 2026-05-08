# Spec Language Reference

> Detail for `/feature` and `/bugfix` — language resolution and consistency rules for spec.md.

## Resolution Cascade

Resolve the spec language **once per pipeline**, in this order (stop at first hit):

1. **Spec header** — existing `### Lang: pt` or `### Lang: en` line in `spec.md` (re-runs, manual edits) → respect it.
2. **Project preference** — field `specLang: "pt" | "en"` in `.claude/mustard.json` → use it.
3. **Ask once** — `AskUserQuestion`: `"Spec language: pt | en?"`. Persist the answer to `.claude/mustard.json#specLang` so future runs skip this step.

**No textual heuristic.** No stopword/diacritic counting. The user is always either explicit (steps 1-2) or asked once (step 3). Aligns with the "Mustard 100% agnostic" principle: never hardcode language signals.

## Persistence

Once resolved, write the chosen value as a header line in `spec.md`:

```
# {Title}
### Status: draft | Phase: PLAN | Scope: {scope}
### Checkpoint: {ISO}
### Lang: pt
```

`/resume` and any subsequent phase reads `### Lang:` directly — never re-resolves.

## Header Translation Table

> **Hard rule:** when `Lang: pt`, **ALL** `## ` body headings MUST come from the PT column. Do NOT mix EN defaults (`## Boundaries`, `## Concerns`, etc) with PT body. When `Lang: en`, all headings stay EN. The `### Lang:` line itself is literal — never translated.

| EN (default) | PT |
|---|---|
| `## Context` | `## Contexto` |
| `## Summary` | `## Resumo` |
| `## Boundaries` | `## Limites` |
| `## Files` | `## Arquivos` |
| `## Root cause` | `## Causa raiz` |
| `## Tasks` / `## Checklist` | `## Tarefas` |
| `## Plan` | `## Plano` |
| `## Acceptance Criteria` | `## Critérios de Aceitação` |
| `## Non-Goals` | `## Não-Objetivos` |
| `## Concerns` | `## Preocupações` |
| `## Decisions` | `## Decisões não-óbvias` |
| `## Dependencies` | `## Dependências` |
| `## Entity Info` | `## Informações da Entidade` |
| `## Symptom` | `## Sintoma` |

## Always EN (no translation)

These are identifiers parsed by scripts or shell — keep in English regardless of `Lang`:

- Status values: `draft | implementing | completed | cancelled`
- Phase values: `PLAN | EXECUTE | QA | CLOSE | COORDINATE`
- Scope values: `light | extended-light | full`
- AC `Command:` field content (commands run in shell)
- File paths, identifiers (variable/function/class names)
- The `### Lang:` line itself (literal)
- Hook output prefixes (`[SPEC-SIZE]`, `[HYGIENE]`, `[BOUNDARY WARNING]`)

## Dispatch Propagation

Agent dispatch template (`templates/commands/mustard/templates/agent-prompt/SKILL.md`) receives `{spec_lang}` placeholder. Orchestrator reads the spec's `### Lang:` line and fills it. The CONTEXT block instructs:

```
Spec language is `{spec_lang}`. Use `{spec_lang}` for prose, labels, and Concerns you add. Code/commands stay EN.
```

Agents adding `## Concerns` or marking `[x]` boxes inherit the language automatically.

## Contexto Narrative Rules

The `## Contexto` (Lang=pt) or `## Context` (Lang=en) section is for **humans rediscovering the work** — the briefing someone returning to the spec next week, or another team member who knows the stack but not this specific module. **Not** an agent input.

**Required structure** (one paragraph, 4-8 lines, in prose):

1. **How the system *should* work** (1-2 lines, explain domain terms on first use — e.g. "tenant", "UserTenant", "soft delete")
2. **What changed or what's expected** (1 line, reference relevant feature/PR if helpful — but as context, not jargon)
3. **How the bug/gap violates that expectation** (1-2 lines, in prose — "foi possível cadastrar X duas vezes" not "Repository.GetByCondition respects query filter")
4. **Observable impact on user or business** (1 line — not on DB internals)

**Hard rules:**

- NO tables
- NO file paths, line numbers, line citations
- NO method/class/variable names (e.g. avoid `Repository.GetByCondition`, `IgnoreQueryFilters`, `TOCTOU`)
- NO "how to fix" (that goes in Plan/Tasks)
- NO bullet lists (those are for technical sections that come later)
- Assume reader **knows the stack** (TypeScript, .NET, etc) but **NOT this module's specific architecture**
- Explain domain terms (e.g. "tenant", "UserTenant", "query filter") on first appearance, in plain language

### Bad example (do NOT do this — too technical, assumes too much)

```markdown
## Contexto
A feature recente "User reuse" (commit `4f54f2af`) firmou a invariante:
`User.Email` é globalmente único. `UserTenant` é o vínculo que materializa
acesso. O bug viola essa invariante em três caminhos de criação distintos,
todos sem proteção DB-level.
```

Why bad: assumes reader knows `UserTenant`, "query filter", "DB-level". Cites a commit hash. Reads as a compressed technical synthesis, not a narrative someone could pick up cold.

### Good example (do this — narrative, explains terms, mentions impact)

```markdown
## Contexto
No Sialia, cada pessoa cadastrada existe **uma única vez** no sistema, mesmo
que ela trabalhe em vários clientes (chamados *tenants*) ou em diferentes
plataformas. A entidade `User` representa a pessoa; `UserTenant` é o vínculo
que diz em quais clientes ela tem acesso. Essa regra foi consolidada
recentemente na feature "User reuse".

O bug reportado quebra essa regra: foi possível cadastrar a mesma pessoa
duas vezes dentro do mesmo cliente — o mesmo email aparece em duas linhas
distintas da tabela de usuários. Isso confunde relatórios, gera ambiguidade
no login e pode fazer com que permissões ou dados sejam associados ao
registro errado.

Investigando, encontramos três caminhos diferentes que criam usuários no
código, todos sem checagem global de unicidade — agravado por concorrência
sob carga e falta de normalização do email.
```

Why good: explains *tenant* on first use, says impact in user/business terms (relatórios, login, permissões), doesn't cite line numbers or method names, reads as a story someone returning to the work can follow.

### Why this matters

The technical detail belongs in `## Root cause`, `## Files`, `## Tasks` — those sections already exist below. The Contexto's job is **briefing**, not duplicating those.

A reader scanning the spec for the first time should be able to answer "what's broken and why does it matter?" from Contexto alone, without scrolling further.

## Examples

**PT spec header:**

```markdown
# Enhancement: adicionar campo email no usuário
### Status: draft | Phase: PLAN | Scope: light
### Checkpoint: 2026-05-08T10:00:00Z
### Lang: pt

## Contexto
O cadastro de usuário hoje só captura nome. O time de marketing precisa enviar
campanhas por email; sem este campo, o backlog está bloqueado.

## Resumo
Adicionar coluna `email` (varchar(255), nullable) na tabela users + endpoint
de update.
```

**EN spec header:**

```markdown
# Enhancement: add email field to user
### Status: draft | Phase: PLAN | Scope: light
### Checkpoint: 2026-05-08T10:00:00Z
### Lang: en

## Context
The user signup currently only captures name. The marketing team needs to send
email campaigns; without this field, the backlog is blocked.

## Summary
Add `email` column (varchar(255), nullable) to users table + update endpoint.
```
