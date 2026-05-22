# Wave 3 — Dashboard: SpecRow + StageBullet + agrupamento + árvore expansível

### Parent: [[2026-05-21-spec-lifecycle-unification]]
### Wave: 3
### Role: dashboard
### Stage: Close
### Outcome: Completed
### Flags: 
### Lang: pt
### Checkpoint: 2026-05-22T00:58:00Z

## Resumo

Refaz a rota `/specs` como lista densa estilo Linear. Substitui `SpecCard` (que ocupava ~150px por spec) por `SpecRow` (1 linha de ~32px), introduz `StageBullet` (SVG ring com 5 segmentos pintados por Stage), agrupa specs por Stage com count colapsável, e adiciona árvore expansível mostrando waves + ACs + sub-specs em sub-níveis indentados. Lazy-load do `spec_children_tree` ao expandir.

A página de detalhe (`/spec/{name}`) e o `SpecDetailDashboard` permanecem intocados — apenas a lista muda.

## Arquivos

```
apps/dashboard/src-tauri/src/spec_views.rs                     (expor `spec_children_tree` command)
apps/dashboard/src-tauri/src/lib.rs                            (registrar o command)
apps/dashboard/src/lib/dashboard.ts                            (fetchSpecChildrenTree wrapper)
apps/dashboard/src/lib/types/specs.ts                          (ChildrenTree, WaveChild, AcChild types)
apps/dashboard/src/components/specs/SpecRow.tsx                (novo — substitui SpecCard na lista)
apps/dashboard/src/components/specs/StageBullet.tsx            (novo — SVG ring 5 segmentos)
apps/dashboard/src/components/specs/SpecChildRow.tsx           (novo — wave/ac/sub-spec child row)
apps/dashboard/src/components/specs/SpecGroupHeader.tsx        (novo — header com count + collapse)
apps/dashboard/src/components/specs/SpecCard.tsx               (REMOVER — sem call-sites após Wave 3)
apps/dashboard/src/pages/Specs.tsx                             (substituir SpecCard por SpecRow + agrupamento)
apps/dashboard/src/i18n.ts                                     (chaves: route.specs.groups.{analyze,plan,execute,qa_review,close,cancelled})
apps/dashboard/src/components/page/PhaseChip.tsx               (atualizar para Stage; ou criar StageChip novo se mais limpo)
```

## Tarefas

### Backend Tauri
- [ ] Em `spec_views.rs`, adicionar `#[tauri::command] pub async fn spec_children_tree(spec: String, project_path: String) -> Result<ChildrenTree, String>` que executa `mustard-rt run spec-children-tree --spec NAME` e devolve o JSON parseado.
- [ ] Registrar em `lib.rs::invoke_handler`.

### Wrapper TS
- [ ] Em `lib/dashboard.ts`, adicionar `export async function fetchSpecChildrenTree(spec, projectPath): Promise<ChildrenTree>`.
- [ ] Em `lib/types/specs.ts`, exportar tipos `Stage`, `Outcome`, `Flags`, `SpecState`, `ChildrenTree`, `WaveChild`, `AcChild`, `SubSpecChild`. Espelhar o JSON do core 1:1.

### Componentes
- [ ] `StageBullet.tsx`: componente que aceita `stage: Stage` (e flags opcionais `blocked`, `wave_failed`). Renderiza SVG ring 16x16 com 5 arcos (Analyze 20°, Plan 40°, Execute 60°, QaReview 80°, Close 100% — proporcional). Preenchimento determinado pelo Stage atual: arcos anteriores 100% opacos, arco atual com pulso animado, posteriores em 20% opacity. Cor de cada arco vem de `--color-phase-N` em CSS variables (já existem no theme). Outcome terminal (Completed/Cancelled/Abandoned) pinta o ring inteiro na cor do outcome (verde/amber/cinza) com ícone central (✓/⊘/⊗). Flag `blocked` adiciona pequeno badge de pausa no canto; `wave_failed` adiciona triângulo de alerta.
- [ ] `SpecRow.tsx`: linha com colunas — `[chevron|StageBullet] [name (mono)] [model] [waves] [AC] [duration] [→]`. Altura 32px, padding lateral 16px. Hover: `bg-muted/30` + cursor pointer. Click na linha (exceto chevron) abre `/spec/{name}`.
- [ ] `SpecChildRow.tsx`: linha indentada 32px. Mesmo formato visual mas bullet menor (12px) e fonte sm-1 (11px). Aceita `kind: 'wave' | 'ac' | 'sub-spec'` e renderiza colunas correspondentes. Click abre drill-down do parent.
- [ ] `SpecGroupHeader.tsx`: linha com `▾/▸ STAGE_LABEL COUNT`. Click toggles expansão do grupo. Grupos vazios mostram count=0 colapsado por default. Grupos com count>0 expandidos por default exceto `CLOSED`/`CANCELLED` (colapsados).
- [ ] `Specs.tsx`: substituir o `SpecCardComponent` no map por `SpecRow`. Adicionar agrupamento por `state.stage` (com `Outcome != Active` indo para grupos terminais `CLOSED`/`CANCELLED`/`ABANDONED`). Substituir o `SpecsTopBar` actual por pills mais discretos. Manter busca, mantém date filter, descarta tabs `Ativas/Follow-up/Encerradas/Cancelado/Abandonado/Todas` (agora cobertas por agrupamento + filtro principal `Ativas/Suspeitas/Encerradas`).

### Estado e queries
- [ ] React state local em `Specs.tsx`: `expandedSpecs: Set<string>` (which specs are expanded), `expandedGroups: Set<Stage>` (which group headers are expanded).
- [ ] `useQuery` por spec expandida: `useQuery({ queryKey: ['spec-children-tree', spec, projectPath], queryFn: () => fetchSpecChildrenTree(spec, projectPath), enabled: expandedSpecs.has(spec), staleTime: 30_000 })`. Lazy — só dispara quando expande.
- [ ] Garantir que ao colapsar e re-expandir não refetch (cached).

### Limpeza
- [ ] Após confirmar zero call-sites de `SpecCard`, remover `SpecCard.tsx`. Glob por `from "@/components/specs/SpecCard"` deve retornar vazio.
- [ ] `PhaseChip.tsx`: avaliar — provavelmente vira `StageChip` com props `{ stage, outcome, flags }`. Se ainda é usado em SpecDetail (drill-down), preservar comportamento.

### i18n
- [ ] Chaves novas em `i18n.ts`:
  - `route.specs.groups.analyze` → "Analisando" / "Analyzing"
  - `route.specs.groups.plan` → "Planejando" / "Planning"
  - `route.specs.groups.execute` → "Executando" / "Executing"
  - `route.specs.groups.qa_review` → "Validando" / "Reviewing"
  - `route.specs.groups.close` → "Fechadas" / "Closed"
  - `route.specs.groups.cancelled` → "Canceladas" / "Cancelled"
  - `route.specs.groups.abandoned` → "Abandonadas" / "Abandoned"
  - `route.specs.child.wave` / `route.specs.child.ac` / `route.specs.child.sub_spec`
  - `route.specs.empty_group` → "0"
- [ ] Manter chaves antigas `route.specs.title` / `route.specs.subtitle` (usadas no Topbar via pathLabel).

## Layout final (referência visual)

```
┌────────────────────────────────────────────────────────────────────────────┐
│  [Ativas] [Suspeitas] [Encerradas]    Hoje  7d  30d        [+ Nova]        │
│  🔍 Buscar…                                                                 │
├────────────────────────────────────────────────────────────────────────────┤
│  ▾  ANALYZE                                                          1     │
│  ▸ ◔  2026-05-21-mustard-v1-installer-and-update   opus  w1/5  0/0  7m32s │
│  ▾  EXECUTE                                                          2     │
│  ▾ ◕  2026-05-21-flatten-spec-layout-and-multi-collab opus  w2/5  3/5  1h │
│     ✓  wave-1-spec-hygiene                wave        passed         2m   │
│     ◑  wave-2-rt-events                   wave        running        8m   │
│     ✓  AC-W4-1  grep returns empty        ac          passed         —    │
│     ✗  AC-W4-2  build is green            ac          failed         —    │
│     ◕  2026-05-21-tf-skill-mirror         sub-spec    qa-review      5m   │
│  ▸ ◑  2026-05-21-wave-integrity-doctor              —    —     0/0   —   │
│  ▸  QA/REVIEW                                                        0     │
│  ▸  CLOSED                                                           12    │
│  ▸  CANCELLED                                                        3     │
└────────────────────────────────────────────────────────────────────────────┘
```

## Acceptance Criteria

- [ ] AC-W3-1: `pnpm --filter mustard-dashboard build` passa (`tsc -b && vite build`).
- [ ] AC-W3-2: `pnpm --filter mustard-dashboard lint` passa sem warnings.
- [ ] AC-W3-3: `rg -n 'from "@/components/specs/SpecCard"' apps/dashboard/src` retorna vazio.
- [ ] AC-W3-4: Em `pnpm tauri:dev` com workspace selecionado (`C:\Atiz\mustard`), a rota `/specs` mostra grupos `ANALYZE`/`PLAN`/`EXECUTE`/`QA/REVIEW`/`CLOSED` com count corretos.
- [ ] AC-W3-5: Expandir o card de `2026-05-21-flatten-spec-layout-and-multi-collab` mostra ≥2 waves, ≥1 AC, ≥1 sub-spec, todos em sub-níveis indentados.
- [ ] AC-W3-6: Hover em uma linha pinta `bg-muted/30`; click navega para `/spec/2026-05-21-flatten-...`.
- [ ] AC-W3-7: Density check: na resolução 1440x900, viewport `/specs` mostra ≥10 specs simultaneamente em grupos expandidos.
- [ ] AC-W3-8: Filtro `Suspeitas` (placeholder até Wave 5/6 popular o critério) responde aos clicks sem erro de runtime — mesmo que esteja vazio.

## Limites

**IN:** apenas os arquivos listados.

**OUT:**
- `SpecDetail.tsx`, `SpecDetailDashboard.tsx`, `SpecDrillDown.tsx`, `SpecNetworkTab.tsx`, `SpecQualityTab.tsx`, `SpecWavesTab.tsx` — drill-down permanece intocado.
- Card "Saúde" do Workspace — Wave 6.
- Hook backend hygiene — Wave 5.
- Header das specs em disco — Wave 7.

## Notas de craft

- Fonte do spec name: `JetBrains Mono` (já no theme). Body: Inter (memory aesthetic).
- Bullet animation: opcional motion library; se quiser CSS-only, transição em `stroke-dashoffset` no SVG ring funciona bem.
- Cor da chevron `▾/▸` em `muted-foreground/50`; ao hover na linha pinta `muted-foreground`.
- Tipo do child (`wave` / `ac` / `sub-spec`) em `text-muted-foreground/60` com width fixo 80px para alinhamento.

## Concerns (registradas na EXECUTE da W3)

- **SpecCard.tsx não removido (limpeza parcial).** AC-W3-3 (`rg 'from "@/components/specs/SpecCard"'` vazio) passa — nenhum call-site absoluto restou. Mas `SpecDetailDashboard.tsx` (explicitamente OUT desta wave) ainda importa `./SpecCard` por path relativo, então deletar o arquivo quebra o build. Componente restaurado. **Follow-up:** migrar o header do `SpecDetailDashboard` para fora do `SpecCard` (provável Wave 6 ou tactical-fix) e só então remover.
- **AC-W3-2 (lint) não verificável.** `pnpm --filter mustard-dashboard lint` está quebrado em todo o repo: ESLint v9 exige `eslint.config.js` (flat config) e nenhum existe. Gap de ambiente pré-existente, não regressão desta wave. Recomenda-se sub-spec separada para adicionar o flat config antes de exigir AC-W3-2.
- **i18n em `src/lib/i18n.ts`** (não `src/i18n.ts`): as chaves `route.specs.*` canônicas vivem no dict plano do `useT()`. Chaves novas adicionadas lá.
