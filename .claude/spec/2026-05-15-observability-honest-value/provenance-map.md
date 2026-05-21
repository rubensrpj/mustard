# Provenance Map — Observability Honest Value (Wave 0)

Auditoria read-only da proveniência de cada métrica/card exibido no Mustard Dashboard.
Este arquivo é a **entrada obrigatória das Waves 5 e 6**: cada linha rastreia um card até
sua fonte real (arquivo / Tauri command) e classifica se o valor exibido é confiável.

## Legenda das classificações

- **`REAL`** — Dado de sessão, fonte direta, valor confiável. Mostra o que está acontecendo agora.
- **`ACUMULADO`** — Total vitalício desde a instalação. Verdadeiro, mas precisa de delta de sessão para ser útil/honesto.
- **`INFERIDO`** — Número calculado/estimado/derivado sem dado de origem direto, ou proxy de outra métrica.
- **`STALE`** — A fonte existe mas parou de ser atualizada (ex.: mirror SQLite legado pós-Wave 4).
- **`AUSENTE`** — Card existe mas o emissor não produz o dado — sempre zero.

## Repositórios auditados

- `C:/Atiz/mustard-dashboard` — UI (React `src/pages/`, `src/lib/dashboard.ts`, `src/hooks/`) + Tauri/Rust (`src-tauri/src/telemetry.rs`, `db.rs`, `lib.rs`).
- `C:/Atiz/mustard` — emissores de evento (`templates/hooks/`, `templates/scripts/`); log de verdade `.claude/.harness/events.jsonl`.

### Constatação estrutural (vale para todo o mapa)

O `events.jsonl` real do projeto Mustard hoje contém apenas estes tipos:
`tool.use` (56), `finding` (10), `decision` (8), `qa.result` (4), `pipeline.phase` (2),
`close-gate.check` (1), `commit-gate.check` (1). **Não há nenhum** `agent.start`,
`agent.stop`, `retry.attempt`, nem `mustard.subtraction.applied`. Todo card que
depende desses eventos é estruturalmente AUSENTE até as Waves 2/3/6 emitirem-nos.

---

## Tabela de proveniência

| Página | Card | Métrica | Fonte (arquivo / Tauri command) | Transformação | Valor exibido | Classificação | Ação |
|---|---|---|---|---|---|---|---|
| **Home** (portfolio) | Counters | Specs ativas | `useAggregate` → `dashboard_specs` (FS `.claude/spec/active/`) | conta dirs | inteiro | REAL | OK — derivado do filesystem. |
| **Home** (portfolio) | Counters | Em EXECUTE | `useAggregate` → `dashboard_active_pipelines` (`.pipeline-states/*.json`) | filtra phase=EXECUTE | inteiro | REAL | OK. |
| **Home** (portfolio) | Counters | Completed 7d | `useAggregate` → `dashboard_specs` (FS `completed/`) | filtra completed_at < 7d | inteiro | REAL | OK. |
| **Home** (portfolio) | Counters | Eventos hoje | `useAggregate` → `dashboard_recent_events` / metrics | conta eventos do dia | inteiro | REAL | OK. |
| **Home** (portfolio) | Consumo & Economia | Tokens total / hoje | `dashboard_consumption_global` → `db.rs cost_summary` (SQLite `events`/`spans`) | SUM input+output | `formatTokens` | STALE | SQLite é mirror legado pós-Wave 4; hooks só escrevem em `events.jsonl`. Marcar fonte ou migrar para JSONL. |
| **Home** (portfolio) | Consumo & Economia | Custo USD total / hoje | `dashboard_consumption_global` → `db.rs cost_summary` | tokens × tabela de preço | `formatUsd` | INFERIDO | Custo estimado de tokens × preço — não é cobrança medida. Confronta com OTEL real (Prompt Economy). |
| **Home** (portfolio) | Consumo & Economia | RTK saved / efic. / commands | `dashboard_consumption_global.rtk` → `telemetry.rs run_rtk_gain` (binário `rtk gain`, sem `-p`) | parse JSON do binário | `formatTokens` / `%` | ACUMULADO | É **RTK global** (todos os projetos, vitalício). Rotular "global RTK" — não é deste projeto. |
| **Home** (portfolio) | Sparkline 14d | consumido vs RTK saved | `dashboard_consumption_global.daily_series` + `rtk.daily` | série diária | linhas SVG | STALE/ACUMULADO | `daily_series` STALE (SQLite); `rtk.daily` ACUMULADO global. |
| **Home** (portfolio) | Por modelo / Por projeto | tokens, custo, pct | `db.rs consumption_by_model` (SQLite `spans`) | GROUP BY model | tabela | STALE | Mirror SQLite parado pós-Wave 4. |
| **Home** (portfolio) | Atividade recente | event_type / project / summary | `useAggregate` → `dashboard_recent_events` (`events.jsonl`) | tail | linhas | REAL | OK. |
| **Home** (portfolio) | Pipelines ativas | spec/phase/started_at | `dashboard_active_pipelines` (`.pipeline-states/*.json`) | leitura direta | linhas | REAL | OK. |
| **Home** (workspace) | Pipelines ativos | LivePipelineCard | `fetchActivePipelines` (`.pipeline-states/*.json`) | leitura direta | linhas | REAL | OK. |
| **Home** (workspace) | WorkspaceDigest · Pulse | Ao vivo / última atividade | `fetchMetrics.last_event_at` ∥ `fetchRecentEvents[0].ts` | fresh < 5min | texto + dot | REAL | OK (último evento é real). |
| **Home** (workspace) | WorkspaceDigest | Em progresso | `fetchActivePipelines` | `.length` | inteiro | REAL | OK. |
| **Home** (workspace) | WorkspaceDigest | Concluídas hoje | `fetchSpecs` (FS) | filtra completed_at hoje | inteiro | REAL | OK. |
| **Home** (workspace) | WorkspaceDigest | QA pass-rate hoje | `fetchRecentEvents(200)` filtra `qa.result` hoje | `parseQaOverall`; pass/total | `%` | REAL | OK — janela só de hoje. Frágil se >200 eventos/dia. |
| **Home** (workspace) | WorkspaceDigest | Tokens hoje | `fetchMetrics.tokens_today` → `db.rs metrics_from_db` (SQLite) | SUM do dia | `compactNumber` | STALE | SQLite mirror legado; tende a 0. |
| **Home** (workspace) | WorkspaceDigest | Eventos (total) / sessões | `fetchMetrics.total_events` / `sessions_recent` (SQLite `events`) | COUNT | `compactNumber` | STALE | Mirror SQLite, não conta `events.jsonl` atual. |
| **Home** (workspace) | WorkspaceDigest | Foco do dia | `fetchActivityAggregated` (`events.jsonl`) | maior `tokens_total` por spec | spec + W# | INFERIDO | `tokens_total` por grupo é estimado; "foco" é heurística do maior bucket. |
| **Home** (workspace) | WorkspaceDigest | Últimos 7 dias (HeatBars) | `fetchConsumption.daily_series` (SQLite) | série 7d | barras | STALE | Mirror SQLite. |
| **Atividade** | Timeline | Grupos agregados (spec/wave/ação) | `fetchActivityAggregated` → `dashboard_activity_aggregated` (`events.jsonl`) | GROUP BY spec+wave+action | linhas | REAL | OK estrutura. |
| **Atividade** | Timeline | tokens_total por grupo | idem | SUM de tokens por evento | `… tok` | INFERIDO | `tool.use` em `events.jsonl` raramente carrega tokens reais; soma tende a 0/estimativa. |
| **Atividade** | Timeline | files_touched | idem | conta targets distintos | inteiro | REAL | OK. |
| **Atividade** | Eventos (raw) | Stream de eventos | `useActivityFeed` → `dashboard_recent_events` (`events.jsonl`) | tail + filtros client | linhas | REAL | OK. |
| **Atividade** | Eventos (raw) | Filtros: Tipo / Agente / Wave / Spec | derivados do feed (`actor_id`, `wave`, `spec`) | `Set` único | chips | REAL | OK; chip "Agente" só popula se eventos tiverem `actor_id`. |
| **Telemetria** | Header | badge live/idle | `fetchLiveActivity.is_fresh` ∥ pipelines EXECUTE 24h | bool | badge | REAL | OK. |
| **Telemetria** | EM EXECUÇÃO | linha de pipeline (EXECUTE 24h) | `fetchActivePipelines` filtra phase=EXECUTE & updated_at<24h | filtro client | LivePipelineCard | REAL | OK. |
| **Telemetria** | ATIVIDADE POR FASE | eventos hoje (por fase) | `fetchLiveActivity.by_phase[].events_today` → `telemetry.rs live_activity` (`events.jsonl`) | conta eventos c/ `payload.phase` desde 00:00 UTC | número grande | REAL | OK — mas só conta eventos COM `payload.phase`; ANALYZE silenciosa zera (ver Wave 2). |
| **Telemetria** | ATIVIDADE POR FASE | eventos /5min e /1h | `by_phase[].events_last_5min` / `events_last_hour` | janelas de tempo | número | REAL | OK. |
| **Telemetria** | ATIVIDADE POR FASE | sparkline (minute_buckets) | `by_phase[].minute_buckets` (60 buckets) | bucketização por minuto | SVG bars | REAL | OK. |
| **Telemetria** | ATIVIDADE POR FASE | top_tools por fase | `by_phase[].top_tools` (`tool.use` filtrado por fase) | GROUP BY tool, top 3 | chips | REAL | OK. |
| **Telemetria** | ATIVIDADE POR FASE | card ANALYZE / PLAN / QA / CLOSE | idem `by_phase` | — | número | INFERIDO/AUSENTE | ANALYZE/PLAN/QA/CLOSE quase sempre 0: hooks só taggeiam `payload.phase` em EXECUTE. Wave 1/2 deve corrigir o vocabulário e emitir fase em todas. |
| **Telemetria** | RTK · comandos | Tokens salvos | `tele.data.rtk.tokens_saved` → `telemetry.rs rtk_summary` (binário `rtk gain -p`, cwd=repo) | parse JSON | `formatTokens` | ACUMULADO | RTK filtra por cwd; ainda é total vitalício. **Detalhe R3:** o card Home usa `rtk_summary_global` (sem `-p`) — "92M / 79.4%" é GLOBAL. Exigir rótulo honesto "global RTK" e separar do per-projeto. |
| **Telemetria** | RTK · comandos | Taxa (savings_pct) | `tele.data.rtk.savings_pct` (`avg_savings_pct` do binário) | parse JSON | `formatPct` | ACUMULADO | Idem — média vitalícia. Rotular como acumulado. |
| **Telemetria** | RTK · comandos | nº de comandos comprimidos | `tele.data.rtk.total_commands` | parse JSON | `formatNumber` | ACUMULADO | Idem. |
| **Telemetria** | Hooks · interceptação | Total (tokens) | `tele.data.prevention[]` → `telemetry.rs hook_fire_counts` (`.claude/.metrics/*.jsonl`) | SUM `tokens_saved` de cada linha jsonl | `formatTokens` | ACUMULADO | Arquivos `.metrics/*.jsonl` são append-only vitalícios — nunca zeram entre sessões. **R4: precisa de delta de sessão.** |
| **Telemetria** | Hooks · interceptação | breakdown por hook | `hook_fire_counts` por arquivo (`stem` = nome do hook) | top 5 por `tokens_saved` | barras | ACUMULADO | Idem — total desde a instalação. Delta de sessão. |
| **Telemetria** | Roteamento de modelo | nº dispatches (blocks+allows) | `tele.data.routing` → `telemetry.rs routing_breakdown` (`.metrics/model-routing-gate.jsonl`) | conta linhas block/allow | número | ACUMULADO | Arquivo `.metrics` vitalício. **R5: precisa de delta de sessão.** |
| **Telemetria** | Roteamento de modelo | Intervenção % | `(blocks / (blocks+allows)) * 100` no componente | razão | `formatPct` | INFERIDO | Razão derivada de dois acumuladores vitalícios — não reflete a sessão. |
| **Telemetria** | Roteamento de modelo | Bloqueados / Liberados | `routing.blocks` / `routing.allows` | contagem | número | ACUMULADO | Vitalício. Delta de sessão. |
| **Telemetria** | Roteamento de modelo | por tipo de agente | `routing.by_intent` (`extract_routing_key`: subagent_type) | GROUP BY key, top 6 | barras | ACUMULADO | Vitalício. Delta de sessão. |
| **Telemetria** | Roteamento de modelo | por categoria de ação | `routing.by_note` (NOTE_META) | GROUP BY `note` | lista | ACUMULADO | Vitalício. Delta de sessão. |
| **Telemetria** | Prompt Economy (embutido) | USD (API) | `usePromptEconomy` → `dashboard_prompt_economy` → `cost_block` (SQLite `claude_code_otel`) | SUM `claude_code.cost.usage` | `$x.xx` | REAL/ACUMULADO | REAL se OTEL ativo; é total medido vitalício. Honesto, mas acumulado. |
| **Telemetria** | Prompt Economy (embutido) | Sessions | `claude_events_block` (`claude_code.session.count`) | SUM | número | ACUMULADO | Total vitalício de sessões OTEL. |
| **Telemetria** | Prompt Economy (embutido) | Bytes omitidos / ev | `subtractions_block` (SQLite `events` `mustard.subtraction.applied`) | SUM bytes / COUNT | bytes · ev | AUSENTE | Nenhum evento `mustard.subtraction.applied` no log real (R6). Card sempre 0. |
| **Telemetria** | Prompt Economy (embutido) | OTEL healthy/down | `freshness_block.otel_healthy` | metric<5min ∥ pid recente | texto | REAL | OK. |
| **Telemetria** | Como o projeto está indo | Histórico · Acerto de 1ª | `fetchQualityMetrics.pass_at_1` → `db.rs quality_metrics_from_db` | `SUM(status='completed')/COUNT(*)` em `specs` | `formatPct` | INFERIDO | **NÃO é pass@1 de QA.** É só "% de specs concluídas". Rótulo mente. Recalcular de `qa.result` em `events.jsonl`. |
| **Telemetria** | Como o projeto está indo | Histórico · Precisou refazer | `fetchQualityMetrics.fix_loop_rate` (SQLite `spans`) | derivado de fix-loop em spans | `formatPct` | STALE | `spans` não recebe escrita pós-Wave 4 → tende a 0. |
| **Telemetria** | Como o projeto está indo | Histórico · Tempo médio/fase | `fetchQualityMetrics.avg_phase_duration_ms` (SQLite `spans`) | AVG duração | `formatDurationMs` | STALE | `spans` parado → 0. |
| **Telemetria** | Como o projeto está indo | Histórico · Waves lentas | `fetchQualityMetrics.slowest_waves` (SQLite `spans`) | top 5 duração | inteiro | STALE | `spans` parado → lista vazia. |
| **Telemetria** | Como o projeto está indo | AC dos últimos QAs | `fetchRecentEvents(200)` filtra `qa.result` | `parseQaOverall`; pass/fail/skip | contagens + taxa | REAL | OK — única fonte honesta de QA. Limite 200 eventos pode cortar histórico. |
| **Telemetria** | Como o projeto está indo | Onde o esforço acontece | `tele.data.workflow.by_phase` → `telemetry.rs workflow_by_phase` (`events.jsonl`) | conta `pipeline.phase`+`tool.use` por `payload.phase` | barras | REAL | OK estrutura — mas viesado a EXECUTE (só essa fase taggeia phase). Wave 1/2. |
| **Telemetria** | AGENTES DESPACHADOS | Total dispatches / Erros / lista | `tele.data.agent_activity` → `telemetry.rs agent_activity_from_jsonl` (`agent.start`/`agent.stop`) | pareia start→stop por sessionId+actor.id | número/barras | AUSENTE | `events.jsonl` real não tem nenhum `agent.start`/`agent.stop`. Card sempre vazio (já tem empty-state). Wave 2/6 deve emitir. |
| **Telemetria** | AGENTES DESPACHADOS | avg_duration_ms | idem | delta start→stop via `parse_iso_ms` | `formatDurationMs` | AUSENTE/INFERIDO | Sem eventos; e `parse_iso_ms` usa aritmética de data aproximada (`days = year*365+month*31+day`) — duração seria imprecisa mesmo com eventos. |
| **Telemetria** | FERRAMENTAS — uso acumulado | Read/Bash/Agent/Edit/Write count | `tele.data.tool_breakdown` → `telemetry.rs tool_breakdown` (`events.jsonl` `tool.use`) | GROUP BY tool, top 15 | barras | ACUMULADO | Conta todo `tool.use` do `events.jsonl` (vitalício do arquivo). Honesto rotular "uso acumulado" — já faz isso. |
| **Prompt Economy** | Header | badge OTEL ativo/parado/não-config | `deriveBadge(freshness)` | last_metric_ts + healthy | badge 3-cores | REAL | OK — lógica honesta (red só se nunca viu métrica). |
| **Prompt Economy** | Cache da API | USD total | `dashboard_prompt_economy.cost.usd_total` (`claude_code_otel`) | SUM `cost.usage` | `formatUsd` | REAL/ACUMULADO | Medido pela API via OTEL — confiável. É total vitalício. |
| **Prompt Economy** | Cache da API | Por modelo (USD) | `cost.by_model` (`claude_code_otel` GROUP BY model) | SUM por model | linhas | REAL/ACUMULADO | Idem. |
| **Prompt Economy** | Bytes omitidos pelo Mustard | Total | soma de 4 subtractions | SUM | `formatBytes` | AUSENTE | Sem eventos `mustard.subtraction.applied` no log → sempre 0. |
| **Prompt Economy** | Bytes omitidos · diff-vs-full | bytes / count | `subtractions.diff_vs_full_*` | GROUP BY type | bytes (n ev) | AUSENTE | `emit-subtraction.js --type diff-vs-full` só roda em `/resume` wave≥2 — raríssimo. Emissor essencialmente parado (R6). |
| **Prompt Economy** | Bytes omitidos · wave-slice | bytes / count | `subtractions.wave_slice_*` | idem | bytes (n ev) | AUSENTE | `--type wave-slice` idem `/resume` wave≥2. Poucos eventos reais por sessão; parece travado porque o emissor quase nunca dispara (R6). |
| **Prompt Economy** | Bytes omitidos · review-diff-first | bytes / count | `subtractions.review_diff_first_*` | idem | bytes (n ev) | AUSENTE | Só `/review`. Emissor raro. |
| **Prompt Economy** | Bytes omitidos · analyze-diff-skip | bytes / count | `subtractions.analyze_diff_skip_*` | idem | bytes (n ev) | AUSENTE | `emit-subtraction --type analyze-diff-skip` no início de `/feature`/`/bugfix` — único razoavelmente frequente, mas ainda sem eventos no log atual. |
| **Prompt Economy** | Eventos Claude Code | Sessions | `claude_events.session_count` (`claude_code.session.count`) | SUM | número | ACUMULADO | Total vitalício OTEL. |
| **Prompt Economy** | Eventos Claude Code | Active time | `claude_events.active_time_seconds` (`claude_code.active_time.total`) | SUM | `formatActiveTime` | ACUMULADO | Total vitalício OTEL. |
| **Prompt Economy** | Canary tail | últimas N linhas | `freshness.canary_tail` (`.harness/.canary.log`) | tail 20 | `<pre>` | REAL | OK — só aparece quando badge=red. |
| **Knowledge** | Cabeçalho | "confiança reflete observações" | texto estático | — | parágrafo | INFERIDO | A explicação não bate: `confidence` ≠ `occurrences`; ver abaixo. R9. |
| **Knowledge** | Lista/cards | confidence % | `fetchKnowledgeBrowse`/`fetchSearchKnowledge` → `db.rs knowledge_browse_from_db` (SQLite `knowledge`) ∥ `knowledge.json` | `Math.round(c*100)` | `%` | INFERIDO | `confidence` é seedado em 1.0 por `session-knowledge` e raramente recalculado — quase tudo mostra 100%. Sem sinal real. |
| **Knowledge** | Lista/cards | occurrences | `knowledge.json` entry `.occurrences` | número cru | (no KnowledgeCard) | INFERIDO | **R9:** valor sem semântica clara — ex.: entry "high-hook-retry…" tem `occurrences:76` somando retries de hook, não recorrência do padrão. Atrito (hook-retry) rotulado como `convention`. |
| **Knowledge** | Lista/cards | tool breakdown | `knowledge.json` description embute `{"Write":3,"Bash":20…}` como texto | string crua na description | texto | INFERIDO | Não é campo estruturado — está cravado na `description`. Incompreensível para o usuário. R9. |
| **Knowledge** | Seções por tipo | contagem por tipo | agrupa rows por `row.type` | `reduce` client | `({n})` | REAL | OK contagem — mas tipos poluídos: `convention` contém lições de hygiene/retry que não são convenções de código. R9. |
| **Qualidade** | KPI ribbon | Acerto de 1ª tentativa | `fetchQualityMetrics.pass_at_1` (SQLite `specs`) | `completed/total` | `fmtPct` | INFERIDO | Mesmo bug da Telemetria: é "% specs completas", não pass@1 de QA. Rótulo enganoso. |
| **Qualidade** | KPI ribbon | Precisou refazer | `fetchQualityMetrics.fix_loop_rate` (SQLite `spans`) | derivado fix-loop | `fmtPct` | STALE | `spans` parado pós-Wave 4. |
| **Qualidade** | KPI ribbon | Tempo médio por fase | `fetchQualityMetrics.avg_phase_duration_ms` (SQLite `spans`) | AVG | `fmtSec` | STALE | `spans` parado. |
| **Qualidade** | Specs deste projeto | FASE (por spec) | `fetchSpecs` → `dashboard_specs` (FS `spec/*/`) | lê `phase` do estado | PhaseChip | REAL | OK. |
| **Qualidade** | Specs deste projeto | WAVES (por spec) | `fetchSpecs` (filhos FS `wave-N/`) ∥ contagem de eventos | `fsChildren.length` ∥ `byWave.size` | badge | REAL/INFERIDO | REAL quando há dirs `wave-N`; INFERIDO quando inferido só de eventos taggeados com `wave`. |
| **Qualidade** | Specs deste projeto | AC (pass/fail/skip por spec) | `useActivityFeed` filtra `qa.result` | `parseQaOverall` agregado por spec/wave | AcBreakdown | REAL | OK — fonte honesta. |
| **Qualidade** | Specs deste projeto | RETRIES (por spec/wave) | `useActivityFeed` filtra `retry.attempt` | conta eventos `retry.attempt` | número | AUSENTE | **R: nenhum `retry.attempt` é emitido** (só citado em bugfix/SKILL.md). Coluna sempre 0. Wave 3 deve emitir o evento. |
| **Qualidade** | Qualidade por papel | Pass@1 (por role) | `quality_metrics_from_db` `RoleQuality` | **hardcoded `pass_at_1: 0.0`** | `fmtPct` | INFERIDO | Campo literalmente fixado em 0.0 no Rust (`db.rs` ~617). Sempre mostra 0,0%. Remover ou calcular. |
| **Qualidade** | Qualidade por papel | Fix loops / Amostras | `RoleQuality.fix_loops` / `samples` (SQLite `spans`) | COUNT | número | STALE | `spans` parado. |
| **Qualidade** | Waves mais lentas | duração | `quality_metrics.slowest_waves` (SQLite `spans`) | top 5 duração | `fmtSecExact` | STALE | `spans` parado → seção não renderiza. |
| **Qualidade** | Tokens por fase | input/output avg | `quality_metrics.tokens_by_phase` (SQLite `spans` AVG) | AVG por phase | barras in/out | STALE | `spans` parado → seção não renderiza. |
| **Comandos** | Catálogo | cards de comando (cmd, categoria, exemplos…) | estático `src/data/commands-catalog.ts` | — | lista | REAL | OK — conteúdo curado estático, não é métrica. Risco: drift se comandos mudarem no Mustard. |
| **PRD** | Builder | todos os campos do formulário | estado local React + `localStorage` (`mustard-prd-draft`) | `generatePrdMarkdown` | preview markdown | REAL | OK — ferramenta de geração, não exibe métrica. Sem proveniência de dados. |
| **PRD** | Builder | seletor de Projeto | `discoverProjects` (FS) | lista | `<select>` | REAL | OK. |
| **Settings** | Diretório de projetos | path configurado | `useStore.projectsRoot` (zustand persist) | — | `<code>` | REAL | OK — preferência do usuário. |
| **Settings** | Idioma | pt/en | `useStore.language` (persist) | — | botões | REAL | OK. |
| **Settings** | Projetos descobertos | lista + última atividade | `discoverProjects` (FS) | scan | linhas | REAL | OK. |
| **Settings** | Environment | variáveis `MUSTARD_*`/`OTEL_*` | `readEnv`/`writeEnv` (`.claude/settings.json#env`) + catálogo estático | leitura/escrita JSON | inputs | REAL | OK — config, não métrica. |

**Total: 70 linhas de card/métrica na tabela.**

Distribuição: REAL ~28 · ACUMULADO ~14 · INFERIDO ~12 · STALE ~9 · AUSENTE ~9
(algumas linhas têm classificação dupla REAL/ACUMULADO ou STALE/ACUMULADO quando a fonte muda de modo).

---

## Ações para Wave 5/6

### Wave 5 — Dashboard: bugs (correções de proveniência factual)

1. **Quality `pass_at_1` mente** — `db.rs quality_metrics_from_db` calcula `pass_at_1` como
   `specs completed / total specs`, NÃO como pass@1 de QA. Recalcular a partir de eventos
   `qa.result` em `events.jsonl` (1ª tentativa por wave), ou renomear o rótulo para
   "% specs concluídas". Afeta cards em **Telemetria** e **Qualidade** (KPI ribbon).
2. **`RoleQuality.pass_at_1` hardcoded 0.0** — `db.rs` (~linha 617) fixa o campo em `0.0`.
   Calcular de verdade ou remover a coluna "Pass@1" da tabela "Qualidade por papel".
3. **Métricas STALE do SQLite `spans`/`specs`** — `fix_loop_rate`, `avg_phase_duration_ms`,
   `slowest_waves`, `tokens_by_phase`, `consumption_*`, `metrics_from_db` leem mirror SQLite
   que parou de receber escrita pós-Wave 4. Migrar leitura para `events.jsonl` ou marcar
   visualmente como "dado legado / sem atualização".
4. **RTK rotulado como projeto quando é global** — Home usa `rtk_summary_global` (sem `-p`);
   "92M / 79.4%" é o ganho GLOBAL vitalício de todos os projetos. Adicionar rótulo honesto
   "global RTK" e separar do per-projeto. Telemetria usa `rtk_summary` (`-p`) mas ainda é
   acumulado — rotular "acumulado".
5. **Hooks & Roteamento são acumuladores vitalícios** — `hook_fire_counts` e
   `routing_breakdown` somam arquivos `.claude/.metrics/*.jsonl` append-only desde a
   instalação. Implementar **delta de sessão** (filtrar por `ts` da sessão atual ou guardar
   baseline no `SessionStart`). Aplica-se aos cards "Hooks · interceptação" e "Roteamento de
   modelo" inteiros (Total, Bloqueados, Intervenção %, por agente, por nota).
6. **`parse_iso_ms` impreciso** — aritmética de data aproximada (`days = year*365+month*31+day`)
   produz durações erradas atravessando virada de mês. Corrigir antes de confiar em
   `avg_duration_ms` de "Agentes despachados".
7. **Custo USD inferido vs medido** — `consumption` custo é `tokens × tabela de preço`
   (INFERIDO); Prompt Economy USD é medido por OTEL (REAL). Não exibir os dois como
   equivalentes; deixar claro qual é estimativa.

### Wave 6 — Dashboard: IA + design (cards AUSENTES que dependem de novos emissores)

8. **Agentes despachados — AUSENTE** — depende de `agent.start`/`agent.stop` em
   `events.jsonl`, que nenhum hook emite hoje. Wave 2/6 precisa emitir esses eventos
   (SubagentStart/SubagentStop hooks) antes do card ter valor. Até lá, manter empty-state.
9. **Prompt Economy "Bytes omitidos" — AUSENTE** — depende de `mustard.subtraction.applied`.
   `emit-subtraction.js` existe mas só é invocado em `/resume` (wave≥2), `/review` e início
   de `/feature`/`/bugfix`. Poucos disparos reais por sessão → card parece travado (R6).
   Wave 6 deve: ou ampliar os pontos de emissão (diff-vs-full a cada wave), ou rotular o
   card como "raro por design" e mostrar quando foi o último evento.
10. **Coluna RETRIES — AUSENTE** — `Quality.tsx` conta eventos `retry.attempt` que **nunca
    são emitidos** (só citados em `bugfix/SKILL.md` como texto). Wave 3 deve emitir
    `retry.attempt` de verdade; só então a coluna terá valor.
11. **ANALYZE/PLAN/QA/CLOSE silenciosas** — cards de fase mostram ~0 porque os hooks só
    taggeiam `payload.phase` durante EXECUTE. Wave 1 (vocabulário canônico) + Wave 2
    (ANALYZE deixa de ser silenciosa) devem fazer todas as fases emitirem `phase`.
    Afeta "Atividade por fase" e "Onde o esforço acontece".
12. **Knowledge incompreensível (R9)** — três problemas: (a) `confidence` seedado em 1.0
    e quase nunca recalculado → tudo mostra 100%; (b) `occurrences` sem semântica — mistura
    contagem de hook-retries com recorrência de padrão; (c) atrito operacional (entries
    "high-hook-retry…") rotulado como `convention`, e o "tool breakdown" cravado dentro da
    string `description` em vez de campo estruturado. Wave 4 (Knowledge despoluída) deve
    redefinir o schema; Wave 6 deve então redesenhar a página em cima do schema limpo.
