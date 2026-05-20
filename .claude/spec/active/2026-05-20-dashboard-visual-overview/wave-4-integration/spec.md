# Wave 4 — Integração: reescrever Workspace.tsx montando as 5 visualizações

### Parent: [[2026-05-20-dashboard-visual-overview]]
### Status: queued
### Phase: PLAN
### Scope: full (wave)
### Checkpoint: 2026-05-20T22:30:00Z
### Lang: pt

## PRD

## Contexto

Com os 5 componentes prontos (Wave 3), esta wave reescreve `Workspace.tsx` removendo as visualizações antigas (EffortHeatmap fixo de "hoje", SpecTracksList que duplica `/specs`, WorkspaceEffortFooter que cabe melhor como `WorkspaceFilesRanking` na sidebar) e monta o novo layout. É a única wave que toca `pages/Workspace.tsx` — todas as anteriores são adições isoladas.

## Métrica de sucesso

`Workspace.tsx` reescrito renderiza as 5 visualizações sem regressão de hero (StatusBar + PipelineTimeline permanecem), build/lint/type-check passam, smoke manual no `pnpm dev` mostra a nova página sem erros no console.

## Não-Objetivos

- Não introduzir nova rota.
- Não alterar `WorkspaceStatusBar`, `PipelineTimeline`, `WorkspaceAlertsColumn` (reusa intactos).
- Não migrar `SpecTracksList` para um lugar novo — só removê-lo do `Workspace.tsx`. Arquivo pode permanecer no repo (uso futuro). Não excluir.

## Acceptance Criteria

- [x] AC-1: Build passa — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-2: Lint passa — Command: `pnpm --filter mustard-dashboard lint`
- [x] AC-3: 5 componentes importados — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/pages/Workspace.tsx','utf8');['WorkspaceSpecsByStatus','WorkspaceTokenSummary','WorkspaceMonthCalendar','WorkspaceEventsFeed','WorkspaceFilesRanking'].forEach(c=>{if(!t.includes(c))throw new Error('missing '+c)})"`
- [x] AC-4: Imports obsoletos removidos — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/pages/Workspace.tsx','utf8');['EffortHeatmap','SpecTracksList','WorkspaceEffortFooter','useTelemetryHeatmap'].forEach(s=>{if(t.includes(s))throw new Error('residual import '+s)})"`
- [x] AC-5: Hero preservado — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/pages/Workspace.tsx','utf8');['WorkspaceStatusBar','PipelineTimeline','WorkspaceAlertsColumn'].forEach(c=>{if(!t.includes(c))throw new Error('hero broken: missing '+c)})"`

## Plano

## Arquivos (~1)

```
apps/dashboard/src/pages/Workspace.tsx   (rewrite layout)
```

## Layout alvo

```
┌─────────────────────────────────────────────────────────────────┐
│ PageHeader (Visão Geral · Sala de operações multi-track)        │
├─────────────────────────────────────────────────────────────────┤
│ DataCard:  WorkspaceStatusBar  +  PipelineTimeline (hero)        │
├──────────────────────────────────────┬──────────────────────────┤
│ WorkspaceSpecsByStatus  (col-span 2) │ WorkspaceTokenSummary    │
├──────────────────────────────────────┴──────────────────────────┤
│ WorkspaceMonthCalendar (full-width)                             │
├──────────────────────────────────────┬──────────────────────────┤
│ WorkspaceEventsFeed (main, flex-1)   │ WorkspaceAlertsColumn    │
│                                      │ WorkspaceFilesRanking    │
└──────────────────────────────────────┴──────────────────────────┘
```

## Tarefas

### Frontend Integration Agent

- [x] Substituir os imports da página: remover `EffortHeatmap`, `SpecTracksList`, `WorkspaceEffortFooter`, `useTelemetryHeatmap`; adicionar os 5 `Workspace*` novos
- [x] Manter o hook `useWorkspaceSummarySingle(activeProject?.path ?? null)` (precisa por causa do hero) e remover `useTelemetryHeatmap`
- [x] Hero permanece: `<DataCard padded>` com `<WorkspaceStatusBar>` + `<PipelineTimeline>` (sem mudanças no shape)
- [x] Linha de KPIs (substitui o "Atividade hoje" + EffortHeatmap): `<div className="grid grid-cols-3 gap-6">` com `WorkspaceSpecsByStatus` (`col-span-2`) + `WorkspaceTokenSummary` (`col-span-1`)
- [x] `WorkspaceMonthCalendar` full-width dentro de `<DataCard padded>` abaixo dos KPIs
- [x] Bloco principal: `<div className="flex gap-6">` com `<main className="flex-1 min-w-0">` contendo `<WorkspaceEventsFeed>` e `<aside className="w-[280px] shrink-0">` contendo `<WorkspaceAlertsColumn>` + `<WorkspaceFilesRanking>` empilhados (`gap-6`)
- [x] Empty states preservados (sem projetos / sem workspace selecionado / loading) — passar `repoPath = activeProject?.path` para os 5 componentes
- [x] `pnpm --filter mustard-dashboard lint && pnpm --filter mustard-dashboard build`

## Dependências

- [[wave-3-ui]]: 5 componentes prontos.

## Network

- Parent: [[2026-05-20-dashboard-visual-overview]]
- Depende de: [[wave-3-ui]]
- Desbloqueia: QA (Wave 10) → CLOSE
- Recebe memória: [[wave-3-ui]] (componentes + props), [[wave-1-backend]] (commands disponíveis, indireto via wave-3-ui).
- Grava memória: `{workspace_layout_summary: "...", removed_imports: [...], notes: "..."}` para a fase QA.

## Limites

Em escopo: `apps/dashboard/src/pages/Workspace.tsx`.

Fora de escopo: todos os componentes (eles estão prontos da Wave 3), Sidebar, Topbar, outras pages.
