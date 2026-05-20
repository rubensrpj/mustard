# Telemetria dashboard — redesign completo (timeline-led + heatmap + bug fix de spec attribution)

### Status: draft
### Phase: PLAN
### Scope: full
### Checkpoint: 2026-05-19T23:55:00Z
### Lang: pt

> **Bloqueada por `2026-05-19-pipeline-state-from-sqlite`.** Não aprovar/executar antes daquela spec fechar (Wave 5 do ingest + delete dos JSONs precisa landar) — esta spec lê todas as agregações de telemetria de eventos consolidados no SQLite. Sem aquele consolidamento, o redesign trabalha em cima de source-of-truth fragmentada.

## PRD

## Contexto

A página `apps/dashboard/src/pages/Telemetry.tsx` cobre 2 abas (Atividade + Economia) e expõe 7 surfaces (5 cards de fase, Histórico, Critérios, Onde o esforço acontece, Agentes despachados, Ferramentas, KPIs de economia em RTK/Hooks/Roteamento/Cache). Hoje, com a migração de eventos para SQLite quase completa, a página tem três problemas distintos:

1. **Dados subaproveitados.** Vários cards mostram `0` mesmo com eventos fluindo (`ATIVIDADE POR FASE` reporta 0 em todas as 5 fases enquanto `AGENTES DESPACHADOS` reporta 72). Causa provável dupla: (a) query filtra por "hoje" UTC e a sessão atravessa meia-noite UTC; (b) bug colateral — eventos da sessão atual não atribuem `spec` corretamente, então surfaces que filtram por spec não os pegam (na aba Eventos, o filtro SPEC só mostra `lsp-doctor-and-deny-paths`, não as 2 specs ativas/recém-fechadas).
2. **Estrutura templated.** Hallmark audit identificou: 4 critical + 9 major findings — eyebrow-itis (29 uppercase mono-cap labels na página), grids 3-coluna idênticas repetidas, default-attractor sameness entre as 2 abas (mesmo fingerprint estrutural só trocando conteúdo), card-in-card, hierarquia plana (CANARY TAIL pesa o mesmo que USD measured).
3. **Falta protagonista.** Para um dashboard de telemetria de **pipeline-driven tool**, a pipeline é o protagonista óbvio — hoje ela é tratada como mais uma linha numa tabela. As novas visualizações (timeline cronológica, heatmap de esforço, KPIs com sparkline) que faltam são justamente o que dá personalidade visual ao Mustard.

Esta spec resolve os três num único pacote: bug fix + estrutura nova + visualizações novas, preservando identidade visual existente (Linear+Notion dark-first, **brand: mustard yellow**, Inter, blocos Notion-like + listas Linear-like — memory `design-aesthetic`).

## Usuários/Stakeholders

Mantenedores do Mustard, que precisam visibilidade real do que está rodando (não 0s). Indiretamente, qualquer usuário do `mustard-dashboard` que abre Telemetria pra entender o que acontece nos pipelines do projeto dele. Solicitado por Rubens em 2026-05-19 após review da telemetria atual contra 2 sessões reais.

## Métrica de sucesso

- Página Atividade mostra a pipeline ativa (ou a última fechada) como **protagonista visual** (timeline horizontal hero), não como linha numa lista.
- Eventos de qualquer sessão aparecem atribuídos à spec correta (filtro SPEC exibe `2026-05-19-pipeline-state-from-sqlite`, `2026-05-19-telemetry-dashboard-redesign`, etc., não só `lsp-doctor-and-deny-paths`).
- Time-range selector (Hoje / 7d / 30d / All) muda todas as agregações atomicamente.
- Hallmark audit roda 0 critical findings pós-redesign (mesmo audit que rodou pré-redesign — diff aparece em decisões).
- Frontend não introduz nova identidade visual: mesmo dark + mustard yellow accent + Inter; apenas reorganiza hierarquia.

## Não-Objetivos

- **Não introduzir nova identidade visual.** Linear+Notion dark + mustard yellow accent fica. Sem 3D, sem aurora-blob, sem glassmorphism, sem novos tons de paleta.
- **Não trocar tab pills por outro componente.** Tabs Atividade | Economia continuam; apenas conteúdo dentro de cada tab muda.
- **Não tocar o stream OTEL collector** (`apps/dashboard/src-tauri/src/...` para coleta OTEL). Esta spec lê dados; coleta é outra fronteira.
- **Não substituir TanStack Query.** Hooks continuam usando o padrão atual de fan-out (memory `dashboard-use-queries-fanout`).
- **Não adicionar busca vetorial / IA pra "sumarizar atividade"** — overkill, fora de escopo.
- **Não migrar shape do `PipelineSummary`** (a spec-mãe `pipeline-state-from-sqlite` já preservou). Novos surfaces criam novos shapes (`PhaseEvent`, `HeatmapCell`, `AcceptanceCriterion`, etc.); shapes existentes continuam.
- **Não tocar a aba Eventos** (timeline + stream de eventos) — ela funciona bem; apenas adicionar Hallmark fixes pequenos (eyebrows, tabular-nums) durante Wave 6 polish.

## Critérios de Aceitação

Critérios binários, executáveis. Cada um roda da raiz do projeto; exit 0 = passou. Padrão `node -e "...includes()"` (cross-shell-safe per memory `feedback_ac_cross_shell_windows.md`).

- [ ] AC-1: Dashboard build limpo — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-2: Workspace Rust compila — Command: `cargo build -p mustard-core -p mustard-rt`
- [ ] AC-3: Testes rt e dashboard backend passam — Command: `cargo test -p mustard-rt -p mustard-dashboard`
- [ ] AC-4: Componente `PipelineTimeline` existe — Command: `node -e "if(!require('fs').existsSync('apps/dashboard/src/components/telemetry/PipelineTimeline.tsx'))process.exit(1)"`
- [ ] AC-5: Componente `EffortHeatmap` existe — Command: `node -e "if(!require('fs').existsSync('apps/dashboard/src/components/telemetry/EffortHeatmap.tsx'))process.exit(1)"`
- [ ] AC-6: Componente `TimeRangeSelector` existe — Command: `node -e "if(!require('fs').existsSync('apps/dashboard/src/components/telemetry/TimeRangeSelector.tsx'))process.exit(1)"`
- [ ] AC-7: 7 Tauri commands de telemetria registrados em lib.rs — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src-tauri/src/lib.rs','utf8');for(const t of ['dashboard_telemetry_phases','dashboard_telemetry_timeline','dashboard_telemetry_heatmap','dashboard_telemetry_history','dashboard_telemetry_criteria','dashboard_telemetry_effort','dashboard_telemetry_agents']){if(!c.includes(t))process.exit(1)}"`
- [ ] AC-8: Telemetry.tsx referencia os novos componentes — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/pages/Telemetry.tsx','utf8');for(const x of ['PipelineTimeline','EffortHeatmap','TimeRangeSelector']){if(!c.includes(x))process.exit(1)}"`
- [ ] AC-9: Bug fix — emit_phase.rs constrói HarnessEvent com campo spec — Command: `node -e "const c=require('fs').readFileSync('apps/rt/src/run/emit_phase.rs','utf8');if(c.indexOf('spec:')<0)process.exit(1)"`
- [ ] AC-10: Zero eyebrows uppercase em Telemetry.tsx (sem strings com 4+ letras maiúsculas consecutivas em labels JSX, ignorando acrônimos comuns) — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/pages/Telemetry.tsx','utf8');const m=c.match(/>\s*[A-Z]{4,}[A-Z\s]*\s*</g);if(m&&m.filter(s=>!/CSS|SQL|JSON|API|URL|HTTP|UUID|UTC|RTK|QA|EXECUTE|ANALYZE|PLAN|CLOSE/.test(s)).length>0)process.exit(1)"`
- [ ] AC-11: tabular-nums presente em pelo menos um componente de telemetria — Command: `node -e "const fs=require('fs'),p=require('path');function walk(d){if(!fs.existsSync(d))return [];let r=[];for(const f of fs.readdirSync(d,{withFileTypes:true})){const x=p.join(d,f.name);if(f.isDirectory())r=r.concat(walk(x));else if(f.name.endsWith('.tsx'))r.push(x)}return r}const files=walk('apps/dashboard/src/components/telemetry').concat(['apps/dashboard/src/pages/Telemetry.tsx']);const c=files.filter(f=>fs.existsSync(f)).map(f=>fs.readFileSync(f,'utf8')).join('\n');if(!c.includes('tabular-nums'))process.exit(1)"`
- [ ] AC-12: PipelineTimeline e EffortHeatmap importados no Telemetry.tsx (não só mencionados em strings) — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/pages/Telemetry.tsx','utf8');if(!/import[^;]*PipelineTimeline/.test(c)||!/import[^;]*EffortHeatmap/.test(c))process.exit(1)"`

## Plano

## Informações da Entidade

Não cria entidade de domínio nova. Usa as projeções e eventos já consolidados pela spec-mãe `2026-05-19-pipeline-state-from-sqlite`:

- `pipeline_state_for_spec(spec)` → reconstrói estado completo de uma spec (status, scope, tasks, currentWave, completedWaves, ...).
- `pipelines_from_db()` → lista todas as pipelines com `PipelineSummary` shape.
- Eventos canônicos no `SqliteEventStore`: `pipeline.phase`, `pipeline.status`, `pipeline.task.*`, `pipeline.wave.complete`, `qa.result`, `review.result`, `agent.start`, `agent.stop`, `tool.use`.

Novos view-shapes (puramente do dashboard, não persistidos):

| Shape | Campos | Origem |
|---|---|---|
| `PhaseSummary` | `{ phase, events_count, last_event_at, sparkline: number[] }` | `dashboard_telemetry_phases(time_range)` |
| `TimelineEvent` | `{ id, ts, phase, spec, agent?, summary }` | `dashboard_telemetry_timeline(time_range, limit)` |
| `HeatmapCell` | `{ day_of_week, hour, event_count }` (7×24 grid) | `dashboard_telemetry_heatmap(time_range)` |
| `HistoryEntry` | `{ spec, status, started_at, completed_at, duration_per_phase: {analyze, plan, execute, qa, close}, ac_passed, ac_total }` | `dashboard_telemetry_history(limit)` |
| `AcceptanceCriterion` | `{ spec, id, status, last_run_at }` | `dashboard_telemetry_criteria(time_range)` |
| `EffortBreakdown` | `{ top_files: [{path, count}], top_tools: [{name, count}], top_phases: [{phase, duration_ms}], top_agents: [{agent_type, count}] }` | `dashboard_telemetry_effort(time_range)` |
| `AgentDispatch` | `{ subagent_type, count, error_count, avg_duration_ms, last_dispatched_at }` | `dashboard_telemetry_agents(time_range)` — agrupado por `subagent_type` REAL (não pelo nome do hook que emitiu) |

## Briefing de design (Hallmark audit — referência inline)

Esta spec foi precedida por um Hallmark audit da página atual. Os achados ditam decisões de design abaixo. **Agentes da Wave 4/5 devem ler esta seção antes de tocar `Telemetry.tsx`.**

**Findings críticos (eliminar):**

1. **Eyebrow on every section.** Página atual tem 29 uppercase mono-cap labels (5 section eyebrows + ~18 sub-eyebrows dentro de cards + per-aba). Redesign: zero eyebrows uppercase. Headings normais H2 com peso visual diferente das labels.
2. **3-column feature grid repetido.** Página atual repete "3 cards de largura igual" 3 vezes. Redesign: assimetria deliberada. Quando 3 cards são necessários, variar larguras (e.g. 2fr 1fr 1fr) ou usar hierarquia tipográfica (1 hero + 2 secundários).
3. **Default-attractor sameness entre as 2 abas.** Atividade e Economia hoje têm fingerprint idêntico. Redesign: **macroestruturas diferentes por aba** — Atividade = timeline-led (pipeline ativa como protagonista horizontal); Economia = stat-led asimétrico (1 hero number + breakdown).
4. **Card-in-card.** Card "Roteamento de modelo" tem 3 níveis de nesting visual. Redesign: 1 nível de containment, separar com whitespace.

**Findings major (corrigir):**

5. **Hierarquia plana.** ECONOMIA DE TOKENS, PROMPT ECONOMY, CANARY TAIL têm peso visual idêntico hoje. Redesign: 1 hero (= protagonista) + 1-2 secundários + 1 diagnostic colapsável (CANARY TAIL collapsed default).
6. **Tabular-nums ausente.** Todos os números em colunas devem usar `font-variant-numeric: tabular-nums`.
7. **Numbers centered + labels CAPS.** Redesign: numbers como typographic primary (grandes, peso heavy, à esquerda); labels normais inline (sem CAPS, peso normal).
8. **Mixed accent semantics.** Hoje cores dots verde/amarelo/laranja/cinza/roxo sem sistema. Redesign: 3-color anchor — `--ok` (verde discreto), `--attention` (**mustard yellow — brand**), `--error` (vermelho discreto). Phase identity via **posição + glyph + brand-yellow só na fase ativa** (NÃO 5 cores diferentes por fase).
9. **Invented metrics no card Roteamento.** Texto "exemplo: 487 dispatches..." competindo com números reais. Redesign: mover explicações pra tooltip do ícone `?` (info pattern).
10. **Empty states fracos.** Hoje texto flutuando no card. Redesign: skeleton com `—` no slot do número + texto descritor pequeno no rodapé (visual sinaliza "esperando dado").

**Findings minor (polish):**

11. **CANARY TAIL exposto no fim da página.** Mover pra collapsed-by-default ou pra Configurações > Diagnostics.
12. **Descriptor verbose dentro de cada card.** Mover pra tooltip no ícone `?` (info pattern). Default: dado fala por si.

**Identidade visual (preservada, não mudar):**

- Dark-first (mesmo background base já em uso)
- **Accent único: mustard yellow** (`--color-accent-mustard`). Brand do projeto. Usado para: fase ativa, badges "novidade", call-out de attention. Não usar como decoração genérica.
- Inter (display + body)
- Blocos Notion-like (containers com border sutil + padding generoso) + listas Linear-like (rows densas, sem decoração)
- Sem 3D, aurora-blob, glassmorphism, animação on-scroll, bouncy easing

**Motion budget:** ≤3 primitives totais na página:
- `number-tick` (counter change suave, 200ms ease-out)
- `wave-glow` (pulse 1× quando uma wave nova entra em estado active)
- `once-on-mount` fade (cards aparecem com 100ms fade na primeira carga; sem repetir em re-render)

Tudo mais é instant (focus rings, hover backgrounds, etc.). `prefers-reduced-motion: reduce` → tudo instant.

## Arquivos

- `apps/rt/src/run/emit_phase.rs` (edição) — bug fix: garantir que `spec` field é sempre populado quando emitindo `pipeline.phase`. Verificar/adicionar testes.
- `apps/rt/src/run/emit_event.rs` (edição) — mesma verificação pra `emit-event`.
- `apps/rt/src/run/emit_pipeline.rs` (edição se já existe pela spec-mãe; senão criada lá) — mesma verificação.
- `apps/rt/src/dispatch.rs` ou `apps/rt/src/hooks/*` (auditoria) — Grep por `HarnessEvent` construção sem `spec:` populado quando contexto tem spec. Fix surgical onde encontrar.
- `apps/dashboard/src-tauri/src/db.rs` (edição grande) — 7 funções de agregação novas: `telemetry_phases`, `telemetry_timeline`, `telemetry_heatmap`, `telemetry_history`, `telemetry_criteria`, `telemetry_effort`, `telemetry_agents`. SQL agrupado por time_range param. Index `idx_events_ts_spec` se ainda não existe.
- `apps/dashboard/src-tauri/src/lib.rs` (edição grande) — 7 Tauri commands `dashboard_telemetry_*` wrappando as funções de db.rs. Aceitam param `time_range: "today" | "7d" | "30d" | "all"`.
- `apps/dashboard/src/components/telemetry/PipelineTimeline.tsx` (novo) — protagonista da aba Atividade. Renderiza pipeline ativa como timeline horizontal (5 stations: ANALYZE → PLAN → EXECUTE → QA → CLOSE). Active wave glow (mustard yellow). Completed dim. Future outlined. Empty state: "Nenhuma pipeline em execução — última fechada: <nome>".
- `apps/dashboard/src/components/telemetry/PhaseStation.tsx` (novo) — sub-componente da timeline. 1 station = 1 fase. Glyph + label + duração + count de eventos. Estados: future / active / completed.
- `apps/dashboard/src/components/telemetry/EffortHeatmap.tsx` (novo) — heatmap 7×24 (dia da semana × hora). Color encoding via opacidade do brand-yellow (low = `--color-paper`, high = `--color-accent-mustard`). Hover cell → tooltip com hora + count.
- `apps/dashboard/src/components/telemetry/HistoryStrip.tsx` (novo) — strip horizontal com últimas N pipelines fechadas. Cada item: nome + scope chip + duração total + AC pass/total. Click → drill pra Atividade dessa pipeline.
- `apps/dashboard/src/components/telemetry/CriteriaPanel.tsx` (novo) — taxa de aprovação atual (número grande, tabular-nums) + sparkline 30d + lista das últimas 5 falhas ("AC-3 falhou em <spec> há 2h").
- `apps/dashboard/src/components/telemetry/EffortPanel.tsx` (novo) — 4 listas (top files / top tools / top phases / top agents) com bar widths proporcionais. Densidade Linear-like.
- `apps/dashboard/src/components/telemetry/AgentRoster.tsx` (novo) — top N `subagent_type` (general-purpose, Explore, Plan, etc. — não nome do hook!). Por linha: agent + dispatches + erros + avg duration.
- `apps/dashboard/src/components/telemetry/TimeRangeSelector.tsx` (novo) — segmented control: Hoje | 7d | 30d | All. Estado controlado, propaga via context ou prop drilling pros 7 hooks.
- `apps/dashboard/src/components/telemetry/EconomySection.tsx` (novo) — aba Economia rewriteada. Hero: número gigante "Tokens economizados" + sparkline 30d. Abaixo: 3 cards menores (RTK, Hooks, Roteamento) com larguras assimétricas. Abaixo: Prompt Economy section (3 blocos atuais mas com hierarquia visual). CANARY TAIL collapsed.
- `apps/dashboard/src/hooks/useTelemetryPhases.ts`, `useTelemetryTimeline.ts`, `useTelemetryHeatmap.ts`, `useTelemetryHistory.ts`, `useTelemetryCriteria.ts`, `useTelemetryEffort.ts`, `useTelemetryAgents.ts` (novos, 7 hooks) — TanStack Query wrappers tipados. Cada um aceita `timeRange` param. `refetchOnWindowFocus: true`, `refetchInterval: 5000` (5s polling). Memory `dashboard-use-queries-fanout`.
- `apps/dashboard/src/lib/dashboard.ts` (edição) — typed wrappers `invoke<PhaseSummary[]>('dashboard_telemetry_phases', {timeRange})` etc.
- `apps/dashboard/src/pages/Telemetry.tsx` (rewrite grande) — composição: tab selector + TimeRangeSelector + (Atividade: PipelineTimeline hero + AgentRoster lateral + EffortHeatmap + HistoryStrip + CriteriaPanel + EffortPanel + CanaryTail collapsed) ou (Economia: EconomySection).
- `apps/dashboard/src-tauri/tests/telemetry_*.rs` (novos, 2-3 testes) — testa 1-2 das aggregations contra DB seed.
- `apps/dashboard/src/components/telemetry/__tests__/` (opcional se houver setup de Vitest) — smoke tests dos componentes.

## Tarefas

### Wave 1 — Bug fix: spec attribution em eventos

- [ ] Grep `apps/rt/src/` por construção de `HarnessEvent` (busca por `HarnessEvent {` ou `HarnessEvent::new(`). Identificar todos os call sites onde `spec` field é construído.
- [ ] Pra cada site: confirmar que `spec: Some(<spec_name>)` é populado quando o contexto tem uma spec ativa. Os contextos onde isso falha hoje: emissores que rodam fora de uma pipeline ativa OU emissores que rodam dentro mas perderam o ponteiro pro spec name.
- [ ] Identificar a fonte do spec name: provavelmente `current_spec()` helper em `apps/rt/src/run/env.rs` ou semelhante (Grep pra confirmar). Se não existir, criar — lê do active pipeline-state filename OU da env var `MUSTARD_ACTIVE_SPEC` se setada por `/feature`/`/resume`.
- [ ] Corrigir os call sites identificados pra usar o helper.
- [ ] Teste: emite 1 evento via `emit-phase --to ANALYZE --spec test-spec`; query no SqliteEventStore retorna o evento com `spec == Some("test-spec")`. Teste de fail-open: sem spec ativa, helper retorna `None` (não panica).
- [ ] Validate: `cargo test -p mustard-rt`.

### Wave 2 — Backend aggregation Tauri commands — depende de Wave 1

- [ ] Em `apps/dashboard/src-tauri/src/db.rs`: implementar 7 funções de agregação. SQL agrupado por `time_range` (`'today'` → `date(ts) >= date('now')`; `'7d'` → `ts >= datetime('now', '-7 days')`; `'30d'` → `ts >= datetime('now', '-30 days')`; `'all'` → sem filtro). Cada função retorna o shape correspondente da tabela em § Informações da Entidade.
- [ ] Adicionar index `CREATE INDEX IF NOT EXISTS idx_events_ts_spec ON events(ts, spec)` na schema migration (verificar se já existe).
- [ ] Em `apps/dashboard/src-tauri/src/lib.rs`: 7 Tauri commands `dashboard_telemetry_*`. Cada um aceita `time_range: String` param e retorna `Vec<T>` do shape correspondente. Fail-open: retorna `Ok(vec![])` em DB error (não quebra UI).
- [ ] Pra `telemetry_agents`: agrupar por `agent.start.payload.subagent_type` (REAL — `general-purpose`, `Explore`, `Plan`, etc.) e não pelo nome do hook emissor. Esse é o bug atual ("AGENTES DESPACHADOS mostra só `subagent-tracker`" porque agrega pelo nome do hook).
- [ ] Testes: 2-3 testes em `apps/dashboard/src-tauri/tests/telemetry_aggregations_test.rs`. Seed eventos, valida shape.
- [ ] Validate: `cargo build -p mustard-dashboard && cargo test -p mustard-dashboard`.

### Wave 3 — Frontend primitives — depende de Wave 2

- [ ] `apps/dashboard/src/lib/dashboard.ts`: adicionar typed wrappers para os 7 commands novos.
- [ ] 7 hooks em `apps/dashboard/src/hooks/useTelemetry*.ts`. TanStack Query, `refetchOnWindowFocus: true`, `refetchInterval: 5000`. Param `timeRange` passa pro queryKey (cada timeRange = cache separado).
- [ ] `TimeRangeSelector.tsx`: segmented control com 4 opções (Hoje | 7d | 30d | All). Estado via Context (`TelemetryTimeRangeContext`) pra evitar prop drilling pelos 7 componentes.
- [ ] `PipelineTimeline.tsx`: SVG horizontal com 5 stations (ANALYZE → PLAN → EXECUTE → QA → CLOSE). Conectores entre stations (linha sutil). Cada station = `PhaseStation`. Active wave: `--color-accent-mustard` fill + pulse animation (`wave-glow`, 1× quando vira active). Completed: ink color, sem decoração. Future: outline-only. Empty state quando nenhuma pipeline ativa: exibe a última fechada (dim, com label "última fechada: <nome>"). Sem timeline = empty state com instrução curta.
- [ ] `PhaseStation.tsx`: glyph (1 ícone por fase, lucide ou phosphor — escolher 1 lib só) + label da fase + duração da fase (tabular-nums) + count de eventos. Estados future/active/completed via prop. No CAPS labels.
- [ ] `EffortHeatmap.tsx`: SVG grid 7 (dias) × 24 (horas). Cada cell color encoded por opacidade do brand-yellow (0 = `--color-paper`, max = `--color-accent-mustard` full). Hover cell → tooltip com "Domingo 14h • 42 eventos". Empty state visual: grid cinza claro com tooltip explicativo.
- [ ] `HistoryStrip.tsx`: lista horizontal de últimas N pipelines. Densa, Linear-like rows. Cada item: nome + scope chip (full/light/touch) + duração total (`hh:mm`, tabular-nums) + AC `<passed>/<total>`. Click → router push pra detalhe (futuro; agora só hover state).
- [ ] `CriteriaPanel.tsx`: layout 2-col asimétrico. Esquerda: número grande "Taxa de aprovação" (tabular-nums) + sparkline 30d (SVG inline, sem lib externa). Direita: lista 5 últimas falhas formato "AC-3 • spec-nome • há 2h".
- [ ] `EffortPanel.tsx`: 4 sub-listas dispostas em grid 2×2. Top files, Top tools, Top phases, Top agents. Bar widths proporcionais. Densidade Linear-like.
- [ ] `AgentRoster.tsx`: top N rows. Cada row: agent_type + dispatches + erros (red dot se >0) + avg duration. Sem badge "subagent-tracker" porque o REAL agent_type é exposto.
- [ ] `EconomySection.tsx`: composição da aba Economia. Hero: "<X> tokens economizados" em peso massivo, tabular-nums, + sparkline 30d abaixo. Depois 3 cards (RTK, Hooks, Roteamento) com larguras assimétricas (e.g. 2fr 1fr 1fr — RTK protagonista por ser o de maior savings). Depois Prompt Economy section (3 blocos atuais — Cache, Contexto, Eventos — mas com hierarquia: Cache é hero porque tem USD measured; Contexto e Eventos secundários menores). CANARY TAIL no fim, dentro de `<details>` collapsed default.
- [ ] **Sem eyebrows uppercase.** Headings normais (H2 com text-weight e tracking diferentes das labels). Labels inline com numbers, peso normal, sem CAPS.
- [ ] **Tabular-nums** em todo container numérico: usar utility class Tailwind `tabular-nums` ou CSS `font-variant-numeric: tabular-nums`.
- [ ] **Motion budget ≤3 primitives:** definir keyframes once em `apps/dashboard/src/styles/telemetry.css` (ou inline Tailwind) — `@keyframes number-tick`, `@keyframes wave-glow`, `@keyframes once-on-mount-fade`. Respeitar `prefers-reduced-motion: reduce`.
- [ ] Validate: `pnpm --filter mustard-dashboard build`.

### Wave 4 — Page rewrite: Atividade tab — depende de Wave 3

- [ ] `apps/dashboard/src/pages/Telemetry.tsx`: refator. Mantém top bar (breadcrumb + "live" indicator + "Coletor parado" badge + tab pills Atividade | Economia + TimeRangeSelector — adicionar este).
- [ ] Aba Atividade composição:
  - **Hero:** `PipelineTimeline` (full-width)
  - **Right of hero:** `AgentRoster` (compacto, sidebar)
  - **Below hero:** `EffortHeatmap` (full-width, mid-density)
  - **Below heatmap:** grid 2×1 — `HistoryStrip` (esquerda, mais largo) + `CriteriaPanel` (direita, mais estreito)
  - **Below grid:** `EffortPanel` (full-width)
  - **Footer:** `<details>` collapsed com CANARY TAIL (se coletor parado)
- [ ] Empty states coerentes: cada componente recebe sua data via hook e renderiza skeleton enquanto `isLoading`, e empty state textual quando `data?.length === 0`.
- [ ] **Sem eyebrows.** Headings em H2 normais (não CAPS).
- [ ] Validate: `pnpm --filter mustard-dashboard build`. Visual: cargo run desktop, navegar até /telemetria, validar layout.

### Wave 5 — Page rewrite: Economia tab — depende de Wave 4 (mesmo arquivo Telemetry.tsx)

- [ ] No mesmo `Telemetry.tsx`, branch da aba Economia renderiza `<EconomySection />` (já implementado em Wave 3).
- [ ] Migrar dados existentes (RTK total, Hooks breakdown, Roteamento intervenção, Cache USD, Contexto enviado/evitado, Eventos sessions) pros novos slots dentro de `EconomySection`. Esses dados já vêm dos endpoints existentes (não precisa de Wave 2 backend); apenas reorganizar o consumo.
- [ ] Hero: somatório de "tokens economizados acumulado" = RTK savings + Hooks-evitados + Roteamento-blocked (se makes sense; senão pegar só RTK como hero, mais simples).
- [ ] CANARY TAIL no `<details>` collapsed.
- [ ] Validate: `pnpm --filter mustard-dashboard build` + visual.

### Wave 6 — Polish + accessibility + audit cleanup — depende de Waves 1-5

- [ ] Audit grep zero eyebrows uppercase ≥4 chars (AC-10) — fixar qualquer remanescente.
- [ ] Audit grep tabular-nums em todos os containers numéricos (AC-11) — adicionar onde faltar.
- [ ] Audit visual: rodar `hallmark audit apps/dashboard/src/pages/Telemetry.tsx` (manualmente, não é AC). Esperado: 0 critical, ≤3 minor. Documentar nas Decisões.
- [ ] Acessibilidade:
  - `:focus-visible` ring (`--color-accent-mustard`, 3:1 contrast mínimo) em todos interativos (tabs, time range buttons, drill links).
  - Heatmap cells: `role="img"` + `aria-label` por cell.
  - Timeline stations: navegação via Tab (keyboard).
  - Texto em descriptors com `text-muted` mas contraste ≥4.5:1 contra background.
- [ ] `prefers-reduced-motion: reduce` → cortar `wave-glow` e `once-on-mount-fade`; `number-tick` vira instant.
- [ ] Validate final: `cargo build && cargo test && pnpm --filter mustard-dashboard build`.

## Dependências

- **Bloqueada por:** `2026-05-19-pipeline-state-from-sqlite` — esta spec lê de eventos consolidados no SQLite (`pipeline_state_for_spec`, `pipelines_from_db`). Sem Wave 5 daquela spec (ingest + delete), há JSONs órfãos coabitando com eventos e queries ficam não-determinísticas.
- **Spec ascendente:** `2026-05-19-dashboard-phase-from-sqlite` (CLOSE 2026-05-20) — estabeleceu o padrão de query SQLite + dashboard reader migration. Esta finaliza o trabalho na superfície de telemetria.
- **Não bloqueia:** `2026-05-19-artifact-update-followups` — eles tocam superfícies diferentes (artefatos defasados); podem rodar em paralelo se conveniente.

## Limites

- `apps/rt/src/run/emit_*.rs` + `apps/rt/src/dispatch.rs`/`apps/rt/src/hooks/*` (auditoria surgical pra bug fix Wave 1)
- `apps/dashboard/src-tauri/src/db.rs` (7 funções novas + 1 index)
- `apps/dashboard/src-tauri/src/lib.rs` (7 Tauri commands novos)
- `apps/dashboard/src/components/telemetry/` (8 componentes novos + 1 helper context)
- `apps/dashboard/src/hooks/useTelemetry*.ts` (7 hooks novos)
- `apps/dashboard/src/lib/dashboard.ts` (typed wrappers — 7 funções novas)
- `apps/dashboard/src/pages/Telemetry.tsx` (rewrite — manter top bar + tab pills, trocar conteúdo das 2 abas)
- `apps/dashboard/src/styles/telemetry.css` ou inline Tailwind (keyframes pra 3 motion primitives)
- `apps/dashboard/src-tauri/tests/telemetry_*.rs` (2-3 testes novos)
- **Fora dos limites:**
  - aba Eventos (lista cronológica + filtros) — funciona; apenas micro-polish em Wave 6 (eyebrows, tabular-nums)
  - aba Timeline (sub-tab da Atividade > Eventos) — fora
  - identidade visual (cores, fonts, paleta base) — Linear+Notion dark + mustard yellow fica
  - OTEL collector (lado da coleta) — esta spec lê dados
  - busca vetorial / IA pra resumir telemetria — fora
  - `PipelineSummary` shape — preservado pela spec-mãe
  - sync layer (ElectricSQL/PowerSync) — futura
