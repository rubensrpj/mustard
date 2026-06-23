---
id: wave.melhorias-no-dashboard-destacar-projeto.2-specs
---

# wave-2-specs

## Resumo

Página de Specs e UX das ondas: acelerar a primeira pintura, ícone por estágio, clique abre a onda e detalhe da onda em painel split sempre aberto (itens 3, 4, 5 e 6).

## Rede

- Pai: [[melhorias-no-dashboard-destacar-projeto]]

## Tarefas

- [ ] Remover o bloqueio da primeira pintura da rota Specs: usar placeholderData/skeleton/prefetch para não travar a tela inteira em `cardsQuery.isLoading` (Specs.tsx:420-427, 452).
- [ ] Ícone por onda refletir o estágio: mapear `wave.status` (completed/failed/queued/in_progress) para marcadores distintos no row da onda; onda concluída não exibe o ícone de andamento (SpecWavesTab/index.tsx:399-409, 51-56).
- [ ] Clicar numa onda seleciona/abre a onda e mostra seu conteúdo no painel, em vez de só abrir o drawer/reabrir a spec (SpecWavesTab/index.tsx:264; SpecDetailDashboard/index.tsx).
- [ ] Detalhe da onda vira painel split sempre aberto e redimensionável: remover o drawer `<Sheet>` e os botões de pinar/fechar; reaproveitar o SplitDetail redimensionável (WaveMarkdownDrawer/index.tsx:144-245; SpecDetailDashboard/index.tsx:42,48,76; SplitDetail/index.tsx).

## Arquivos

- `apps/dashboard/src/pages/Specs.tsx`
- `apps/dashboard/src/features/specs/SpecWavesTab/index.tsx`
- `apps/dashboard/src/features/specs/WaveMarkdownDrawer/index.tsx`
- `apps/dashboard/src/features/specs/SpecDetailDashboard/index.tsx`
- `apps/dashboard/src/components/layout/SplitDetail/index.tsx`
