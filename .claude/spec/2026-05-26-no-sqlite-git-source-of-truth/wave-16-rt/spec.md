# OTEL — rewrite para NDJSON (preservar telemetria, deletar SQLite)

### Stage: planned
### Outcome: Active
### Flags:
### Scope: light
### Checkpoint: 2026-05-27T11:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 5A (renumbered wave-16-rt). **Telemetria OTEL preservada, canal SQLite morto.**

Decisão do usuário 2026-05-26: telemetria de uso (claude_code.token.usage, claude_code.session.count, etc.) tem que continuar existindo — não é "feature em busca de uso", é o substrato da página /economia + diagnose. Mas o sink SQLite (`apps/rt/src/run/otel/store.rs` → `mustard_core::telemetry::TelemetryStore`/`telemetry.db`) sai. Novo canal: NDJSON eventos `pipeline.telemetry.*` no canal já existente (`crate::run::event_writer_ndjson`), per-spec quando há spec ativa, cross-session via session_id quando não há (escrita em `.events/_session/{session_id}/otel.ndjson` — fallback path quando `spec_id` é None). Diagnose lê NDJSON cross-session.

- `apps/rt/src/run/otel/store.rs` **DELETE** via `git rm`. Era o wrapper SQLite (`Store::open`, `Store::upsert_metric`, `Store::otel_row_count`, `Store::otel_sample`, `subtractions_since`). Tudo migra para NDJSON; sem store handle (cada gravação é stateless: serializa evento + append em arquivo).
- `apps/rt/src/run/otel/mod.rs` MODIFY:
  - Drop `pub mod store;` da declaração de módulos.
  - Atualizar doc-comment do header (atualmente fala em "SQLite store" e "`rusqlite (bundled)`") para refletir NDJSON.
- `apps/rt/src/run/otel/collector.rs` MODIFY:
  - Drop `use super::store::Store` (+ qualquer outro re-export de `store::*`).
  - Substituir `store.upsert_metric(row)` / `store.purge_irrelevant_otel_metrics()` por chamadas a `crate::run::event_writer_ndjson::write_event_with_ts` emitindo `pipeline.telemetry.metric` com payload `{ metric, model, session_id, sum, ts_bucket, attrs }` (preservar todos os campos do `MetricRow`). Filtro `CONSUMED_METRICS` continua aplicado antes da escrita (in-memory check, sem acessar SQLite).
  - `subtractions_since` agora opera sobre NDJSON: `EventReader::stream(.events/**/*.ndjson)` filtrando `kind = "pipeline.telemetry.metric"` + window por timestamp.
- `apps/rt/src/run/otel/diagnose.rs` MODIFY:
  - Drop `use super::store::Store`.
  - `otel_row_count`, `otel_last_bucket`, `otel_sample` → leem NDJSON via `EventReader::filter_kind("pipeline.telemetry.metric")` agregando por session_id/metric/sum. Função `otel_sample` retorna últimos N eventos como `SampleRow` (mantém shape pra preservar output JSON do diagnose-otel).
  - Self-test mode: produzir JSON com `events_observed > 0` (validação binária de que o leitor encontra eventos quando há fixtures NDJSON).
- `apps/rt/src/run/otel/project.rs` MODIFY (se necessário): este módulo é a **projeção OTLP→MetricRow** (puro, sem store). Se já não importa store, é leave-as-is (validar no commit final). Por isso fica fora do `Files` count abaixo.

**Files (4):** `apps/rt/src/run/otel/store.rs` (DELETE), `apps/rt/src/run/otel/mod.rs`, `apps/rt/src/run/otel/collector.rs`, `apps/rt/src/run/otel/diagnose.rs`.

**Verify:** `cargo build -p mustard-rt` + invariante decrescente + `node -e "if(require('fs').existsSync('apps/rt/src/run/otel/store.rs'))process.exit(1)"`.

## Critérios de Aceitação

- [ ] AC-5A-1: `cargo build -p mustard-rt` passa após migração. Command: `cargo build -p mustard-rt`
- [ ] AC-5A-2: Arquivo `apps/rt/src/run/otel/store.rs` foi removido. Command: `node -e "if(require('fs').existsSync('apps/rt/src/run/otel/store.rs'))process.exit(1)"`
- [ ] AC-5A-3: Nenhum dos arquivos `otel/mod.rs`, `otel/collector.rs`, `otel/diagnose.rs` referencia `SqliteEventStore` / `sqlite_store` / `TelemetryStore` / `rusqlite::`. Command: `bash -c "! git grep -nE 'SqliteEventStore|sqlite_store|TelemetryStore|rusqlite::' -- apps/rt/src/run/otel/mod.rs apps/rt/src/run/otel/collector.rs apps/rt/src/run/otel/diagnose.rs"`
- [ ] AC-5A-4: `apps/rt/src/run/otel/mod.rs` não tem mais `pub mod store;`. Command: `bash -c "! git grep -nE 'pub mod store' -- apps/rt/src/run/otel/mod.rs"`

## Plano

## Arquivos

- `apps/rt/src/run/otel/store.rs` (DELETE)
- `apps/rt/src/run/otel/mod.rs`
- `apps/rt/src/run/otel/collector.rs`
- `apps/rt/src/run/otel/diagnose.rs`

## Tarefas

1. `git rm apps/rt/src/run/otel/store.rs`.
2. `otel/mod.rs`: remover `pub mod store;`; reescrever doc-comment do header para refletir NDJSON (event kind `pipeline.telemetry.metric`, leitura via `EventReader`).
3. `otel/collector.rs`: drop `use super::store::*`. Substituir chamadas a `store.upsert_metric(row)` por `event_writer_ndjson::write_event_with_ts(spec_or_session, "pipeline.telemetry.metric", payload, ts)`. Payload preserva todos os campos do `MetricRow` (metric, model, session_id, sum, ts_bucket, attrs). Filtro `CONSUMED_METRICS` aplicado antes da escrita. `purge_irrelevant_otel_metrics` vira no-op (NDJSON nunca recebeu os irrelevantes).
4. `otel/diagnose.rs`: drop `use super::store::*`. `otel_row_count`, `otel_last_bucket`, `otel_sample` reimplementadas sobre `EventReader::stream(events_dir)` + `filter_kind("pipeline.telemetry.metric")` + agregação em RAM. `SampleRow` shape preservado. Self-test produz JSON com `events_observed > 0` quando fixtures presentes (ou `0` quando vazio — fail-open).
5. Validar `otel/project.rs` — se não tem nenhum import SQLite (é puro OTLP→MetricRow), deixar inalterado. Se tem qualquer resíduo, remover.

## Dependências

Depende de W2C `event_writer_ndjson::write_event_with_ts` (já comitado em `dev_rubens` como W6-rt), W1B `EventReader` (já comitado como W2-core). Consome `mustard_core::EventReader`, `crate::run::event_writer_ndjson`.

## Limites

- CAP RÍGIDO: ≤5 arquivos (4 nesta sub-spec — 3 MODIFY + 1 DELETE)
- Sem stubs preservando nomes SQLite — DELETE callers/usos diretamente
- Invariante decrescente: após commit `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite"` DEVE decrescer
- `otel/project.rs` fica fora do diff se já é puro (sem import SQLite); o agent valida e reporta
- `apps/rt/src/mcp/` é OUT-OF-SCOPE (sub-spec irmã wave-17-rt em paralelo)
- Tests inline (`#[cfg(test)] mod tests` em collector/diagnose, se houver) reescritos para usar NDJSON fixtures via `std::fs::write` em tempdir + `EventReader` ao invés de `Store::open`
- Commit message sugerido: `feat(wave-5/rt): W5A — otel via NDJSON, DELETE otel/store.rs`
