# Wave 3 — 5 componentes da Visão Geral em components/workspace/

## PRD

## Contexto

Com hooks (Wave 2) e badges semânticos (Wave 1b) prontos, esta wave entrega os 5 componentes UI que a Visão Geral redesenhada renderiza. Cada componente é autônomo: prop única `repoPath`, consome hook próprio, usa `<DataCard padded>` + `<SectionHeader>` do barrel `components/page`, badges semânticos via novas variants. A integração final (montar no `Workspace.tsx`) é a Wave 4 — esta wave não toca a página.

## Métrica de sucesso

5 arquivos novos em `components/workspace/`, cada um renderizável isolado (smoke render no Vite dev), type-check passa.

## Não-Objetivos

- Não tocar `Workspace.tsx` (Wave 4).
- Não criar testes unitários — verificação é via build/type-check + smoke render manual.
- Não estilizar fora de tokens existentes (Tailwind 4 + tokens do tema atual).

## Acceptance Criteria

- [x] AC-1: Type-check passa — Command: `pnpm --filter mustard-dashboard exec tsc --noEmit`
- [x] AC-2: 5 arquivos existem — Command: `node -e "['WorkspaceSpecsByStatus','WorkspaceTokenSummary','WorkspaceMonthCalendar','WorkspaceEventsFeed','WorkspaceFilesRanking'].forEach(c=>{const p='apps/dashboard/src/components/workspace/'+c+'.tsx';if(!require('fs').existsSync(p))throw new Error('missing '+p)})"`
- [x] AC-3: MonthCalendar tem estado mês/ano — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/components/workspace/WorkspaceMonthCalendar.tsx','utf8');if(!/useState[^;]*month/i.test(t)||!/useState[^;]*year/i.test(t))throw new Error('missing month/year state')"`
- [x] AC-4: EventsFeed renderiza deep-link spec — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/components/workspace/WorkspaceEventsFeed.tsx','utf8');if(!t.includes('/specs#'))throw new Error('missing spec deep-link')"`
- [x] AC-5: SpecsByStatus tem filtro de período — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/components/workspace/WorkspaceSpecsByStatus.tsx','utf8');['Hoje','7d','30d'].forEach(p=>{if(!t.includes(p))throw new Error('missing period '+p)})"`

## Plano

## Arquivos (~5)

```
apps/dashboard/src/components/workspace/WorkspaceSpecsByStatus.tsx   (new)
apps/dashboard/src/components/workspace/WorkspaceTokenSummary.tsx    (new)
apps/dashboard/src/components/workspace/WorkspaceMonthCalendar.tsx   (new)
apps/dashboard/src/components/workspace/WorkspaceEventsFeed.tsx      (new)
apps/dashboard/src/components/workspace/WorkspaceFilesRanking.tsx    (new)
```

## Component Contract (compartilhado)

- Prop única obrigatória: `repoPath: string` (não-null garantido pelo parent).
- Consome hook próprio (`useWorkspace*`) — nunca chama `invoke()` direto.
- Renderiza dentro de `<DataCard padded>` com `<SectionHeader title=… right=…/>`.
- Estado vazio: `<EmptyState>` do barrel `components/page`.
- Badges via `<Badge variant="success|warning|error|info|status-*">` — nunca cor inline.
- Navegação via `useNavigate()` do `react-router`. Spec deep-link: `/specs#${spec}`.
- Apenas `WorkspaceMonthCalendar` é stateful (mês/ano em `useState`); demais são puramente derivados do hook.

## Tarefas

### Frontend UI Agent

- [x] `WorkspaceSpecsByStatus.tsx`:
  - Consome `useWorkspaceSummarySingle(repoPath)` (já existe) — não cria hook novo
  - Segmented control no cabeçalho: `Hoje | 7d | 30d` (estado local, filtra `tracks` por data do último evento)
  - Grid de chips: cada status com `<Badge variant="status-...">` + contagem grande à direita
  - Footer: `<a href="/specs">Ver detalhes →</a>`
- [x] `WorkspaceTokenSummary.tsx`:
  - Consome `useWorkspaceTokenSummary(repoPath)`
  - Card com número grande formatado (`Intl.NumberFormat`), subtítulo "últimos 30 dias"
  - Lista mini com top 3 pipelines (`<Badge variant="info">{spec}</Badge> {saved}`)
  - Footer: `<a href="/economia">Ver detalhes →</a>`
- [x] `WorkspaceMonthCalendar.tsx`:
  - State `[year, setYear]`, `[month, setMonth]` (default = mês corrente)
  - Header com chevrons (`<` `>`) que decrementam/incrementam mês — passa pro ano quando ultrapassa
  - Consome `useWorkspaceMonthActivity(repoPath, year, month)`
  - Grid 7×6 (semana + dia): cada célula com número do dia + bg colorido por faixa de densidade (`event_count`):
    - 0 → bg transparente
    - 1-3 → `bg-info/15`
    - 4-9 → `bg-warning/25`
    - 10+ → `bg-success/35` (alta atividade, não erro)
  - Click numa célula: `navigate('/specs?date=' + date)`
  - Hover: tooltip simples `{event_count} eventos · {top_phase}`
- [x] `WorkspaceEventsFeed.tsx`:
  - Consome `useWorkspaceEventsFeed(repoPath, 50)`
  - Lista cronológica reversa: cada linha `[<Badge variant=kindToVariant(kind)>{kind}</Badge>] {relativeTime(ts)} · {payload_summary} · {spec ? <a href={'/specs#'+spec}>{spec}</a> : ''}`
  - `kindToVariant`: `pipeline.status → info`, `pipeline.complete → success`, `pipeline.dispatch_failure → error`, `pipeline.scope → info`, qualquer outro `→ info`
  - `relativeTime`: implementar pequeno helper (sem importar lib pesada) — "há 3min", "há 2h", "ontem 14:32"
  - Limite visual: 50 entradas, scroll vertical interno (`max-h-[480px] overflow-y-auto`)
- [x] `WorkspaceFilesRanking.tsx`:
  - Consome `useWorkspaceSummarySingle(repoPath)` — usa `top_files_today` (já existe no shape)
  - Tabela compacta: arquivo (truncado à esquerda) | hits (direita)
  - Top 10, sem paginação
- [x] `pnpm --filter mustard-dashboard exec tsc --noEmit`

## Dependências

- [[wave-1-badges]]: novos variants no `badge.tsx`.
- [[wave-2-data]]: 3 hooks `useWorkspace*`.

## Network

- Parent: [[2026-05-20-dashboard-visual-overview]]
- Depende de: [[wave-1-badges]], [[wave-2-data]]
- Desbloqueia: [[wave-4-integration]]
- Recebe memória: [[wave-1-badges]] (lista de variants disponíveis), [[wave-2-data]] (signatures dos hooks).
- Grava memória: `{components_created: [...], hooks_used: [...], badge_variants_used: [...], notes: "..."}` para [[wave-4-integration]].

## Limites

Em escopo: `apps/dashboard/src/components/workspace/Workspace{SpecsByStatus,TokenSummary,MonthCalendar,EventsFeed,FilesRanking}.tsx`.

Fora de escopo: `Workspace.tsx` (Wave 4), todo o resto.
