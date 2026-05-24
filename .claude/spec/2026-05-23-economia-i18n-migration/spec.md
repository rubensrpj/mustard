# Tactical Fix: Economia.tsx + filhos via i18n + cleanup de layout

## Contexto

Derivado de [[2026-05-22-economia-didatica-e-economias-reais]].

A infraestrutura de i18n já existe (`apps/dashboard/src/i18n.ts` — i18next + react-i18next, default `pt`, persistência via zustand, `<html lang>` sincronizado). Settings/Preferences/Sidebar já usam `useTranslation()`.

Falta:

1. Migrar `Economia.tsx` + componentes filhos (`PerAgentTable`, `SavingsBreakdownCard`, `ScopeBar`, `IngestionStaleBanner`, `EstimatedBySpecWave`) para `t()` em vez de strings hardcoded em PT
2. Limpar o layout: rótulo "SPANS" (inglês) → "EXECUÇÕES" via chave; remover a seção "Distribuição de tokens por agente" (duplica visualmente o `PerAgentTable` acima); ajustar o subheader `TOKENS QUE A FERRAMENTA EVITOU DE GASTAR` que duplica o h2 da seção

## Decisão de design

- **Namespace**: usar o `common` namespace existente para chaves novas, prefixadas `economy.*` (`economy.kpi.cost.title`, `economy.kpi.cache.tier.optimal`, etc.) — separa do `nav.*`/`preferences.*` sem multiplicar arquivos
- **Chaves no `i18n.ts`**: adicionar bloco PT completo + bloco EN traduzido. Manter o resto intacto
- **Não migrar SessionRow / SpecOrWaveRow para t()** se as únicas strings forem `—` (símbolo) ou data já formatada; só migrar o que é texto-PT real
- **Layout cleanup atômico**: na mesma sub-spec porque o componente é o mesmo — `PerAgentTable` mantém-se, "Distribuição" é deletada por inteiro, o subheader da savings card vira mesmo h2 da seção (uma única fonte de verdade)
- **Componentes filhos**: `<ScopeBar>` já tem labels constantes; migrar `TABS`. `<SavingsBreakdownCard>` tem `SOURCE_LABEL` + caption — migrar ambos. `<PerAgentTable>` provavelmente tem header "SPANS" — migrar pra `t('economy.table.dispatches')`
- **Plurais**: usar `_one`/`_other` (i18next nativo) para "X execuções"

## Arquivos

- `apps/dashboard/src/i18n.ts` — adicionar bloco `economy.*` em PT e EN
- `apps/dashboard/src/pages/Economia.tsx` — `import { useTranslation } from 'react-i18next'`; substituir cada string PT por `t('economy....')`; remover seção `Distribuição de tokens por agente`; simplificar header da seção savings
- `apps/dashboard/src/components/economy/ScopeBar.tsx` — labels via `t()`
- `apps/dashboard/src/components/economy/PerAgentTable.tsx` — header columns via `t()`
- `apps/dashboard/src/components/economy/SavingsBreakdownCard.tsx` — `SOURCE_LABEL` via `t()`; header card via `t()`

## Tarefas

### UI Agent (dashboard)

- [x] `i18n.ts`: adicionar chaves `economy.*` (KPI cards, secções, table headers, banner, savings labels, tier descriptions) em ambos PT e EN
- [x] `Economia.tsx`: importar `useTranslation`; migrar strings; remover seção "Distribuição"; absorver subheader da savings card no h2 da seção; verificar que `(top N)` agora respeita `_one`/`_other`
- [x] `ScopeBar.tsx`: migrar labels das 4 tabs + dropdown labels
- [x] `PerAgentTable.tsx`: migrar header "Spans"/"Agent"/"Tokens"/"Cost"
- [x] `SavingsBreakdownCard.tsx`: migrar `SOURCE_LABEL` (PT mantida como default; EN adicionada); migrar headers
- [x] `pnpm --filter mustard-dashboard build`
- [x] Verificar visualmente em ambos idiomas via Preferences toggle

## Critérios de Aceitação

- [x] AC-1: build dashboard verde — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-2: chaves PT do bloco economy presentes — Command: `bash -c "grep -q 'economy.kpi.cost' apps/dashboard/src/i18n.ts && echo ok"`
- [x] AC-3: chaves EN do bloco economy presentes — Command: `node -e "const fs=require('fs');const n=(fs.readFileSync('apps/dashboard/src/i18n.ts','utf8').match(/economy\./g)||[]).length;process.exit(n>=20?0:1)"`
- [x] AC-4: Economia.tsx usa useTranslation — Command: `bash -c "grep -q 'useTranslation' apps/dashboard/src/pages/Economia.tsx && echo ok"`
- [x] AC-5: seção duplicada removida — Command: `bash -c "test $(grep -c 'Distribuição de tokens por agente' apps/dashboard/src/pages/Economia.tsx) -eq 0 && echo ok"`
- [x] AC-6: SPANS (inglês user-facing) eliminado — Command: `node -e "const fs=require('fs');const re=/>SPANS</;const files=['apps/dashboard/src/pages/Economia.tsx','apps/dashboard/src/components/economy/PerAgentTable.tsx'];process.exit(files.some(f=>re.test(fs.readFileSync(f,'utf8')))?1:0)"`

## Limites

- Não tocar backend (pricing é [[2026-05-23-cache-aware-pricing]])
- Não criar pasta `locales/` separada — manter as resources inline em `i18n.ts` por enquanto (existing convention)
- Não migrar páginas além de Economia
- Não introduzir nova lib (i18next-react-i18next já está)
