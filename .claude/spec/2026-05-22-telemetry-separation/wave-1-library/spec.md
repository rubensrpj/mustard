# wave-1-library — Módulo telemetry/ dedicado + telemetry.db independente

### Parent: [[2026-05-22-telemetry-separation]]
### Stage: Close
### Outcome: Completed
### Flags:
### Lang: pt

## Resumo

Criar um **módulo dedicado** `packages/core/src/telemetry/` (SOLID: responsabilidade
única = telemetria; trait-backed para testes) dono de tudo: o banco independente
`.harness/telemetry.db`, o schema, e as APIs de escrita e leitura. Tabelas com
nomes claros: `usage_totals` (totais agregados), `run_usage` (uso/custo por
execução, com `spec`/`wave_id`/`agent_id` load-bearing) e `run_attribution`
(mapa para carimbar o run na escrita). Uma migração constrói o `telemetry.db` dos
dados atuais, faz o **backfill** da atribuição nos runs históricos (correlação
única com `events`), remove a telemetria do `mustard.db` e roda `VACUUM`.

## Causa raiz

Hoje a telemetria está espalhada e mal nomeada: `claude_code_otel`/`spans` no
`store/sqlite_schema.sql` (banco quente), escrita em `economy/writer.rs`, leitura
em `economy/reader.rs`, ingest em `economy/sources/otel.rs`. `claude_code_otel`
carrega `ts_bucket`/`signal`/`token_type`/`attrs`/`count` que ninguém lê; `spans`
nasce sem atribuição, forçando JOIN de leitura com `events`. Falta um dono coeso.

## Arquivos

- `packages/core/src/telemetry/mod.rs` (novo) — declara submódulos, re-exporta a API pública; traits `TelemetryWriter` e `TelemetryReader` (DIP) + impl SQLite + fake em memória
- `packages/core/src/telemetry/schema.sql` (novo) — `usage_totals(metric, model, session_id, sum, updated_at, PRIMARY KEY(metric, model, session_id))`; `run_usage` (colunas atuais de spans + `agent_id`, `spec`/`wave_id` load-bearing); `run_attribution(session_id, tool_use_id, spec, wave_id, agent_id, updated_at, PRIMARY KEY(session_id, tool_use_id))`; índices usados
- `packages/core/src/telemetry/store.rs` (novo) — `TelemetryStore`: resolve `.harness/telemetry.db` (env override), WAL + `busy_timeout` + `synchronous=NORMAL` + fast-path `user_version`
- `packages/core/src/telemetry/model.rs` (novo) — structs `UsageMetric`, `RunUsage`, `RunAttribution` (lenient serde onde vier de fora)
- `packages/core/src/telemetry/writer.rs` (novo) — `upsert_usage_metric`, `record_run`, `upsert_attribution`, `lookup_attribution`
- `packages/core/src/telemetry/reader.rs` (novo) — queries cruas: custo total/por-modelo/por-sessão, session.count, active_time, frescor; runs por spec/wave/agent/model/phase, série diária, cache ratio, trace por spec
- `packages/core/src/telemetry/migrate.rs` (novo) — migração one-shot (idempotente)
- `packages/core/src/lib.rs` — `pub mod telemetry;`
- `packages/core/src/store/sqlite_schema.sql` — REMOVER `claude_code_otel`, `spans` e índices

## Tarefas

### Library Agent (Wave 1)

- [ ] Criar o módulo `telemetry/` com `mod.rs` expondo traits `TelemetryWriter`/`TelemetryReader` (trait-backed IO, com fake em memória para testes — ver skill core-trait-backed-io).
- [ ] `schema.sql`: `usage_totals` reduzida, `run_usage` (spans + `agent_id`), `run_attribution`. Embed via `include_str!`.
- [ ] `store.rs`: open de `.harness/telemetry.db` (env override análogo a `MUSTARD_DB_PATH`) com WAL + pragmas + fast-path `user_version`.
- [ ] `model.rs`, `writer.rs`, `reader.rs`: structs + APIs de escrita (`upsert_usage_metric`/`record_run`/`upsert_attribution`/`lookup_attribution`) e leitura (todas as medições do inventário).
- [ ] `migrate.rs` idempotente: agregar `claude_code_otel`→`usage_totals` por `(metric, model, session_id)`; copiar `spans`→`run_usage` fazendo o **backfill** de `spec`/`wave_id`/`agent_id` por correlação única com `events(agent.start)` (primário `tool_use_id`, fallback `session_id`+janela `ts`); `DROP` da telemetria no mustard.db + `VACUUM`. Disparar a migração no open do store (ou via passo em `store/migrations.rs`).
- [ ] Remover `claude_code_otel`/`spans`/índices de `store/sqlite_schema.sql`.
- [ ] Testes: (a) agregação preserva os 5 totais; (b) backfill atribui spec/wave/agent como a CTE atual; (c) migração idempotente; (d) telemetry.db não abre/refere `mustard.db`; (e) fake do trait cobre reader/writer.

## Critérios de Aceitação

- [ ] AC-1: `cargo build -p mustard-core` passa — Command: `cargo build -p mustard-core`
- [ ] AC-2: `cargo test -p mustard-core` passa — Command: `cargo test -p mustard-core`
- [ ] AC-3: módulo dedicado existe e resolve telemetry.db — Command: `bash -c "test -f packages/core/src/telemetry/mod.rs && grep -rq 'telemetry.db' packages/core/src/telemetry && echo ok"`

## Limites

- `packages/core/src/telemetry/**` (novo módulo), `packages/core/src/lib.rs`, `packages/core/src/store/sqlite_schema.sql` (só remover as tabelas movidas)
- NÃO criar dependência de leitura entre `telemetry.db` e `mustard.db` (sem ATTACH no caminho de leitura)
- NÃO tocar em `apps/rt` nem `apps/dashboard` (Waves 2 e 3 consomem este módulo)
- NÃO adicionar dependência externa
