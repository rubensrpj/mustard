---
id: wave.melhorias-no-dashboard-destacar-projeto.plan
---

# Plano de Waves

## Tabela de Waves

| Wave | Spec | Papel | Depende de | Resumo |
|------|------|-------|------------|--------|
| 1 | [[wave-1-sidebar]] | sidebar | — | Sidebar e Visão Geral: destacar o projeto selecionado e mover a versão do menu lateral para a rota de Visão Geral (itens 1 e 2). |
| 2 | [[wave-2-specs]] | specs | — | Página de Specs e UX das ondas: acelerar a primeira pintura, ícone por estágio, clique abre a onda e detalhe da onda em painel split sempre aberto (itens 3, 4, 5 e 6). |

## Critérios de Aceitação
- **AC-1** — Typecheck do dashboard verde. Command: `pnpm --filter mustard-dashboard typecheck`
- **AC-2** — Backend Tauri compila com `version` no DTO. Command: `cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml`
- **AC-3** — Linha do projeto ativo com destaque distinto condicionado a `isActive` (não apenas `bg-muted/40`). Command: `rg -n "isActive" apps/dashboard/src/components/layout/Sidebar/index.tsx`
- **AC-4** — Versão sai do sidebar e aparece na Visão Geral; `ProjectOverview` carrega `version`. Command: `rg -n "version" apps/dashboard/src-tauri/src/project_overview.rs apps/dashboard/src/features/workspace/ProjectInfoCard/index.tsx`
- **AC-5** — Typecheck do dashboard verde. Command: `pnpm --filter mustard-dashboard typecheck`
- **AC-6** — Primeira entrada em Specs não bloqueia a tela inteira em isLoading (skeleton/placeholderData/prefetch). Command: `rg -n "placeholderData|Skeleton|prefetch|isLoading" apps/dashboard/src/pages/Specs.tsx`
- **AC-7** — Ícone por onda distinto por estágio (concluída != andamento). Command: `rg -n "completed|in_progress|failed|queued" apps/dashboard/src/features/specs/SpecWavesTab/index.tsx`
- **AC-8** — Clicar numa onda abre/seleciona a onda; detalhe em painel split sempre aberto e redimensionável, sem `<Sheet>` nem pinar/fechar. Command: `rg -n "Sheet|PinIcon|drawerPinned" apps/dashboard/src/features/specs/WaveMarkdownDrawer/index.tsx apps/dashboard/src/features/specs/SpecDetailDashboard/index.tsx`
