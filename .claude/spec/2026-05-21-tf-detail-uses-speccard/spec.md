# Tactical Fix — Detail usa SpecCard + fases menores

## PRD

## Contexto

`<SpecDetailDashboard>` renderiza um header customizado (slug + StatusPill + PhaseChip + traço + ...) em vez de reusar `<SpecCard>`. Por isso o detalhe mostra PhaseChip ("plan"), traço "—" e rodapé incompleto (`ACs/arquivos/tools` sem ondas/modelo/duração) enquanto a Lista mostra o SpecCard polido. Usuário pediu "mesmo componente nas duas páginas" — solução é o Detalhes USAR `<SpecCard>` no header (sem botão Detalhes porque já está em detalhe). Adicionalmente, o `<PhaseStation>` ficou grande demais (`h-10` circles); usuário quer menor (`h-8`), aplicado uniformemente em Lista e Detalhes.

## Métrica de sucesso

Detalhes e Lista mostram o MESMO `<SpecCard>` no topo — bit-para-bit visualmente idêntico. PhaseStation circles passam de `h-10` (40px) para `h-8` (32px) — render único, ambas rotas atualizadas juntas. Detail view mantém o `<SpecDrillDown>` (Ondas/Trace/Qualidade/Rede) abaixo do card.

## Acceptance Criteria

- [x] AC-1: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-2: SpecDetailDashboard usa SpecCard — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecDetailDashboard.tsx','utf8');process.exit(/SpecCard\\s+as|import.*SpecCard.*from/.test(s)&&/<SpecCard/.test(s)?0:1)"`
- [x] AC-3: PhaseStation usa h-8 (não mais h-10) — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/telemetry/PhaseStation.tsx','utf8');process.exit(s.includes('h-8 w-8')&&!s.includes('h-10 w-10')?0:1)"`
- [x] AC-4: SpecCard renderiza Detalhes condicionalmente — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecCard.tsx','utf8');process.exit(s.includes('onOpenSpec &&')?0:1)"`

## Plano

## Summary

Refatorar `SpecDetailDashboard` para reusar `<SpecCard>`. Tornar o botão "Detalhes" condicional em `<SpecCard>` (só renderiza quando `onOpenSpec` é fornecido). Reduzir tamanho das fases.

## Checklist

### dashboard-impl Agent

- [x] **(1) SpecCard — Detalhes condicional.** Em `apps/dashboard/src/components/specs/SpecCard.tsx`, envolver o botão "Detalhes" em `{onOpenSpec && (<button ... />)}`. Quando `onOpenSpec` é `undefined` (caso do Detail view), o botão some.

- [x] **(2) SpecDetailDashboard usa SpecCard.** Em `apps/dashboard/src/components/specs/SpecDetailDashboard.tsx`:
  - Importar `SpecCard` de `@/components/specs/SpecCard`.
  - REMOVER o header customizado (o `<div>` com slug + StatusPill + PhaseChip + traço + PipelineTimeline + bottom row de ACs/arquivos/tools).
  - Renderizar `<SpecCard data={specCardData} repoPath={repoPath} />` (SEM `onOpenSpec`).
  - Abaixo, `<SpecDrillDown ... />` com as sub-abas (já existente).
  - Aproveitar a query `useQuery(['spec-card', ...])` que já existia; só passar o resultado pro `<SpecCard>`.
  - Loading skeleton: quando `specCardQ.data == null`, mostrar o skeleton do card (ou um placeholder).

- [x] **(3) PhaseStation menor — h-8 globalmente.** Em `apps/dashboard/src/components/telemetry/PhaseStation.tsx`:
  - `circleSize`: `"h-10 w-10"` → `"h-8 w-8"`.
  - `iconSize`: `"h-5 w-5"` → `"h-4 w-4"`.
  - `labelSize`: mantém `"text-[12px] font-medium"`.
  - `activeRing`: mantém `"ring-2"`.
  - `minWidth`: `"min-w-[56px]"` → `"min-w-[52px]"`.
  - Em `PipelineTimeline.tsx`: ajustar `connectorTop` de `top-[20px]` (centro de h-10=40) para `top-[16px]` (centro de h-8=32). Ajustar `connectorInset` de `left-8 right-8` (32px) para `left-6 right-6` (24px).

- [x] Build verde: `pnpm --filter mustard-dashboard build`.

## Files (~4)

- `apps/dashboard/src/components/specs/SpecCard.tsx`
- `apps/dashboard/src/components/specs/SpecDetailDashboard.tsx`
- `apps/dashboard/src/components/telemetry/PhaseStation.tsx`
- `apps/dashboard/src/components/telemetry/PipelineTimeline.tsx`
