# Enhancement: dashboard-activity-feed

### Status: closed | Phase: CLOSE | Scope: light
### Checkpoint: 2026-05-13T03:20:34.000Z
### Lang: pt

## Contexto

A timeline cross-project que adicionamos no Aggregate Home limita-se a 20 eventos para caber acima da seção de Projetos, suficiente para skimming mas insuficiente para investigar uma cadeia de ações ("o que rodou nas últimas 4 horas?"). A Sidebar ainda exibe "Activity — soon" como placeholder, e o operador que quer correlacionar eventos entre projetos cai no terminal lendo `events.jsonl` direto. O backend já entrega `dashboard_recent_events(repo_path, limit)` aceitando limit dinâmico — só falta uma página dedicada que peça mais e ofereça filtro por tipo. O resultado é que a frase "Activity full-screen cross-project" no roadmap nunca sai do placeholder.

## Resumo

Ativar "Activity" da Sidebar como rota `/activity`. Página densa com filtro multi-select por `event_type` (chips toggleáveis derivados dos tipos presentes nos resultados), busca limit configurável (100 por projeto), e lista paginada client-side por chunks de 50 com botão "Carregar mais". Cmd+K ganha entry "Ir para Activity".

## Checklist

### Frontend Agent

- [x] Criar `src/hooks/useActivityFeed.ts` exportando `useActivityFeed(projects: Project[], limitPerProject: number)` que retorna `{ events, loading, types }`. Usa `useQueries` com `queryKey: ['activity-feed', p.path, limitPerProject]`, `queryFn: () => fetchRecentEvents(p.path, limitPerProject)`, `staleTime: 15_000`. Flatten em `{ projectId, projectName, event }`, ordena por ts desc. `types`: Set distinct de event_type presentes para alimentar o filtro.
- [x] Criar `src/pages/Activity.tsx`: header "Activity cross-project" + breadcrumb. Linha de filtros: chips dos types (toggle multi-select, active=Badge default, idle=Badge outline). Lista densa: por padrão render primeiros 50, botão "Carregar mais 50" no final se houver mais. Cada row: StatusDot por tipo (mesma fn `eventVariant` que já existe em ProjectDetail — copiar inline ou exportar). Coluna: dot + Badge event_type + projeto + relativeTime + summary truncado 200. Empty: "Sem eventos." Loading: 5 skeleton rows.
- [x] Editar `src/components/layout/Sidebar.tsx` substituindo o `<div className={disabledItemClass}>` de Activity por `<NavLink to="/activity" className={navItemClass}>` mantendo o ícone Activity.
- [x] Editar `src/App.tsx` adicionando `<Route path="/activity" element={<Activity />} />`.
- [x] Editar `src/components/CommandPalette.tsx` adicionando `Command.Item` "Ir para Activity" no grupo "Navegar".
- [x] Rodar `pnpm exec tsc --noEmit` e garantir zero erros.

## Arquivos (~5)

- `src/hooks/useActivityFeed.ts` (NEW)
- `src/pages/Activity.tsx` (NEW)
- `src/components/layout/Sidebar.tsx` (edit)
- `src/App.tsx` (edit)
- `src/components/CommandPalette.tsx` (edit)

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: TypeScript type-check passa sem erros — Command: `pnpm exec tsc --noEmit`
- [x] AC-2: Arquivos novos `useActivityFeed.ts` e `Activity.tsx` existem — Command: `node -e "const f=require('fs'); process.exit(f.existsSync('src/hooks/useActivityFeed.ts') && f.existsSync('src/pages/Activity.tsx') ? 0 : 1)"`
- [x] AC-3: Rota `/activity` registrada em `App.tsx` — Command: `node -e "const s=require('fs').readFileSync('src/App.tsx','utf8'); process.exit(s.includes('path=\"/activity\"') && s.includes('<Activity') ? 0 : 1)"`
- [x] AC-4: Sidebar Activity é NavLink ativa — Command: `node -e "const s=require('fs').readFileSync('src/components/layout/Sidebar.tsx','utf8'); process.exit(/<NavLink[^>]*to=\"\\/activity\"/.test(s) ? 0 : 1)"`
- [x] AC-5: useActivityFeed usa useQueries + fetchRecentEvents — Command: `node -e "const s=require('fs').readFileSync('src/hooks/useActivityFeed.ts','utf8'); process.exit(s.includes('useQueries') && s.includes('fetchRecentEvents') ? 0 : 1)"`
