# Spec Language

> Loaded by `/feature` + `/bugfix`. Law as checklist, mappings as table. Rationale: `docs/TEMPLATE-RATIONALE.md` (never loaded).

## Law

- Spec narrative locale is BCP-47 only (`pt-BR`, `en-US`, …); short codes (`pt`, `en`) are rejected. Never mix languages inside one spec.
- Resolve the locale ONCE per pipeline, first hit wins: (1) `meta.json#lang` → (2) `mustard.json#specLang` → (3) ask once via AskUserQuestion, persist to `mustard.json#specLang`. No textual heuristic, ever.
- `spec.md` carries no `### Lang:` (or any lifecycle) header — metadata lives only in the `meta.json` sidecar (`spec-draft`/`wave-scaffold` write it; later phases read it there).
- Tone is a separate dimension: `mustard.json#tone` = `didactic` (default) | `technical` | `concise`; `spec-draft` wires it into the drafting prompt. Language never changes tone.
- Everything that is code stays English regardless of locale: identifiers, file paths, shell + AC `Command:` lines, comments in EVERY form, log/error/exception strings, API string constants (unless replacing an already-localised one). Never translate pre-existing comments while editing; only new comments you write are English. `mustard-rt run language-audit` reports drift (`--strict` fails the build).
- A `pt-BR` spec uses ALL PT `##` headings below; `en-US` keeps all EN. The `lang` value is a literal code, never translated. Banners are catalogued for `pt-BR` and `en-US`; other BCP-47 codes are accepted for the body and fall back to the default banner catalogue.

## Headings (EN ↔ PT)

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

- `## PRD` and `## Plan`/`## Plano` are narrative DIVIDERS (PRD = what/why; Plano = how) grouping subsections in one file — intentionally absent from the parser map. The authoritative matcher is `apps/rt/src/run/spec_sections.rs`: when adding or renaming a PARSED heading, update the table AND the module.

## Contexto rules

- Audience: a human rediscovering the work next week — a briefing, not agent input. One prose paragraph, 4-8 lines, in order: how the system should work (explain each domain term on first use) → what changed / is expected → how the bug or gap violates that → the observable user/business impact.
- Forbidden in Contexto: tables, file paths, line numbers, method/class/variable names, "how to fix", bullet lists — those belong in `## Root cause` / `## Files` / `## Tasks`. The PRD layer is prose-only (no paths, no identifiers); the Plano layer may carry them. `Métrica de sucesso` states an observable outcome, never an implementation detail.

## Dispatch

Agent prompts receive `{spec_lang}` (from `meta.json#lang`): spec prose, labels and appended `## Concerns` use it; the code the agent writes stays English.

## Component Contract (UI specs only)

Add `## Component Contract` between `## Files` and `## Tasks` ONLY when the spec creates or refactors a UI component (new component file, props/variants refactor, form creation) — never on non-UI specs. Shape (PT or EN labels to match the spec): Props (`{prop}: {type}`, required vs optional explicit) · States (loading | empty | error | success | disabled — all visible and testable) · Variants (size | color | density) · Breakpoints (xs–xl behaviour) · A11y (ARIA roles, tab order, `aria-*`, focus-visible, contrast token) · DS tokens consumed (never literal values) · Microinteractions (hover/focus/active distinct; respects `prefers-reduced-motion`).
