# Wave Plan — Economia: moat de tokens e contexto unificado

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full (wave plan)
### Checkpoint: 2026-05-21T07:05:00Z
### Lang: pt

## PRD (visão única)

O Mustard hoje grava cinco sinais de economia (RTK gain, `tokens_saved` de hooks, decisão de roteamento de modelo, tamanho de contexto enviado a agente, custo Anthropic medido) em cinco lugares diferentes — cada hook monta seu próprio JSON, cada query SQL fica espalhada no `src-tauri/` do dashboard, e a página de Economia mistura adapters com leitura de banco no mesmo arquivo. Resultado: `bash_guard` e `model_routing` emitem `tokens_saved: 0` cravado desde sempre, ninguém faz `INSERT INTO spans`, o collector OTEL nunca é iniciado após a migração JS→Rust (1 linha de spawn faltando), o JSONL local do Claude Code (que carrega o custo real e o conteúdo das requisições) nunca é parseado. Dashboard mostra zeros em campos que tecnicamente existem mas nunca recebem escrita real. Esta feature consolida o domínio de economia em `packages/core/src/economy/` espelhando o padrão SQLite (`core::store::sqlite_store`), absorve OTEL+JSONL+RTK como adapters paralelos do mesmo domain model, instrumenta hooks pra emitir números reais via estimador `tiktoken-rs`, atribui cada custo por agente (join `session_id` ↔ `agent.start`/`agent.stop` ↔ `tool_use_id`), suporta `EconomyScope` (Projeto/Spec/Wave/Comparar Projetos) como cidadão de primeira classe desde a fundação. Em paralelo, estabelece o Design System Foundation com Tailwind 4 `@theme` + CSS vars + primitivas caseiras (DiffViewer LCS, syntax highlighter, TreeNode) — base do trace viewer estilo claude-devtools que substitui Events feed da Visão Geral + timeline da página `/specs`. Fim: Economia.tsx repaginada usando único `reader::economy_summary(scope)` com scope picker funcional.

## Métrica de sucesso

Operador abre `/economia` e responde sem inferência: (a) custo real em USD desta sessão, deste spec, deste projeto, e comparado a outros projetos; (b) economia real (não estimada) que cada guard/hook impediu, com magnitude; (c) por agente despachado, quanto contexto entrou (com decomposição: prefix-stable, slice, recipe, wave-slice) e quanto retornou; (d) cache hit ratio do prompt cache da API; (e) qual modelo rodou em cada Task. Trace viewer mostra a hierarquia spec→wave→agent→tool com tokens por nível, navegável e cacheável.

## Não-Objetivos globais (valem para todas as waves)

- Não chamar a API Anthropic diretamente — Mustard observa o Claude Code via hooks + arquivos locais; zero dependência de chave de API.
- Não criar nova crate `mustard-economy` — fica como módulo dentro de `mustard-core`.
- Não migrar dados antigos do dashboard — escopo é arquitetura nova, dados velhos morrem com a feature.
- Não tocar nas tabelas existentes (`events`, `knowledge_patterns`, `memory_decisions`, `memory_lessons`) — apenas adicionar tabelas novas via migration adicional.
- Não introduzir dependências novas além de `tiktoken-rs` (Rust) e atribuição MIT para o LCS algorithm reescrito.
- Não construir Storybook — documentação inline em `apps/dashboard/src/components/ds/DS.md`.
- Não criar a "visão unificada cross-project" (Workspace global) — fica para spec separada futura. Esta entrega o reader multi-projeto e o tab "Comparar projetos" na Economia, não uma página nova.

## Tabela de Waves

| Wave | Spec                       | Role    | Modelo | Status   | Depende de                              | Resumo                                                                |
|------|----------------------------|---------|--------|----------|------------------------------------------|-----------------------------------------------------------------------|
| 1    | [[wave-1-core-economy]]    | library | opus   | draft    | —                                        | Domain model + writer + reader + estimator + `EconomyScope` + `MultiProjectReader` em `packages/core/src/economy/` |
| 2    | [[wave-2-hooks-real]]      | backend | opus   | queued   | [[wave-1-core-economy]]                  | Hooks emitem números reais via `core::economy::writer::*` — `bash_guard`, `model_routing`, `budget`, `spec_extract`, `tracker` |
| 3    | [[wave-3-ingestion]]       | backend | opus   | queued   | [[wave-1-core-economy]]                  | Adapters externos: OTEL revival (`session_start` spawn + reader) + JSONL parser (`SessionEnd` + watcher) + RTK gain |
| 4    | [[wave-4-attribution]]     | backend | opus   | queued   | [[wave-2-hooks-real]], [[wave-3-ingestion]] | Atribuição por agente: join `session_id` ↔ `agent.start`/`stop` ↔ `tool_use_id`. Readers `per_agent`/`per_spec`/`per_wave`/`per_project` |
| 5    | [[wave-5-ds-foundation]]   | ui      | opus   | queued   | — (paralela com 1-4)                     | Tailwind 4 `@theme` + CSS vars + primitivas caseiras: `DiffViewer` (LCS), `CodeBlock`, `TreeNode`, `MetricsPill`, `BaseRow` |
| 6    | [[wave-6-trace-viewer]]    | full    | opus   | queued   | [[wave-4-attribution]], [[wave-5-ds-foundation]] | Backend `dashboard_spec_trace` + frontend `<ExecutionTrace>` substituindo Events feed (Visão Geral) + timeline (/specs) |
| 7    | [[wave-7-economia-page]]   | ui      | opus   | queued   | [[wave-4-attribution]], [[wave-5-ds-foundation]] | Economia.tsx repaginada — único `reader::economy_summary(scope)` com scope picker funcional incluindo Comparar Projetos |
| 8    | [[wave-8-visao-geral-revamp]] | ui   | opus   | queued   | [[wave-5-ds-foundation]]                 | Visão Geral cosmética: i18n provider + hero multi-spec + StatusCounters substituindo MonthCalendar + Alerts/Files split 50/50 + fix top_files_today + move "economizados hoje" pro card Economia |

**Paralelismo:**
- [[wave-5-ds-foundation]] não depende de nenhuma wave anterior — pode iniciar no dia 1, em paralelo com [[wave-1-core-economy]].
- [[wave-6-trace-viewer]], [[wave-7-economia-page]] e [[wave-8-visao-geral-revamp]] podem rodar em paralelo entre si após [[wave-4-attribution]] + [[wave-5-ds-foundation]] terminarem (W8 só precisa de W5).

Planos SDD (declarados upfront, executados ao final):

| Plano  | Arquivo                       | Conteúdo                                                                                                                |
|--------|--------------------------------|-------------------------------------------------------------------------------------------------------------------------|
| Review | [[review]] (`review/spec.md`)  | Checklist 7 categorias, reviewer `sonnet`, verdict em `review/verdict.md`. Audita: SOLID, DS, Patterns, i18n, Integration, Build, Elegance |
| QA     | [[qa]] (`qa/spec.md`)          | Consolida AC do parent + 7 waves. Runner `qa-run --include-children`. Relatório em `qa/report.md`                       |

## Network

Grafo de dependências (wikilinks Obsidian — clicáveis no dashboard via [[mustard-wave-network-standard]] já entregue):

- [[wave-1-core-economy]] → [[wave-2-hooks-real]] → [[wave-4-attribution]] → [[wave-6-trace-viewer]] → [[review]] → [[qa]]
- [[wave-1-core-economy]] → [[wave-3-ingestion]] → [[wave-4-attribution]] → [[wave-7-economia-page]] → [[review]] → [[qa]]
- [[wave-5-ds-foundation]] → [[wave-6-trace-viewer]]
- [[wave-5-ds-foundation]] → [[wave-7-economia-page]]
- [[wave-5-ds-foundation]] → [[wave-8-visao-geral-revamp]]

Memória compartilhada entre waves: cada wave grava agent memory ao terminar; o orquestrador injeta o resumo da(s) wave(s) predecessor(a)(s) no prompt da próxima — `[[wave-4-attribution]]` recebe contratos de `[[wave-2-hooks-real]]` + `[[wave-3-ingestion]]`; `[[wave-6-trace-viewer]]` recebe shape do reader de `[[wave-4-attribution]]` + catálogo de primitivas de `[[wave-5-ds-foundation]]`; `[[wave-7-economia-page]]` idem.

## Critérios de Aceitação (globais — somam aos de cada wave)

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-G1: Build do core passa — Command: `cargo check -p mustard-core`
- [x] AC-G2: Testes do core passam — Command: `cargo test -p mustard-core`
- [x] AC-G3: Build do rt passa — Command: `cargo check -p mustard-rt`
- [x] AC-G4: Build do dashboard passa — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-G5: Type-check do dashboard passa — Command: `pnpm --filter mustard-dashboard exec tsc --noEmit`
- [x] AC-G6: Módulo `economy` exporta API pública esperada — Command: `node -e "const t=require('fs').readFileSync('packages/core/src/lib.rs','utf8');if(!t.includes('pub mod economy'))throw new Error('economy module not re-exported')"`
- [x] AC-G7: Todas as 8 waves marcadas `completed` no wave-plan.md (manual check final)

## Critique Coverage (auditoria explícita da conversa)

Mapeamento dos 7 pontos da crítica original do usuário sobre a Visão Geral (`2026-05-20-dashboard-visual-overview` recém-fechada). Toda crítica concreta deve cair em UMA destas categorias: coberta por wave, non-goal justificado, ou surfaced pra decisão.

| # | Crítica original | Cobertura | Wave |
|---|---|---|---|
| 1 | Hero útil só pra 1 spec; "eventos/min" técnico demais; "economizados hoje" no lugar errado | ✅ coberto | [[wave-8-visao-geral-revamp]] |
| 2a | SPECS POR STATUS deveria ocupar width inteira | ✅ coberto | [[wave-8-visao-geral-revamp]] |
| 2b | SPECS POR STATUS deve obedecer língua escolhida em Preferences | ✅ coberto | [[wave-8-visao-geral-revamp]] (i18n provider) |
| 3 | ECONOMIA DE TOKENS sempre vazia | ✅ coberto | [[wave-1-core-economy]] → [[wave-2-hooks-real]] → [[wave-3-ingestion]] → [[wave-4-attribution]] → [[wave-7-economia-page]] (toda a stack) |
| 4 | MonthCalendar grande, não diz nada, deveria virar status counters | ✅ coberto | [[wave-8-visao-geral-revamp]] |
| 5 | Events feed deveria ser trace hierárquico estilo claude-devtools, reusável em /specs | ✅ coberto | [[wave-6-trace-viewer]] |
| 6 | Alerts + Files columns finos, sem ícones, sem split 50/50 | ✅ coberto | [[wave-8-visao-geral-revamp]] |
| 7 | Bug `top_files_today` esvazia pós-CLOSE | ✅ coberto | [[wave-8-visao-geral-revamp]] |

**Decisões de não-objetivo explícitas (da conversa):**
- API direta Anthropic — non-goal global (`## Não-Objetivos globais`): Mustard observa Claude Code via hooks+JSONL local, nunca chama a API.
- Storybook — non-goal global: documentação inline em `DS.md`.
- Migrar pages legadas para i18n — non-goal de W8: lazy, só Visão Geral nesta wave.
- "Visão unificada cross-project" (Workspace global como página nova) — explícito non-goal global; entrega apenas o reader multi-projeto + tab "Comparar projetos" na Economia.
- Refactor das ~40 páginas existentes pra consumir DS — non-goal de W5: migração lazy.

## Limites globais

```
ESCOPO:
  packages/core/Cargo.toml
  packages/core/src/lib.rs
  packages/core/src/economy/**
  packages/core/src/store/migrations.rs (apenas ADICIONAR migrations novas, nunca alterar existentes)
  packages/core/tests/economy_*.rs
  apps/rt/src/hooks/{bash_guard,model_routing,budget,tracker,session_start,session_cleanup}.rs
  apps/rt/src/run/{otel,spec_extract,rtk_gain}.rs (e descendentes — virar adapters)
  apps/rt/src/run/transcript_parser.rs (novo)
  apps/dashboard/src-tauri/src/{telemetry.rs,db.rs,main.rs,lib.rs}
  apps/dashboard/src/pages/Economia.tsx
  apps/dashboard/src/pages/Specs.tsx (apenas substituir timeline+events tab pelo novo trace)
  apps/dashboard/src/pages/Workspace.tsx (apenas substituir EventsFeed pelo novo trace)
  apps/dashboard/src/components/{ds,trace,economy}/**
  apps/dashboard/src/styles/theme.css (e descendentes)
  apps/dashboard/src/hooks/{useEconomySummary,useSpecTrace,useDsTheme}.ts (novos)

OUT-OF-BOUNDS:
  apps/cli/** (CLI não muda)
  packages/core/src/{model,store/event_store.rs,store/sqlite_store.rs,projection,reader,knowledge}/** (não alterar — apenas usar)
  apps/dashboard/src/components/workspace/Workspace{StatusBar,AlertsColumn,SpecsByStatus,TokenSummary,MonthCalendar,FilesRanking}.tsx (Wave anterior)
  apps/dashboard/src/components/specs/** (exceto remoção de timeline e events se Wave 6 substituir)
  Qualquer schema change em `events`, `knowledge_patterns`, `memory_decisions`, `memory_lessons`
  Qualquer chave de API Anthropic ou call HTTP para api.anthropic.com
```
