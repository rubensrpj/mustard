# Feature: harness-legibility

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-18T23:55:00Z
### Lang: pt

> Spec de backlog (Parte A, item A5). Rascunho criado em lote — passa por ANALYZE de refino quando for aprovada via `/approve`.

## Contexto

Um harness é a camada que disciplina o agente — no Mustard, os hooks e os gates do pipeline. A crítica do repositório `mattpocock/skills` é direta: frameworks que tomam conta do processo tiram o controle do usuário e tornam bugs no próprio processo difíceis de resolver. Hoje, quando um gate do Mustard bloqueia — o `close-gate` negando o CLOSE, o `context-budget` recusando um Task — a mensagem diz que bloqueou, mas raramente explica com clareza o que foi violado e como o usuário contorna. O harness age, mas não se explica. Um harness premium não é o mais forte; é o mais legível e escapável. Esta mudança faz cada gate emitir uma mensagem que diz o que violou, por que, e qual a saída (env var, ajuste), e adiciona uma visão `/status --harness` que lista os hooks ativos e o que cada um enforça.

## Resumo

Padronizar as mensagens de bloqueio dos hooks de gate para o formato "o quê / por quê / como contornar" (incluindo a env var de override quando existir), e adicionar uma visão `/status --harness` que enumera os hooks ativos do `settings.json` com o que cada um faz e seu modo atual.

## Entidades

N/A — infraestrutura de enforcement.

## Component Contract

N/A — sem trabalho de UI.

## Arquivos (~7)

- `templates/hooks/close-gate.js` — mensagem de bloqueio explicativa
- `templates/hooks/context-budget.js` — idem
- `templates/hooks/output-budget.js` — idem (advisory)
- `templates/hooks/spec-size-gate.js` — idem
- `templates/hooks/review-gate.js` — idem
- `templates/hooks/enforce-registry.js` — idem
- `templates/commands/mustard/status/SKILL.md` — visão `--harness`
- (possível) `templates/hooks/_lib/` — helper compartilhado de formatação de mensagem

## Limites

- `templates/hooks/` (apenas hooks de gate listados; mensagens, não lógica de decisão), `templates/hooks/_lib/`, `templates/commands/mustard/status/SKILL.md`
- **Fora dos limites:** a lógica de quando bloquear (não muda — só a mensagem); hooks que não são gates; `settings.json` (lido pela visão `--harness`, não editado); `src/`.

## Tarefas

### Templates Agent (Wave 1) — mensagens auto-explicativas

- [x] Criar um helper em `templates/hooks/_lib/` que formata mensagem de gate como: `[GATE] {o quê foi violado}. {por quê}. Saída: {env var / ação para contornar}`.
- [x] Aplicar o helper em `close-gate.js`, `context-budget.js`, `output-budget.js`, `spec-size-gate.js`, `review-gate.js`, `enforce-registry.js` — sem alterar a lógica de decisão, só o texto emitido.
- [x] Garantir que cada mensagem cite a env var de override quando ela existir (ex.: `MUSTARD_QA_GATE_MODE`, `MUSTARD_CLOSE_GATE_MODE`, `CONTEXT_BUDGET_MODE`).
- [x] Rodar os testes de hooks (`bun test templates/hooks/__tests__/hooks.test.js`) e `npm run build`.

### Templates Agent (Wave 2) — visão do harness

- [x] Em `status/SKILL.md`: adicionar a flag `--harness` que lê `settings.json`, lista os hooks registrados por evento de ciclo de vida, e mostra o que cada um enforça + o modo atual (lido das env vars).
- [x] Rodar `npm run build`.

## Dependências

- Independente das demais specs da Parte A.
- Wave 2 depende de Wave 1 apenas por consistência de mensagem (não é bloqueante).

## Preocupações

- **Fail-open inviolável:** os hooks devem continuar fail-open (exit 0 em erro). O helper de mensagem não pode introduzir um caminho que lance exceção e quebre o hook.
- **Não virar verborragia:** a mensagem explicativa precisa ser curta (1-3 linhas). Mensagem longa em todo bloqueio vira ruído — o oposto do objetivo.

## Critérios de Aceitação

- [x] AC-1: O helper de formatação de gate existe — Command: `bash -c 'ls templates/hooks/_lib/ | grep -qi gate'`
- [x] AC-2: `status/SKILL.md` documenta a flag `--harness` — Command: `bash -c 'grep -q "\\-\\-harness" templates/commands/mustard/status/SKILL.md'`
- [x] AC-3: Os testes de hooks passam — Command: `bash -c 'bun test templates/hooks/__tests__/hooks.test.js'`
- [x] AC-4: Build e type-check passam — Command: `npm run build`

## Não-Objetivos

- Não alterar QUANDO um gate bloqueia — só COMO ele comunica o bloqueio.
- Não remover hooks nem afrouxar enforcement — legibilidade, não permissividade.
- Não construir um painel de harness no dashboard (eventual escopo da Parte B).
