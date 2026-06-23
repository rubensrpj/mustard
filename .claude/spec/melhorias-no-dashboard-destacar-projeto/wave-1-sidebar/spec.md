---
id: wave.melhorias-no-dashboard-destacar-projeto.1-sidebar
---

# wave-1-sidebar

## Resumo

Sidebar e Visão Geral: destacar o projeto selecionado e mover a versão do menu lateral para a rota de Visão Geral (itens 1 e 2).

## Rede

- Pai: [[melhorias-no-dashboard-destacar-projeto]]

## Tarefas

- [ ] Destacar o projeto selecionado no sidebar com acento visual forte (borda/anel/fundo de acento), condicionado a `isActive`, substituindo o sutil `bg-muted/40` (Sidebar/index.tsx:320-324).
- [ ] Remover o badge de versão exibido sob cada projeto no sidebar (Sidebar/index.tsx:235-241).
- [ ] Estender o DTO `ProjectOverview` no backend Tauri com o campo `version` (project_overview.rs) e propagá-lo na interface TS `ProjectOverview` (dashboard.ts:1219-1227).
- [ ] Renderizar a versão na rota de Visão Geral via ProjectInfoCard, junto ao bloco de info do projeto (ProjectInfoCard/index.tsx:414-423).

## Arquivos

- `apps/dashboard/src/components/layout/Sidebar/index.tsx`
- `apps/dashboard/src-tauri/src/project_overview.rs`
- `apps/dashboard/src/lib/dashboard.ts`
- `apps/dashboard/src/features/workspace/ProjectInfoCard/index.tsx`
