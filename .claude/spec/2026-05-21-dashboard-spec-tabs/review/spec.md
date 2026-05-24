# Review — dashboard-spec-tabs

## Resumo

Code review consolidado após todas as 6 waves rodarem. Dois reviews em paralelo: um pra `apps/dashboard/` (UI + Tauri) e outro pra `apps/rt/` + `packages/core/` (Rust subcommands + helper de union).

## Tarefas

- [ ] Dispatch `dashboard` review agent. Foco:
  - SpecTabBar / SpecDetailDashboard limpos e sem duplicação com SpecDrillDown.
  - `Specs.tsx` mantém state local (sem leak pra `useStore`).
  - Hooks novos (`useSpecWaveFiles`, `useSpecMemoryCrossWave`) seguem o padrão de `useSpecWaves` (queryKey por `[repoPath, spec, ...]`, `enabled`, `staleTime`).
  - `WaveMarkdownDrawer` usa shadcn `Sheet` corretamente (sem props removidos).
  - `react-markdown` v10: nada de `inline` prop em `code`.
  - `spec-graph-layout` é puro (sem efeitos colaterais), determinístico com seed se houver.
  - Tipos: `SpecChild.source` é opcional pra back-compat.
  - Acessibilidade: tabs e botões com `aria-label`, navegação por teclado funciona.
- [ ] Dispatch `rt` review agent. Foco:
  - `wave_files`, `spec_children`, `memory_cross_wave` seguem o padrão fail-open / JSON byte-stable / `cmd /C` ou `sh -c` correto.
  - Sem `unwrap` em prod. Tests inclusos.
  - `memory_cross_wave` não regride no schema de eventos (não escreve novos kinds).
  - Helper de union em `mustard-core` (se portado) tem testes.
- [ ] Consolidar verdict. Tactical-fix candidates surface se houver fricção pequena.

## Acceptance Criteria

- [ ] AC-R-1: Build full passa — Command: `cargo build --workspace`
- [ ] AC-R-2: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-R-3: Testes rt + core passam — Command: `cargo test -p mustard-core -p mustard-rt`

## Limites

Sem mudança de código própria — só consolidação de findings.

## Network

- Parent: [[2026-05-21-dashboard-spec-tabs]]
- Depende: [[wave-1-ui]], [[wave-2-ui]], [[wave-3-ui]], [[wave-4-ui]], [[wave-5-general]], [[wave-6-general]]
