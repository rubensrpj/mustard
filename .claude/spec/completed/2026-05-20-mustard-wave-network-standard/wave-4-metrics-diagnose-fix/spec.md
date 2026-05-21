# Wave 4 — Métricas: diagnose do que está quebrado + fix com agrupamento por parent

### Parent: [[2026-05-20-mustard-wave-network-standard]]
### Status: completed
### Phase: CLOSE
### Scope: full (wave)
### Checkpoint: 2026-05-20T20:40:00Z
### Lang: pt

## PRD

## Contexto

O operador reporta que "toda a área de métrica não está funcionando" — KPIs do dashboard zerados ou somando cego entre specs irrelacionadas (RTK savings, token.saved, cross-wave memory size). Isso impede confiar em qualquer número e fere o princípio recém-criado: cada wave/spec é uma unidade de trabalho rastreável.

Esta wave é dividida em duas tarefas sequenciais dentro do mesmo agente:

1. **Diagnose**: rodar query exploratória no `events` SQLite, comparar com o que o dashboard renderiza, listar TODAS as fontes de divergência (event não emitido, agregação errada, parse quebrado, query sem `WHERE spec=`, etc.).
2. **Fix**: corrigir cada item da lista. Sempre que houver agregação cross-spec, adicionar grupamento por `parent` (lendo o `Parent:` wikilink da spec). Hookar o novo `metrics wave-status` (de [[wave-1-rt-infra]]) nas páginas que hoje usam queries soltas.

## Métrica de sucesso

- Diagnose entregue como `metrics-audit.md` no spec dir desta wave (lista verificável de N items + classificação: emitter/aggregator/renderer/parser).
- Cada item da lista marcado [x] no audit ao fim da wave.
- Páginas Economia e Quality do dashboard renderizam tree parent→waves com números diferentes de zero quando há eventos reais.
- Cross-wave memory bytes injetados aparecem como métrica por wave.

## Não-Objetivos

- Não migrar schema do `mustard.db` (só queries novas/corrigidas).
- Não criar dashboard de "system health" — só corrigir as KPIs existentes.
- Não tocar `token.saved` emitter (assumido correto — verificação só leitura).
- Não substituir RTK ou criar tracking alternativo — só consertar a leitura/exibição do que o RTK já emite.

## Acceptance Criteria

- [ ] AC-1: Cargo check passa — Command: `cargo check -p mustard-rt`
- [ ] AC-2: Build dashboard passa — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-3: `metrics-audit.md` existe no spec dir desta wave — Command: `bash -c 'test -f .claude/spec/*/2026-05-20-mustard-wave-network-standard/wave-4-metrics-diagnose-fix/metrics-audit.md'`
- [ ] AC-4: Página `Economia.tsx` consome `dashboard_metrics_wave_status` — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/pages/Economia.tsx','utf8');if(!t.includes('dashboardMetricsWaveStatus')&&!t.includes('metrics_wave_status'))throw new Error('Economia not wired to wave-status')"`
- [ ] AC-5: SpecsList e SpecsCard agrupam por parent — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/components/SpecsList.tsx','utf8');if(!/parent|Parent/.test(t))throw new Error('SpecsList missing parent grouping')"`

## Plano

## Arquivos (~6)

```
.claude/spec/active/2026-05-20-mustard-wave-network-standard/wave-4-metrics-diagnose-fix/metrics-audit.md
  (new — produzido pela tarefa de diagnose)

apps/dashboard/src-tauri/src/spec_views.rs            (modify — bridge metrics_wave_status)
apps/dashboard/src-tauri/src/main.rs                  (modify — register handler)
apps/dashboard/src/lib/dashboard.ts                   (modify — wrapper + interfaces)
apps/dashboard/src/pages/Economia.tsx                 (modify — consome metrics-wave-status, render tree por parent)
apps/dashboard/src/components/SpecsList.tsx           (modify — group by parent)
apps/dashboard/src/components/SpecCard.tsx            (modify — badge `+N waves` quando parent)
```

## Tarefas

### General Agent

#### Tarefa 1 — Diagnose (entrega `metrics-audit.md`)

- [ ] Listar todas as queries de métrica em `apps/dashboard/src-tauri/src/spec_views.rs` e arquivos correlatos (Grep `SUM\|COUNT\|GROUP BY` em `src-tauri/src/`)
- [ ] Listar todas as páginas/componentes do dashboard que exibem KPIs (Grep `useQuery.*metrics\|useQuery.*token\|useQuery.*economy` em `apps/dashboard/src/`)
- [ ] Para cada par query↔renderer, executar query manualmente contra `mustard.db` (via `mustard-rt run db-query --sql "..."`) e comparar com o número exibido no dashboard rodando local
- [ ] Classificar cada divergência em: (a) emitter — evento nunca é emitido; (b) aggregator — query soma errado/sem GROUP BY parent; (c) renderer — UI ignora o dado retornado; (d) parser — payload tem shape esperado diferente
- [ ] Gravar `metrics-audit.md` no spec dir desta wave com a lista numerada + classificação + linha onde está o problema + fix proposto

#### Tarefa 2 — Fix (consome `metrics-audit.md`)

- [ ] Para cada item do audit, aplicar o fix. Itens com classificação `aggregator` SEM `GROUP BY` parent → adicionar
- [ ] Adicionar wrapper `dashboardMetricsWaveStatus(specName)` em `lib/dashboard.ts` consumindo o novo `metrics wave-status` da [[wave-1-rt-infra]]
- [ ] Refatorar `Economia.tsx` para renderizar tree parent→waves: cada parent é uma seção expandível com chips por wave (status badge + tokens_saved + duration + retries + cross-wave bytes)
- [ ] `SpecsList.tsx`: agrupar specs por parent (parent no topo, children indentadas)
- [ ] `SpecCard.tsx`: badge `+N waves` quando spec é parent (link pra aba Network do drill-down)
- [ ] `pnpm --filter mustard-dashboard build && cargo check -p dashboard --manifest-path apps/dashboard/src-tauri/Cargo.toml`
- [ ] Marcar cada item do `metrics-audit.md` como [x] feito

## Dependências

- [[wave-1-rt-infra]]: precisa do subcomando `metrics wave-status`.

## Network

- Parent: [[2026-05-20-mustard-wave-network-standard]]
- Depende de: [[wave-1-rt-infra]]
- Paralela a: [[wave-2-skill-template]], [[wave-3-dashboard-graph]]
- Recebe memória: [[wave-1-rt-infra]] (shape do JSON do `metrics wave-status`).
- Grava memória: `{metrics_fixed: [...], parent_grouping_added_in: [...], notes: '...'}`.

## Limites

Em escopo: `apps/dashboard/src-tauri/src/{spec_views,main}.rs`, `apps/dashboard/src/{lib/dashboard.ts,pages/Economia.tsx,components/SpecsList.tsx,components/SpecCard.tsx}`, novo `metrics-audit.md` no spec dir.

Fora de escopo: outras páginas do dashboard (Workspace, Knowledge, etc.), `token.saved` emitter, RTK binário, schema do `mustard.db`.
