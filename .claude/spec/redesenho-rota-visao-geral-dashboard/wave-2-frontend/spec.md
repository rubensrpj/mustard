# wave-2-frontend

## Resumo

Redesenho da rota Visão Geral em duas seções (Specs + Projetos), consumindo os comandos da Onda 1 e reusando componentes existentes; remoção de ROI/Economia/Timeline.

## Rede

- Pai: [[redesenho-rota-visao-geral-dashboard]]
- Depende de: [[wave-1-backend]]

## Tarefas

- [ ] Bindings em dashboard.ts: fetchGitInfo e fetchProjectOverview + tipos GitInfo e ProjectOverview (skill dashboard-tauri-pattern).
- [ ] Hooks useGitInfo.ts e useProjectOverview.ts (skill dashboard-use-pattern: queryKey com repoPath na folha, enabled: !!repoPath, estado vazio tolerante).
- [ ] Componente SpecStatusCards: 3 cards de estágio (Planejando/Executando/Finalizadas) com contagem derivada de fetchSpecCards (campo phase/status); clicar navega para /specs?filter=<estágio>.
- [ ] Componente SpecAlertsBand: faixa de Alertas com Suspeitas (de workspace_health/suspect_specs) e Specs paradas (stale: ativas sem evento há >= 7 dias, derivado de last_event_at); clicar navega para o filtro correspondente.
- [ ] Componente ProjectInfoCard: monorepo + nº de projetos + linguagens/stacks (useProjectOverview).
- [ ] Componente GitInfoCard: branch + remote + ahead/behind + último commit (useGitInfo).
- [ ] Editar Specs.tsx: ler params de estágio (planejando/executando/finalizadas) e stale; derivar estágio de SpecCard.phase/status; mapear para bucket + sub-filtro de estágio.
- [ ] Editar AggregateOverview: remover RoiScoreboard, bloco agregado de Consumo & Economia, os 4 KPIs soltos e a Timeline de atividade; reestruturar em seção Specs (SpecStatusCards + SpecAlertsBand) e seção Projetos (ProjectInfoCard + GitInfoCard + WorkspaceFilesRanking reusado).
- [ ] Usar o design system existente (DataCard, SectionHeader, StatPill, StatusDot, Tailwind + variáveis de intent); navegação via o router (HashRouter) e invalidação por watcher, sem polling.

## Arquivos

- `apps/dashboard/src/lib/dashboard.ts`
- `apps/dashboard/src/hooks/useGitInfo.ts`
- `apps/dashboard/src/hooks/useProjectOverview.ts`
- `apps/dashboard/src/features/workspace/SpecStatusCards/index.tsx`
- `apps/dashboard/src/features/workspace/SpecAlertsBand/index.tsx`
- `apps/dashboard/src/features/workspace/ProjectInfoCard/index.tsx`
- `apps/dashboard/src/features/workspace/GitInfoCard/index.tsx`
- `apps/dashboard/src/features/workspace/AggregateOverview/index.tsx`
- `apps/dashboard/src/pages/Specs.tsx`
