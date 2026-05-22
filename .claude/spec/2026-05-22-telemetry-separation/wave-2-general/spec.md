# wave-2-general — Coletor OTLP grava no telemetry.db reduzido

### Parent: [[2026-05-22-telemetry-separation]]
### Stage: Close
### Outcome: Completed
### Flags:
### Lang: pt

## Resumo

Fazer o span **nascer atribuído**: o hook que emite `agent.start` grava
`(session_id, tool_use_id) → (spec, wave, agent_id)` no mapa `session_attribution`
do `telemetry.db`; o coletor OTLP, ao receber um trace span, consulta o mapa e
carimba `spec`/`wave_id`/`agent_id` no span. Também aponta o coletor para o
`telemetry.db` e grava `claude_code_otel` no schema reduzido. Depende da Wave 1.

## Causa raiz

`apps/rt/src/run/otel/store.rs:123-166` faz upsert com PK por minuto e grava
`attrs`/`count`/`token_type`. `collector.rs` (rota `/v1/traces`) chama
`economy::writer::record_span`, que grava `spans` SEM `spec`/`wave`/agent — daí o
JOIN de leitura na Wave 3 atual. O runtime de hooks conhece spec/wave/agent no
`agent.start`, mas não repassa isso ao caminho de ingestão OTLP.

## Arquivos

- `apps/rt/src/hooks/tracker.rs` (ou o módulo que emite `agent.start`) — ao emitir `agent.start`, chamar `telemetry::writer.upsert_attribution(session_id, tool_use_id, spec, wave, agent_id)` (módulo da Wave 1)
- `apps/rt/src/run/otel/store.rs` — escrever via `telemetry::writer.upsert_usage_metric` (chave `(metric, model, session_id)`, acumula `sum`, `MAX(updated_at)`); remover `ts_bucket`-minuto/`token_type`/`attrs`/`count`/`signal`
- `apps/rt/src/run/otel/collector.rs` — `/v1/traces`: antes de gravar, `lookup_attribution` e carimbar `spec`/`wave_id`/`agent_id` no run
- `packages/core/src/economy/writer.rs` — `record_span`/`record_api_cost` passam a delegar para `telemetry::writer.record_run` (grava `run_usage` no telemetry.db, já atribuído)

## Tarefas

### General Agent (Wave 2)

- [ ] Emissão de `agent.start`: além do evento, `telemetry::writer.upsert_attribution` no `run_attribution` (mesma fonte de spec/wave/agent que já preenche o payload do evento).
- [ ] `collector.rs`: na rota de traces, resolver atribuição via `lookup_attribution` (primário `tool_use_id`; fallback `session_id` mais recente) e carimbar o run antes de gravar. Sem match → run sem atribuição (como hoje no no-match).
- [ ] `store.rs`: escrever `usage_totals` via `telemetry::writer.upsert_usage_metric` (schema reduzido).
- [ ] `economy/writer.rs`: `record_span`/`record_api_cost` delegam para `telemetry::writer.record_run` (grava `run_usage` atribuído).
- [ ] Ajustar o sample de diagnóstico do coletor ao schema reduzido. `cargo build/test -p mustard-rt` (+ core se a API de `economy/writer.rs` mudar).

## Critérios de Aceitação

- [ ] AC-1: `cargo build -p mustard-rt` passa — Command: `cargo build -p mustard-rt`
- [ ] AC-2: `cargo test -p mustard-rt` passa — Command: `cargo test -p mustard-rt`
- [ ] AC-3: o coletor carimba atribuição na escrita — Command: `bash -c "grep -rq 'attribution' apps/rt/src/run/otel && echo ok"`

## Limites

- `apps/rt/src/hooks/tracker.rs`, `apps/rt/src/run/otel/**`, `packages/core/src/economy/writer.rs`
- NÃO alterar a API do módulo `telemetry` (Wave 1) — consumir
- NÃO tocar no dashboard (Wave 3)
