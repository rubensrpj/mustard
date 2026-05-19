# Enhancement: dashboard-aggregate-home

### Status: closed | Phase: CLOSE | Scope: light
### Checkpoint: 2026-05-13T03:13:33.000Z
### Lang: pt

## Contexto

A Home do Mustard Dashboard hoje exige seleção de um projeto para qualquer dado útil aparecer — sem seleção, os cards de Métricas/Knowledge mostram "Selecione um projeto" e a única coisa visível é uma lista plana de projetos descobertos. O operador que mantém múltiplos projetos perde a visão de conjunto: não sabe quantas pipelines estão rodando agora, em qual projeto está a maior atividade, nem qual spec foi tocada por último sem clicar projeto-a-projeto. O backend já expõe `dashboard_specs`, `dashboard_recent_events` e `dashboard_metrics` por projeto, mas a UI nunca agrega. O resultado é que a tela inicial entrega menos do que um `git log --all` agregaria — invertendo a premissa do dashboard.

## Resumo

Redesenhar `Home.tsx` colocando um bloco `AggregateOverview` no topo com 4 contadores (specs ativas, em EXECUTE, completed 7d, eventos hoje), lista de pipelines ativas cross-project com link direto para SpecDetail, e timeline unificada dos últimos eventos com badge de origem. Lista de projetos demovida para seção abaixo. Implementação client-side via `useQueries` paralelo sem nova IPC/Rust.

## Checklist

### Frontend Agent

- [x] Criar `src/hooks/useAggregate.ts` com `useAggregate(projects: Project[])` retornando `{ counters, activePipelines, timeline, loading }`. Usa `useQueries` chamando `fetchSpecs` + `fetchRecentEvents` por projeto. Counters: `activeSpecs` (specs com status != 'closed' OU phase ∈ {ANALYZE,PLAN,EXECUTE,QA}), `executing` (phase === 'EXECUTE'), `completed7d` (completed_at nos últimos 7 dias), `eventsToday` (events com ts >= meia-noite local). `activePipelines`: flatten de specs por projeto com `{ projectId, projectName, spec }`, filtra phase ativa, ordena por started_at desc. `timeline`: flatten de recentEvents (limit 10 por projeto) com `{ projectId, projectName, event }`, ordena por ts desc, slice top 20.
- [x] Criar `src/components/AggregateOverview.tsx` recebendo `{ projects: Project[] }` e renderizando 3 seções: (a) grid 4 contadores compactos (cada um: número grande + label pequeno, lucide icon discreto, fallback "—" se loading); (b) "Pipelines ativas" — lista densa, vazio mostra "Sem pipelines ativas." Cada row: StatusDot por phase + project name (muted) + " / " + spec name (mono) + Badge phase + relativeTime started_at; click → navigate(`/project/${projectId}/spec/${encodeURIComponent(spec.name)}`); (c) "Atividade recente" — lista de eventos com Badge `event_type` + project name (text-xs muted) + relativeTime + summary truncado a 120 chars. Empty: "Sem eventos recentes."
- [x] Refatorar `src/pages/Home.tsx`: importar AggregateOverview, renderizar no topo passando `projects ?? []` (skip se discovering/no-root), demote a lista de Projetos para uma seção secundária abaixo, REMOVER os Cards de Métricas/Knowledge selecionado (a info agora é cross-project nos counters). Manter empty state quando `!projectsRoot`.
- [x] Rodar `pnpm exec tsc --noEmit` e garantir zero erros.

## Arquivos (~3)

- `src/hooks/useAggregate.ts` (NEW)
- `src/components/AggregateOverview.tsx` (NEW)
- `src/pages/Home.tsx` (refactor)

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: TypeScript type-check passa sem erros — Command: `pnpm exec tsc --noEmit`
- [x] AC-2: Arquivos novos `useAggregate.ts` e `AggregateOverview.tsx` existem — Command: `node -e "const f=require('fs'); process.exit(f.existsSync('src/hooks/useAggregate.ts') && f.existsSync('src/components/AggregateOverview.tsx') ? 0 : 1)"`
- [x] AC-3: Home importa e renderiza AggregateOverview — Command: `node -e "const s=require('fs').readFileSync('src/pages/Home.tsx','utf8'); process.exit(s.includes('AggregateOverview') && s.includes('useDashboard')===false ? 0 : 1)"`
- [x] AC-4: useAggregate usa useQueries do react-query — Command: `node -e "const s=require('fs').readFileSync('src/hooks/useAggregate.ts','utf8'); process.exit(s.includes('useQueries') && s.includes('fetchSpecs') && s.includes('fetchRecentEvents') ? 0 : 1)"`
