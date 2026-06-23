---
id: spec.melhorias-no-dashboard-destacar-projeto
---

# melhorias no dashboard: destacar projeto selecionado no sidebar, mover versao para a visao geral, acelerar a rota de specs, icones de onda refletem o estagio, clicar onda abre a onda, detalhe da onda em split sem drawer

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

melhorias no dashboard: destacar projeto selecionado no sidebar, mover versao para a visao geral, acelerar a rota de specs, icones de onda refletem o estagio, clicar onda abre a onda, detalhe da onda em split sem drawer.

Âncoras (do scan):
- apps/dashboard/src/components/layout/SplitDetail/index.tsx (detail, panel, split)
- packages/core/src/view/projection/card.rs (project, specs, wave, status)
- apps/rt/src/commands/event/emit_pipeline.rs (wave, status, stage, completed)
- apps/dashboard/src-tauri/src/project_overview.rs (project, version, overview)
- apps/mcp/src/lib.rs (project, specs, wave, status)
- apps/cli/templates/skills/skill-creator/scripts/run_loop.py (split)
- apps/scan/src/extract.rs (split)
- apps/dashboard/src/lib/store.ts (sidebar, selected, project, active)
- apps/dashboard/src/components/layout/Sidebar/index.tsx (sidebar, project, active, status)
- apps/dashboard/src/hooks/useProjectOverview.ts (project, overview)
- apps/dashboard/src/lib/types/specs.ts (active, specs, wave, status)
- packages/core/src/domain/model/view/spec.rs (active, wave, status, stage)

Por que agora. O dashboard é a face de uso diário do mustard, e seis atritos de navegação/leitura se acumularam: o projeto selecionado no menu lateral (sidebar) não se distingue dos demais; a versão do projeto polui o sidebar e fica difícil de ler; a primeira abertura da rota de Specs trava por alguns segundos; os ícones de cada onda (wave) mostram sempre "em andamento" mesmo quando a onda já concluiu; clicar numa onda não leva à onda; e o detalhe da onda abre como uma gaveta lateral (drawer) apertada, com botões de pinar/fechar que o usuário não quer. São melhorias incrementais (nenhuma entidade nova), mas tocam duas camadas (React e o backend Tauri em Rust) e cerca de nove arquivos.

## Usuários/Stakeholders

Quem usa o dashboard do mustard para acompanhar pipelines, specs e ondas — hoje o próprio mantenedor (dogfooding) e futuros usuários do curso. Toda a dor é de navegação e legibilidade na interface.

## Métrica de sucesso

- O projeto selecionado é imediatamente reconhecível no sidebar (destaque visual forte, não sutil).
- A versão sai do sidebar e passa a ser lida com clareza na Visão Geral.
- A primeira entrada em Specs não bloqueia a tela — mostra esqueleto/lista rápido.
- Cada onda exibe um ícone coerente com seu estágio (concluída ≠ em andamento).
- Clicar numa onda abre a onda; o detalhe vive num painel split sempre aberto e redimensionável, sem drawer nem pinar/fechar.

## Não-Objetivos

- Não redesenhar o sidebar inteiro nem o tema de cores — apenas o destaque do item ativo.
- Não reescrever a página de Specs nem o sistema de abas — apenas tirar o bloqueio da primeira pintura.
- Não mexer na geração/semântica de eventos de pipeline no backend `apps/rt` — o estágio da onda já é conhecido; o conserto do ícone (item 4) é de renderização no front.
- Não criar entidade nova nem comando Tauri novo além de carregar a versão na Visão Geral.

## Critérios de Aceitação

- **AC-1** — Typecheck do dashboard verde após todas as mudanças.
  Command: `pnpm --filter mustard-dashboard typecheck`
- **AC-2** — Backend Tauri compila (extensão do `ProjectOverview` com versão).
  Command: `cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml`
- **AC-3** (item 1) — A linha do projeto ativo no sidebar aplica destaque visual distinto (acento/borda/anel), condicionado a `isActive`, e não mais apenas `bg-muted/40`.
  Command: `rg -n "isActive" apps/dashboard/src/components/layout/Sidebar/index.tsx`
- **AC-4** (item 2) — A versão não aparece mais no sidebar e passa a ser exibida na Visão Geral; `ProjectOverview` carrega `version`.
  Command: `rg -n "version" apps/dashboard/src-tauri/src/project_overview.rs apps/dashboard/src/features/workspace/ProjectInfoCard/index.tsx`
- **AC-5** (item 4) — Ícone por onda reflete o estágio: concluída/falha/fila/andamento têm marcadores distintos (não "andamento" fixo).
  Command: `rg -n "completed|in_progress|failed|queued" apps/dashboard/src/features/specs/SpecWavesTab/index.tsx`
- **AC-6** (item 5) — Clicar numa onda seleciona/abre a onda (mostra o conteúdo da onda no painel), sem reabrir a spec.
  Command: `pnpm --filter mustard-dashboard typecheck`
- **AC-7** (item 6) — Detalhe da onda renderiza como painel split sempre aberto e redimensionável; o drawer `<Sheet>` e os botões de pinar/fechar foram removidos do fluxo da onda.
  Command: `rg -n "Sheet|PinIcon|drawerPinned" apps/dashboard/src/features/specs/WaveMarkdownDrawer/index.tsx apps/dashboard/src/features/specs/SpecDetailDashboard/index.tsx`
- **AC-8** (item 3) — A primeira entrada na rota Specs não bloqueia a tela inteira em `isLoading` (esqueleto/placeholder/prefetch presentes).
  Command: `rg -n "placeholderData|isLoading|prefetch|Skeleton" apps/dashboard/src/pages/Specs.tsx`

<!-- PLAN -->

## Arquivos

Censo (união das três ondas — o spec pai é documento de coordenação; tarefas/AC por onda vivem no plano de ondas):

Onda A — Sidebar (itens 1 e 2):
- apps/dashboard/src/components/layout/Sidebar/index.tsx — destaque do projeto ativo (320–324); remover badge de versão (235–241)
- apps/dashboard/src-tauri/src/project_overview.rs — incluir `version` no DTO de overview
- apps/dashboard/src/lib/dashboard.ts — interface `ProjectOverview` ganha `version` (1219–1227)
- apps/dashboard/src/features/workspace/ProjectInfoCard/index.tsx — renderizar versão na Visão Geral (414–423)

Onda B — Performance da rota Specs (item 3):
- apps/dashboard/src/pages/Specs.tsx — tirar bloqueio da 1ª pintura (placeholderData/skeleton/prefetch; 420–427, 452)

Onda C — UX das ondas (itens 4, 5 e 6):
- apps/dashboard/src/features/specs/SpecWavesTab/index.tsx — ícone por estágio (399–409, 51–56); clique abre a onda (264)
- apps/dashboard/src/features/specs/WaveMarkdownDrawer/index.tsx — virar painel split sempre aberto; remover `<Sheet>`/pin/close (144–245)
- apps/dashboard/src/features/specs/SpecDetailDashboard/index.tsx — estado `openWave`/`drawerPinned` → seleção sempre aberta em split (42, 48, 76)
- apps/dashboard/src/components/layout/SplitDetail/index.tsx — painel split redimensionável reaproveitável (referência)

## Limites

IN: apps/dashboard (frontend React + backend Tauri/Rust em src-tauri). Mudanças de UI/UX, um campo `version` no DTO de overview, e ajuste de carregamento da rota Specs.
OUT: apps/rt, apps/scan, packages/core, apps/mcp, apps/cli. Nenhuma alteração na geração de eventos de pipeline, na CLI ou no modelo do grain.

## Preocupações

- Item 4 (ícone "andamento" em onda concluída): pelas imagens do usuário, o selo já mostra "CONCLUÍDA", logo o `wave.status` chega correto e o conserto é só de renderização no front (mapear `wave.status`→ícone por estágio em `SpecWavesTab`). **Porém**, se na execução o `wave.status` chegar errado/estagnado do backend (o `meta.json`/eventos `pipeline.wave.complete` não refletirem a conclusão), isso deixa de ser de UI e vira investigação na Onda 2 — possivelmente tocando a origem em `apps/rt` (fora do escopo IN atual). Sinalizar como BLOCKED/concern se aparecer, em vez de mascarar no front.