# Plano de Waves

## Tabela de Waves

| Wave | Spec | Papel | Depende de | Resumo |
|------|------|-------|------------|--------|
| 1 | [[wave-1-backend]] | backend | — | Comandos Tauri de git local e overview de projeto no backend do dashboard (camada de dados para a seção Projetos). |
| 2 | [[wave-2-frontend]] | frontend | [[wave-1-backend]] | Redesenho da rota Visão Geral em duas seções (Specs + Projetos), consumindo os comandos da Onda 1 e reusando componentes existentes; remoção de ROI/Economia/Timeline. |

## Critérios de Aceitação
- **AC-1** — O workspace Rust compila com os comandos e structs novos. Command: `cargo build --workspace`
- **AC-4** — O comando dashboard_git_info existe e está registrado no invoke_handler. Command: `grep -rn "dashboard_git_info" apps/dashboard/src-tauri/src/lib.rs`
- **AC-5** — O backend expõe info de projeto além de name/role (linguagens/monorepo). Command: `grep -rnE "languages|frameworks|monorepo|project_count" apps/dashboard/src-tauri/src`
- **AC-2** — O dashboard (frontend + bindings) compila. Command: `pnpm --filter mustard-dashboard build`
- **AC-3** — Lint limpo no dashboard. Command: `pnpm --filter mustard-dashboard lint`
- **AC-6** — A rota Visão Geral não referencia mais os widgets removidos. Command: `grep -qE "RoiScoreboard|RecentActivity" apps/dashboard/src/features/workspace/AggregateOverview/index.tsx && exit 1 || exit 0`

<!-- wikilinks-footer-start -->
- [wave-1-backend](?) ⚠ unresolved
- [wave-2-frontend](?) ⚠ unresolved
<!-- wikilinks-footer-end -->