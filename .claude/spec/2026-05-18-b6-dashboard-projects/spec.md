# Feature: b6-dashboard-projects

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-19T00:03:00Z
### Lang: pt

> Spec de backlog (Parte B, item B6). Rascunho grosso — criada em lote. Depende de B1 (monorepo) e do carimbo de versão de B5. Inclui trabalho de UI. Revisada 2026-05-18: linka os relatórios HTML gerados por B4.
>
> **Revisada 2026-05-19 (epic _self-verifiability_).** O `mustard-dashboard` é o **canal de instalação**: o cliente baixa o dashboard, seleciona o diretório, instala o Mustard por dentro dele e só então registra os repos/monorepos — o modelo de "mapear um workspace e carregar tudo" é descontinuado. O status por-projeto passa a consumir `mustard-rt run doctor` como fonte de saúde da instalação. O dashboard **já tem telemetria** — o surfacing de saúde/guardrails é extensão das views existentes, não infraestrutura nova. Ajuste pleno de escopo/tarefas quando a b6 for tratada (após o `mustard-doctor`).

## Contexto

O `mustard-dashboard` hoje funciona mapeando uma pasta-workspace única e descobrindo subprojetos dentro dela. O modelo desejado é outro: o usuário adiciona pastas de projeto individualmente, e o dashboard, para cada uma, detecta se há Mustard instalado e qual a versão — funcionando como o ponto de entrada de instalação. O app já é Tauri 2 + React 19 e já tem as peças necessárias (`plugin-dialog` para o seletor de pasta, `plugin-store` para persistir a lista, e o fan-out `useQueries` keyed por `project.path` que já suporta múltiplos projetos). O que falta é trocar o modelo de dados de "um workspace" para "registro de projetos", a detecção de versão por projeto, e o fluxo de instalar/atualizar o Mustard via o `init` nativo (Rust, de B5).

## Resumo

Reformular o `apps/dashboard`: substituir o mapeamento de um workspace por um registro de projetos (adicionar pastas individualmente via `plugin-dialog`, persistir via `plugin-store`). Por projeto: detectar instalação do Mustard e ler o carimbo de versão; indicar "atualização disponível"; oferecer instalar/atualizar invocando o `init`/`update` nativo (B5).

## Entidades

`Project` (registro local: path, nome, status de instalação, versão detectada) — entidade de estado do dashboard, não de schema de banco.

## Component Contract

- **Project list / sidebar** — lista de projetos adicionados; cada item mostra nome, status (instalado / não instalado / update disponível), versão e **saúde** (OK/WARN/FAIL, derivada de `mustard-rt run doctor`). Estados: vazio (nenhum projeto), carregando, erro de path.
- **Add-project flow** — botão → `plugin-dialog` seletor de pasta → adiciona ao registro → dispara detecção.
- **Install/update action** — por projeto sem Mustard ou desatualizado: ação que invoca o `init`/`update` nativo e reflete progresso/resultado.
- **Per-project reports (light)** — por projeto, link para os relatórios HTML que B4 gera (`qa-run`, `metrics`, `event-projections`), abertos no browser. É um link, não um viewer embutido.
- Estética atual (dark-first, Linear + Notion) preservada — muda o modelo de dados, não o visual.

## Arquivos

- `apps/dashboard/src/` — store do registro de projetos (zustand + plugin-store), hooks de detecção, componentes de lista/add/install
- `apps/dashboard/src-tauri/` — comandos Rust: detectar Mustard numa pasta, ler versão, invocar `init`/`update`
- `apps/dashboard/src/pages/`, `App.tsx`, `Sidebar.tsx` — telas do novo modelo

## Limites

- `apps/dashboard/`
- **Fora dos limites:** a CLI Rust em si (B5 — aqui apenas é invocada); o contrato `.claude/` gerado pela CLI.

## Tarefas

### Wave 1 — registro de projetos (backend Tauri)

- [x] Comandos Rust em `src-tauri/`: detectar `.claude/` Mustard numa pasta, ler o carimbo de versão (de B5), invocar `init`/`update`.
- [x] Store do registro de projetos (persistido via `plugin-store`).

### Wave 2 — UI do registro (parallel-safe após Wave 1)

- [x] Substituir o mapeamento de workspace pela lista de projetos; fluxo "adicionar projeto" via `plugin-dialog`.
- [x] Por projeto: badge de status (instalado / ausente / update disponível) + versão.
- [x] Ação de instalar/atualizar com feedback de progresso.

## Dependências

- B1 (monorepo) — o dashboard vive em `apps/dashboard`.
- B5 — o carimbo de versão e o `init`/`update` nativo invocável.
- B4 (opcional) — os relatórios HTML que o dashboard linka; sem B4, o link apenas não aparece.
- `mustard-doctor` (epic self-verifiability, fase 1) — `mustard-rt run doctor` é a fonte do indicador de saúde por-projeto. Sem ele, o card degrada para só instalado/versão.

## Preocupações

- **Carimbo de versão:** depende de B5 gravar o carimbo no `.claude/`. Sem ele, a detecção de versão não tem fonte. Se B6 rodar antes de B5, a versão fica "desconhecida" — degradar com elegância.
- **Migração de dados:** usuários do modelo atual (um workspace mapeado) precisam de um caminho para o registro de projetos sem perder o estado.
- **Guards do dashboard:** o `CLAUDE.md` do dashboard tem regras específicas (HashRouter, `useQueries` keyed por path, slices zustand, `find_mustard_root()` no Rust) — respeitar no ANALYZE.

## Critérios de Aceitação

- [x] AC-1: O dashboard builda após a reformulação — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-2: Existe comando Tauri de detecção de Mustard/versão — Command: `bash -c 'grep -rqiE "detect.*mustard|mustard.*version" apps/dashboard/src-tauri/src'`

## Não-Objetivos

- Não reescrever a estética do dashboard — só o modelo de dados e os fluxos.
- Não construir editor de glossário nem painel de harness (eventuais escopos futuros).
- Não suportar projetos remotos — só pastas locais.
- Não construir um viewer de relatórios embutido — o dashboard apenas linka os HTML que B4 gera.
