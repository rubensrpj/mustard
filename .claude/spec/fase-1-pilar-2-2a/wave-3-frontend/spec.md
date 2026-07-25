---
id: wave.fase-1-pilar-2-2a.3-frontend
---

# wave-3-frontend

## Summary

Seletor de janela (1d/7d/15d/30d) na pagina Economia, compondo com o escopo

## Network

- Parent: [[spec.fase-1-pilar-2-2a]]
- Depends on: [[wave.fase-1-pilar-2-2a.2-tauri]]

## Tasks

- [ ] Adicionar a janela ao tipo de escopo do front (economy.ts) e o from derivado via dayjs (lib/time.ts)
- [ ] Seletor com exatamente as quatro opcoes 1d/7d/15d/30d ao lado do ScopeBar (features/economy)
- [ ] Fiacao: Economia.tsx mantem o estado da janela; os wrappers invoke() (lib/dashboard.ts) e a queryKey dos hooks (useEconomySummary) incluem a janela

## Files

- `apps/dashboard/src/lib/types/economy.ts`
- `apps/dashboard/src/features/economy/ScopeBar/index.tsx`
- `apps/dashboard/src/pages/Economia.tsx`
- `apps/dashboard/src/lib/dashboard.ts`
- `apps/dashboard/src/hooks/useEconomySummary.ts`
