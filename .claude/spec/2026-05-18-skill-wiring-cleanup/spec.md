# Feature: skill-wiring-cleanup

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-18T21:15:34Z
### Lang: pt

> Spec de backlog (Parte A, item A6.1 — desdobramento). Criada em 2026-05-18: a A6 já concluiu adicionando as skills do Matt verbatim, mas sem wiring de comando. Esta spec liga as duas que faltam e remove o `frontend-design` (redundante com `design-craft`).

## Contexto

A A6, já concluída, adicionou quatro skills do Matt verbatim a `templates/skills/`. Mas ela rodou sem uma fase de wiring — então as skills estão instaladas e dormentes: nenhum comando as recomenda, e suas descrições são gated por intenção explícita do usuário, que um run de pipeline não expressa. A análise por comando definiu onde cada uma rende: `diagnose` no `/bugfix`, `improve-codebase-architecture` no `/task`, e `grill-with-docs` na PLAN do `/feature` (esta última fica na A1, por ser o produtor do `CONTEXT.md`). Esta spec faz o wiring mínimo das duas primeiras. E, de quebra, remove o `frontend-design` — um skill órfão (grep confirma zero referências fora do próprio arquivo) cujo conteúdo o `design-craft` já absorveu há tempos: a seção "Frontend Aesthetics" do `design-craft` é o núcleo do `frontend-design` quase verbatim, e o `design-craft` empilha toda uma metodologia em cima. O `frontend-design` é o ancestral; manter os dois é redundância de skill.

## Resumo

(1) Ligar `diagnose` como skill recomendada default dos agentes do `/bugfix`. (2) Ligar `improve-codebase-architecture` como recomendada no passo ASSESS de `/task refactor` e `/task audit`. (3) Dobrar o único callout que falta do `frontend-design` no `design-craft` e então remover o `frontend-design`.

## Entidades

N/A — wiring e faxina de skills.

## Component Contract

N/A.

## Arquivos (~5)

- `templates/refs/agent-prompt/agent-prompt.md` — regras de `{recommended_skills}`
- `templates/commands/mustard/bugfix/SKILL.md` — recomendar `diagnose`
- `templates/commands/mustard/task/SKILL.md` — recomendar `improve-codebase-architecture` no ASSESS
- `templates/skills/design-craft/SKILL.md` — absorver o callout anti-convergência
- `templates/skills/frontend-design/` — removida

## Limites

- `templates/refs/agent-prompt/`, `templates/commands/mustard/{bugfix,task}/`, `templates/skills/design-craft/`, `templates/skills/frontend-design/`
- **Fora dos limites:** o conteúdo verbatim das skills do Matt; `grill-with-docs` (wiring vai na A1); demais skills.

## Tarefas

### Templates Agent (Wave 1) — wiring das skills do Matt

- [x] Em `refs/agent-prompt/agent-prompt.md` (§ regras de `{recommended_skills}`): nova regra — agentes do `/bugfix`, **incluindo o Explore de diagnóstico**, recebem `diagnose` (exceção explícita à regra "Explore recebe skills mínimas" — o loop de diagnóstico é exatamente o método do Explore de bug). Agentes de `/task refactor` e `/task audit` recebem `improve-codebase-architecture` no passo ASSESS.
- [x] Em `bugfix/SKILL.md`: citar `diagnose` no dispatch dos agentes de diagnóstico e de fix.
- [x] Em `task/SKILL.md`: citar `improve-codebase-architecture` no ASSESS das ações `refactor` e `audit`.
- [x] Rodar `npm run build`.

### Templates Agent (Wave 2) — faxina do frontend-design

- [x] Em `design-craft/SKILL.md`, lista "NEVER": acrescentar o callout que falta — fontes "da moda" também viram clichê (ex.: não convergir em Space Grotesk entre gerações) e variar entre temas claro/escuro. É a única coisa do `frontend-design` ainda não coberta.
- [x] Remover `templates/skills/frontend-design/`. Grep confirmou: zero referências fora do próprio `SKILL.md` — órfão. Padronizar em `design-craft` (já é o escolhido pelas regras de `{recommended_skills}`).
- [x] Rodar `npm run build`.

## Dependências

- **A6** (`skills-absorption`) ✓ — `diagnose` e `improve-codebase-architecture` já existem.
- Independente da A1 (a A1 cuida do `grill-with-docs`).

## Preocupações

- Nenhuma grande. O `frontend-design` é órfão confirmado por grep; a regra de `{recommended_skills}` já favorece `design-craft`.

## Critérios de Aceitação

- [x] AC-1: O `frontend-design` foi removido — Command: `node -e "const fs=require('fs');if(fs.existsSync('templates/skills/frontend-design'))process.exit(1)"`
- [x] AC-2: O `bugfix/SKILL.md` recomenda `diagnose` — Command: `bash -c 'grep -q "diagnose" templates/commands/mustard/bugfix/SKILL.md'`
- [x] AC-3: O `task/SKILL.md` recomenda `improve-codebase-architecture` — Command: `bash -c 'grep -q "improve-codebase-architecture" templates/commands/mustard/task/SKILL.md'`
- [x] AC-4: Build e type-check passam — Command: `npm run build`

## Não-Objetivos

- Não adaptar o conteúdo das skills do Matt.
- Não ligar o `grill-with-docs` aqui — vai na A1.
- Não mexer em outras skills além de remover o `frontend-design` e o callout no `design-craft`.
