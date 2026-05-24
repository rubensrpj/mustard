# wave-3-ui — Leitores consomem telemetry::reader (sem join com events)

## Resumo

Apontar todos os leitores de telemetria para o `telemetry.db` e — como o span agora
nasce atribuído (Wave 2) — **remover a CTE de JOIN com `events`**, lendo a economia
direto do span via `GROUP BY spec/wave/agent_id`. Sem ATTACH, sem dependência do
`mustard.db`. Preservar exatamente as medições do inventário. Depende da Wave 1.

## Causa raiz

Hoje os consumidores leem `claude_code_otel`/`spans` do `mustard.db`
(`telemetry.rs:854-1005`, `db.rs:162-962`, `economy/reader.rs:87-696`), e o reader
de economia faz a CTE W4 (`spans` LEFT JOIN `events` em `agent.start`) só para
recuperar spec/wave/agent. Com a atribuição carimbada na escrita (Wave 2) e a
telemetria no `telemetry.db` (Wave 1), o JOIN deixa de ser necessário.

## Arquivos

- `apps/dashboard/src-tauri/src/telemetry.rs` — `cost_block` + counters + frescor → consumir `telemetry::reader` (tabela `usage_totals`)
- `packages/core/src/economy/reader.rs` — substituir a CTE W4 por leitura de `run_usage` auto-atribuído via `telemetry::reader` (`GROUP BY spec/wave_id/agent_id`); `economy_summary`/`context_routing_quality` idem. Mesmos cálculos/colunas de saída
- `apps/dashboard/src-tauri/src/db.rs` — `metrics_from_db`, `quality_metrics_from_db`, `consumption_*`, `aggregate_activity_from_db`, `cost_summary` → `telemetry::reader` (tabela `run_usage`), sem cruzar `events`
- `packages/core/src/store/sqlite_store.rs` — `FROM spans WHERE spec=?1` (trace por spec) → `telemetry::reader`

## Tarefas

### UI Agent (Wave 3)

- [ ] `telemetry.rs`: consumir `telemetry::reader`; manter as 6 leituras (custo total/modelo/sessão, session.count, active_time, frescor). Conferir que somar sem `token_type` dá o mesmo total.
- [ ] `economy/reader.rs`: remover a CTE W4 (join com events); ler `run_usage` atribuído via `telemetry::reader`; mesmos GROUP BY e colunas de saída. Verificar paridade.
- [ ] `db.rs`: consumidores passam a usar `telemetry::reader`; remover cruzamentos com `events`/`specs`.
- [ ] `sqlite_store.rs`: trace por spec via `telemetry::reader`.
- [ ] Validar paridade dos números do dashboard. `cargo build -p mustard-dashboard` + `pnpm --filter mustard-dashboard build`.

## Critérios de Aceitação

- [ ] AC-1: `cargo build -p mustard-dashboard` passa — Command: `cargo build -p mustard-dashboard`
- [ ] AC-2: build do front passa — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-3: a CTE de join com events foi removida do reader — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('packages/core/src/economy/reader.rs','utf8');process.exit(/agent\.start/.test(s)?1:0)"`

## Limites

- `apps/dashboard/src-tauri/src/{telemetry,db}.rs`, `packages/core/src/economy/reader.rs`, `packages/core/src/store/sqlite_store.rs`
- NÃO alterar as medições exibidas — só a origem (telemetry.db) e a atribuição self-contained
- NÃO reintroduzir JOIN/ATTACH com `mustard.db` no caminho de leitura
- NÃO alterar a API do módulo `telemetry` (Wave 1)
