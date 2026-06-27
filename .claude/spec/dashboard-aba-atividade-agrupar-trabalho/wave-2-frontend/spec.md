---
id: wave.dashboard-aba-atividade-agrupar-trabalho.2-frontend
---

# wave-2-frontend

## Summary

Aba Atividade (substitui Specs): agrupa por rotulo humano mapeado do kind + cada item mostra pedido original + narrativa

## Network

- Parent: [[dashboard-aba-atividade-agrupar-trabalho]]
- Depends on: [[wave-1-backend]]

## Tasks

- [ ] Specs.tsx vira Atividade: agrupar por rotulo humano (Nova funcionalidade/Ajuste/Correcao/Follow-up/Investigacao/Mudanca rapida) mapeado do kind, modelando em Sessions.tsx
- [ ] Cada item mostra o pedido original como titulo + a narrativa ao abrir (pedido > fases > mudancas > desfecho)
- [ ] Usar CollapsibleGroup; hooks useAggregate/useTelemetryTimeline pra puxar kind+narrativa
- [ ] Trocar a entrada de navegacao Specs > Atividade

## Files

- `apps/dashboard/src/pages/Specs.tsx`
- `apps/dashboard/src/pages/Sessions.tsx`
- `apps/dashboard/src/components/page/CollapsibleGroup/index.tsx`
- `apps/dashboard/src/hooks/useAggregate.ts`
- `apps/dashboard/src/hooks/useTelemetryTimeline.ts`
