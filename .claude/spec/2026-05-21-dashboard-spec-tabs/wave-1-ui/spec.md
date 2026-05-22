# Wave 1 — Tab system + SpecDetailDashboard

### Parent: [[2026-05-21-dashboard-spec-tabs]]
### Stage: Close
### Outcome: Completed
### Flags: 
### Lang: pt
### Checkpoint: 2026-05-21T16:00:00Z

## Resumo

Substituir o drill-down inline da rota `/specs` por um sistema de abas no padrão Claude Code: uma `<SpecTabBar>` no topo, com aba "Lista" sempre presente, abas de spec dinâmicas, botão `+` que abre quick-open, botão `×` por aba e botão de atualizar dados. Cada `<SpecCard>` ganha botão "Detalhes" que dispara um callback `onOpenSpec(specName)`. O conteúdo da aba de spec é o novo `<SpecDetailDashboard>` — cabeçalho com a `<PipelineTimeline>` (ANALYZE/PLAN/EXECUTE/REVIEW/QA/CLOSE) + as cinco sub-abas reaproveitadas do drill-down (Ondas, Trace, Qualidade, Rede, Sub-specs). State vive em `Specs.tsx` (route-local); sair da rota descarta.

## Contexto

`apps/dashboard/src/pages/Specs.tsx` hoje renderiza `<SpecRow>` (card + `<SpecDrillDown>` inline embaixo). Esse padrão funciona pra uma spec, mas o usuário precisa de várias abertas em paralelo. O `<SpecDrillDown>` já tem o esqueleto certo (shadcn `Tabs` com cinco abas) — vamos reaproveitá-lo como conteúdo de aba, removendo só o `<TabsList>` interno (a navegação principal sobe pra `<SpecTabBar>` no topo).

`+` abre um quick-open modal que lista todas as specs do workspace ativo (mesma fonte que a lista — `dashboardSpecCard` / `fetchSpecs`). Clicar numa spec abre como nova aba e fecha o modal. Se a spec já estiver aberta, foca a aba existente em vez de duplicar.

Atualizar (`⟳`) refetcha as queries da aba ativa: aba "Lista" refetcha `["specs"]` e `["spec-card"]`; aba de spec refetcha `["spec-card", spec]`, `["spec-waves", spec]`, `["spec-quality", spec]`, `["spec-children", spec]` (via `queryClient.invalidateQueries`). Não há refresh global porque cada aba é um contexto isolado.

`×` fecha a aba. Se a aba ativa for fechada, foca a aba à esquerda (cai pra "Lista" se for a única aba de spec). A aba "Lista" não pode ser fechada (não renderiza `×`).

## Arquivos

```
apps/dashboard/src/pages/Specs.tsx                            — refatorar para mostrar TabBar + conteúdo de aba ativa
apps/dashboard/src/components/specs/SpecTabBar.tsx            — NOVO: barra horizontal, + quick-open, × por aba, ⟳
apps/dashboard/src/components/specs/SpecDetailDashboard.tsx   — NOVO: header com PipelineTimeline + sub-abas
apps/dashboard/src/components/specs/SpecCard.tsx              — botão "Detalhes" que dispara onOpenSpec(spec)
apps/dashboard/src/components/specs/SpecDrillDown.tsx         — extrair as cinco sub-abas, sem o TabsList do topo (renderizado pelo SpecDetailDashboard)
```

## Tarefas

- [ ] Definir `SpecTab` em `Specs.tsx`: `{ id: "list", kind: "list" } | { id: string, kind: "spec", specName: string }`. State: `tabs: SpecTab[]`, `activeTabId: string`, inicial `[{id:"list",kind:"list"}]` com `activeTabId="list"`.
- [ ] Criar `<SpecTabBar tabs activeId onActivate onClose onAddRequest onRefresh>` em `apps/dashboard/src/components/specs/SpecTabBar.tsx`. Aba "Lista" tem ícone fixo, abas de spec mostram o slug truncado. Hover na aba revela `×` (a aba "Lista" não). Botões `+` e `⟳` no fim da barra. Overflow horizontal com `overflow-x-auto`.
- [ ] Quick-open: clicar `+` abre `<SpecQuickOpenDialog>` (shadcn Dialog) com input de busca + lista filtrada de specs do workspace. Selecionar = `onOpenSpec(slug)`. Dedupe: se a spec já está em `tabs`, foca em vez de adicionar.
- [ ] `onOpenSpec(spec)` em `Specs.tsx`: se `spec` já está em `tabs`, set `activeTabId`. Senão, `tabs.push({id: spec, kind:"spec", specName: spec})` + set ativo.
- [ ] `onClose(id)`: filtra a aba; se `activeTabId === id`, foca a aba à esquerda (cai pra "list" se vazio).
- [ ] `onRefresh`: usa `useQueryClient().invalidateQueries` com as keys da aba ativa (list-mode: `["specs"]` + `["spec-card"]`; spec-mode: `["spec-card", spec]`, `["spec-waves", spec]`, `["spec-quality", spec]`, `["spec-children", spec]`, `["spec-events", spec]`).
- [ ] Criar `<SpecDetailDashboard repoPath spec>` em `apps/dashboard/src/components/specs/SpecDetailDashboard.tsx`. Layout: cabeçalho `<PipelineTimeline>` em escala maior (sem `scale-[0.82]` do MiniTimeline) + status badge + duração + ondas/ACs counts; abaixo, `<SpecDrillDown>` reaproveitado.
- [ ] Refatorar `<SpecDrillDown>` para receber `tabs` opcional ou expor as 5 sub-abas igual hoje (decisão: manter as 5 sub-abas internas; o `<SpecDetailDashboard>` é o wrapper). Não duplicar a barra de abas — o `<SpecTabBar>` do topo é principal; `<SpecDrillDown>` mantém seu `<TabsList>` para as sub-abas (são níveis distintos).
- [ ] Em `<SpecCard>`: adicionar prop `onOpenSpec?: (slug: string) => void`. Renderizar botão "Detalhes" (lucide-react `Maximize2` ou texto) entre o `<PhaseChip>` e o `<FileText>` viewer. `e.stopPropagation()` no click pra não disparar o toggle herdado (que vai ser removido em seguida).
- [ ] Em `Specs.tsx`: passar `onOpenSpec` para cada `<SpecCardComponent>`. Remover o `<SpecRow>` (toggle de expand inline) e o uso de `expanded`/`setExpanded`. A `<SpecCardComponent>` agora renderiza só o card (sem drill-down).
- [ ] Deep-link via hash: `useEffect` lê `window.location.hash`; se houver, chama `onOpenSpec(hash)` no mount em vez de `setExpanded`. Mantém a UX de URL existente.
- [ ] Sair da rota descarta: como o state vive em `Specs.tsx` (component-local), o unmount já zera. Confirmar que não há ref ao state em store global (`useStore`).
- [ ] Build: `pnpm --filter mustard-dashboard build`
- [ ] Type-check: `pnpm --filter mustard-dashboard exec tsc --noEmit -p tsconfig.json` (já é parte do build)

## Acceptance Criteria

- [ ] AC-W1-1: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-W1-2: `<SpecTabBar>` existe e está importado em `Specs.tsx` — Command: `node -e "const fs=require('fs');const ok=fs.existsSync('apps/dashboard/src/components/specs/SpecTabBar.tsx')&&/SpecTabBar/.test(fs.readFileSync('apps/dashboard/src/pages/Specs.tsx','utf8'));process.exit(ok?0:1)"`
- [ ] AC-W1-3: `<SpecCard>` aceita prop `onOpenSpec` e tem botão "Detalhes" — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecCard.tsx','utf8');process.exit(/onOpenSpec/.test(s)&&/Detalhes/.test(s)?0:1)"`
- [ ] AC-W1-4: `<SpecDetailDashboard>` existe — Command: `node -e "process.exit(require('fs').existsSync('apps/dashboard/src/components/specs/SpecDetailDashboard.tsx')?0:1)"`

## Limites

- `apps/dashboard/src/pages/Specs.tsx`
- `apps/dashboard/src/components/specs/SpecTabBar.tsx` (novo)
- `apps/dashboard/src/components/specs/SpecDetailDashboard.tsx` (novo)
- `apps/dashboard/src/components/specs/SpecCard.tsx`
- `apps/dashboard/src/components/specs/SpecDrillDown.tsx`

## Network

- Parent: [[2026-05-21-dashboard-spec-tabs]]
- Bloqueia: [[wave-2-ui]], [[wave-3-ui]], [[wave-4-ui]], [[wave-5-general]], [[wave-6-general]]
