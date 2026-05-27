# Migrate `packages/core/src/economy/` from SQLite to NDJSON (W7A — readers + writers + multi_project)

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: core
### Checkpoint: 2026-05-27T20:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec W7A da [[2026-05-26-no-sqlite-git-source-of-truth]]. Wave 7 ("Economy layer → NDJSON")
do wave-plan original previa 1 sub-spec única, mas o cluster `packages/core/src/economy/`
tem 4 arquivos de produção + 2 testes + writer signature change que propaga pra ~6 callers
em `apps/rt/`. Split: **W7A** (este) migra core; **W7B/W7C** migram callers em rt;
**W7D** migra dashboard wire-up. Cada sub-spec ≤5 arquivos conforme cap do wave-plan.

### Estado atual (entrada)

`packages/core/src/economy/` consome `rusqlite::Connection` em todo lugar:

- `reader.rs` — 6 funções (`economy_summary`, `per_agent_costs`, `per_spec_costs`, `per_wave_costs`,
  `savings_breakdown`, `context_routing_quality`) recebem `&Connection` e fazem SQL.
- `writer.rs` — 4 funções (`record_run`, `record_api_cost`, `record_savings`, `record_context_cost`)
  recebem `&Connection` e fazem INSERT.
- `store.rs` — `open_for(project_path) -> Result<Connection>` (wrapper sobre `SqliteEventStore::for_project`).
- `multi_project.rs` — `MultiProjectReader::fan_out` abre `.claude/.harness/mustard.db` read-only por projeto.
- `mod.rs` — re-exporta tudo, incluindo `store::open_for`.

### Estado alvo (saída)

- **`reader.rs`** — 6 funções recebem `project_root: &Path` (ou `scope: &EconomyScope`) e
  fazem **filesystem walk** sobre `.claude/spec/*/.events/*.ndjson` + `.claude/.session/*/.events/*.ndjson`
  (canal cross-spec do OTEL collector), usando `mustard_core::events::EventReader` (W1B).
  Cada função preserva **shape idêntico** ao SQLite (mesmas chaves, mesmos tipos).
- **`writer.rs`** — 4 funções viram **pure payload builders** retornando `serde_json::Value` (o
  payload NDJSON) ou `(event_name, payload)` tuple. RT-side caller (W7B) usa essas funções
  + chama `event_route::emit` pra escrever NDJSON. SEM IO no core. Fail-open trivial (pure).
- **`store.rs`** — DELETE (`open_for` não tem mais consumidor; quem precisava de Connection era
  reader/writer, ambos migrados).
- **`multi_project.rs`** — `MultiProjectReader::fan_out` itera projetos chamando closure
  `Fn(&Path, &ProjectPath) -> Result<T>` (sem rusqlite). Fail-open por projeto (skip).
- **`mod.rs`** — drop `pub use store::open_for`; ajusta re-exports do writer pra nova
  assinatura.

### Mapeamento de fontes (eventos NDJSON consumidos)

| Função (reader) | Evento NDJSON principal | Payload usado | Notas |
|---|---|---|---|
| `economy_summary` | `pipeline.telemetry.metric` (`claude_code.cost.usage` / `.token.usage`) | `sum` (USD float), `session_id`, `ts_bucket` | MEASURED branch (unfiltered) |
| `economy_summary` | `pipeline.telemetry.run` | `cost_usd_micros`, `input_tokens+output_tokens` (sum), `spec`, `wave_id` | ESTIMATED branch (spec/wave-filtered) |
| `economy_summary` | `pipeline.economy.savings.*` | `tokens_saved` | total_tokens_saved |
| `per_agent_costs` | `pipeline.telemetry.run` | `agent_id` (em `extra` ou top-level), `cost_usd_micros`, `tokens` | GROUP BY agent_id |
| `per_spec_costs` | `pipeline.telemetry.run` | `spec`, `cost_usd_micros`, `tokens`, `started_at` | GROUP BY spec |
| `per_wave_costs` | `pipeline.telemetry.run` | `spec`, `wave_id` (em `extra`), `cost_usd_micros`, `tokens` | GROUP BY (spec, wave_id) |
| `savings_breakdown` | `pipeline.economy.savings.*` | `source` (derivado do event suffix), `tokens_saved` | GROUP BY source |
| `context_routing_quality` | `pipeline.telemetry.run` | `cache_read_input_tokens`, `input_tokens` | cache_hit_ratio_permille |
| `context_routing_quality` | `pipeline.economy.context.frame` (futuro — zero hoje) | `prompt_size_bytes`, `prefix_stable_bytes`, `retry_overhead_bytes` | prefix_stable / retry_overhead (= 0 hoje, mesmo que SQLite atual) |

### Decisões de design

1. **Walk cross-spec por padrão**: readers iteram `<project_root>/.claude/spec/*/.events/*.ndjson`
   + `<project_root>/.claude/.session/*/.events/*.ndjson` (OTEL canal cross-spec quando session unattached).
   `EconomyScope::Spec/Wave` filtra in-memory por `payload.spec` / `payload.wave_id` (não restringe
   ao spec_dir porque OTEL escreve cross-spec).
2. **Cache via `EventReader::cached_for_session`**: reader mantém uma `EventReader` por chamada
   (process-lifetime cache), invalidada por mtime. Sem cache cross-call por enquanto.
3. **Writer puros**: cada writer retorna `(event_name: &'static str, payload: Value)`. Caller (rt)
   compõe o `HarnessEvent` e chama `event_route::emit`. Esta divisão preserva o princípio "core
   sem IO" (consumido em [[feedback_rust_solid_reuse_global]]).
4. **`MultiProjectReader::fan_out`** — closure recebe `(&Path, &ProjectPath)` no lugar de
   `(&Connection, &ProjectPath)`. Sem flag `SQLITE_OPEN_READ_ONLY` porque é só filesystem walk.
5. **Backward-compat zero**: assinatura dos readers/writers MUDA. Callers em rt (W7B/W7C) e
   dashboard (W7D) MIGRAM JUNTO. Sem stub transitório (regra `feedback_no_stub_fail_open`).
6. **Eventos `pipeline.economy.run`**: novo evento que o tracker.rs (W7B) vai emitir pra cobrir
   o gap onde `record_task_run` antes só escrevia em telemetry.db. Para a parte da reader, é
   compatível com `pipeline.telemetry.run` (OTEL já emite) — reader aceita AMBOS via filtro
   `kind in {pipeline.telemetry.run, pipeline.economy.run}`.

### Hard rule — sem stub

Critério de sucesso desta sub-spec: nenhum reader/writer retorna `Default`/`Vec::new()` quando
há eventos NDJSON correspondentes. AC-FIXTURE garante via fixtures de eventos + assert no payload.

## Critérios de Aceitação

- [x] AC-W7A-1: `cargo build -p mustard-core` verde. Command: `cargo build -p mustard-core`
- [x] AC-W7A-2: `cargo test -p mustard-core economy --no-run` compila com 0 erros. Command: `cargo test -p mustard-core economy --no-run`
- [x] AC-W7A-3: `packages/core/src/economy/store.rs` deletado. Command: `node -e "if(require('fs').existsSync('packages/core/src/economy/store.rs')){process.exit(1)}"`
- [x] AC-W7A-4: `packages/core/src/economy/reader.rs` não importa `rusqlite::*`. Command: `node -e "const s=require('fs').readFileSync('packages/core/src/economy/reader.rs','utf8'); if(/use rusqlite|rusqlite::/.test(s)){process.exit(1)}"`
- [x] AC-W7A-5: `packages/core/src/economy/writer.rs` não importa `rusqlite::*`. Command: `node -e "const s=require('fs').readFileSync('packages/core/src/economy/writer.rs','utf8'); if(/use rusqlite|rusqlite::/.test(s)){process.exit(1)}"`
- [x] AC-W7A-6: `packages/core/src/economy/multi_project.rs` não importa `rusqlite::*`. Command: `node -e "const s=require('fs').readFileSync('packages/core/src/economy/multi_project.rs','utf8'); if(/use rusqlite|rusqlite::/.test(s)){process.exit(1)}"`
- [x] AC-W7A-7: `economy::reader::economy_summary(&path, Project)` com fixture NDJSON de 2 eventos `pipeline.telemetry.metric` retorna `total_cost_usd_micros > 0` (não default). Command: `cargo test -p mustard-core economy::reader::tests::summary_reads_measured_totals_from_ndjson --no-run`
- [x] AC-W7A-8: `economy::reader::savings_breakdown(&path, Project)` com fixture NDJSON de 2 eventos `pipeline.economy.savings.rtk-rewrite` retorna `total_tokens_saved == soma` (não default). Command: `cargo test -p mustard-core economy::reader::tests::savings_breakdown_reads_ndjson --no-run`
- [x] AC-W7A-9: `economy::reader::per_spec_costs(&path, Project)` com fixture NDJSON de 1 evento `pipeline.telemetry.run` com `payload.spec == "spec-A"` retorna 1 `SpecCost` row (não vec vazio). Command: `cargo test -p mustard-core economy::reader::tests::per_spec_costs_groups_run_events_by_spec --no-run`
- [x] AC-W7A-10: invariante decrescente — `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite|TelemetryStore|TelemetryReader|rusqlite::" -- '*.rs'` count menor que 38 (entrada). Command: `bash -c 'count=$(git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite|TelemetryStore|TelemetryReader|rusqlite::" -- "*.rs" | wc -l); test "$count" -lt 38'`

## Plano

## Arquivos

- `packages/core/src/economy/reader.rs` — REWRITE
- `packages/core/src/economy/writer.rs` — REWRITE (pure payload builders)
- `packages/core/src/economy/multi_project.rs` — REWRITE (no rusqlite)
- `packages/core/src/economy/store.rs` — DELETE
- `packages/core/src/economy/mod.rs` — UPDATE re-exports

(5 arquivos — dentro do cap. Tests em reader.rs ficam **inline em #[cfg(test)] mod tests {}** com fixtures de tempdir + NDJSON.)

## Tarefas

1. **`store.rs`**: `git rm` — apaga arquivo inteiro. Remove `mod store;` + `pub use store::open_for` do `mod.rs`.
2. **`writer.rs`**: REWRITE pra pure payload builders:
   - `pub fn savings_event(rec: &SavingsRecord) -> (String, Value)` — devolve `("pipeline.economy.savings.{source}", payload_json)`.
   - `pub fn context_frame_event(rec: &ContextCostFrame) -> (String, Value)` — devolve `("pipeline.economy.context.frame", payload_json)`.
   - `pub fn run_event(rec: &SpanRecord) -> (String, Value)` — devolve `("pipeline.economy.run", payload_json_com_all_fields)`. (Não substitui OTEL — adiciona canal interno pra estimates do tracker.)
   - `pub fn injection_savings_tokens(skeleton: &str) -> i64` — mantém (já é pure).
   - Drop `iso_to_epoch_ms` (não tem mais call-site interno).
   - Tests inline: cada builder vira `serde_json::Value` equivalente ao shape SQLite antigo.
3. **`multi_project.rs`**: REWRITE pra `fan_out<T, F>(projects: &[ProjectPath], query: F) -> HashMap<ProjectPath, T>` onde `F: Fn(&Path, &ProjectPath) -> Result<T>`. Sem rusqlite, sem open. Cada call recebe project_root path. Fail-open por projeto (try query, skip on Err).
4. **`reader.rs`**: REWRITE 6 funções:
   - Cada uma vira `pub fn name(project_root: &Path, scope: EconomyScope) -> Result<T>`.
   - Helper `walk_events(project_root: &Path) -> impl Iterator<Item=(PathBuf, Event)>` itera `<root>/.claude/spec/*/.events/*.ndjson` + `<root>/.claude/.session/*/.events/*.ndjson`.
   - `economy_summary`: combina MEASURED (eventos `pipeline.telemetry.metric` com metric=`claude_code.cost.usage`/`token.usage`) + ESTIMATED (eventos `pipeline.telemetry.run` ou `pipeline.economy.run`) + SAVINGS (`pipeline.economy.savings.*`). Preserva `unfiltered` branch que prefere measured totals.
   - `per_agent_costs`: GROUP BY `payload.agent_id` (ou `payload.extra.agent_id` para shape OTEL).
   - `per_spec_costs`: GROUP BY `payload.spec`.
   - `per_wave_costs`: GROUP BY `(payload.spec, payload.wave_id)`.
   - `savings_breakdown`: filter `pipeline.economy.savings.*`, GROUP BY `source` (parse do suffix do event name OU `payload.source`).
   - `context_routing_quality`: `cache_hit_ratio_permille` = `Σ cache_read_input_tokens / Σ (input_tokens + cache_read_input_tokens)` × 1000 dos `pipeline.telemetry.run`; outros ratios = 0 se sem `pipeline.economy.context.frame` events (preserva comportamento atual onde `record_context_cost` não tem caller).
   - Tests inline:
     - `summary_reads_measured_totals_from_ndjson` — fixture 2 `pipeline.telemetry.metric` events, assert `total_cost_usd_micros > 0`.
     - `savings_breakdown_reads_ndjson` — fixture 2 `pipeline.economy.savings.rtk-rewrite` events, assert total + per_source.
     - `per_spec_costs_groups_run_events_by_spec` — fixture 1 `pipeline.telemetry.run` event, assert 1 row.
     - `per_agent_costs_groups_run_events_by_agent` — fixture 2 events 2 agents.
     - `per_wave_costs_groups_run_events_by_wave` — fixture 2 events spec/wave.
     - `context_routing_cache_hit_from_telemetry_run` — fixture 1 run com cache_read_input_tokens, assert permille calc.
     - `multi_project_fan_out_iterates_projects` — 2 tempdirs com 1 event each, assert 2 entries.
5. **`mod.rs`**: drop `pub mod store;` + `pub use store::open_for;`. Atualiza re-exports do writer (remove `record_api_cost`, `record_savings`, `record_context_cost`, `record_run` se viraram `*_event` builders). Mantém `MultiProjectReader`, `injection_savings_tokens`.
6. **Verify**: `rtk cargo build -p mustard-core` + `rtk cargo test -p mustard-core economy --no-run` + AC-W7A-10 grep.

## Dependências

- Consome `mustard_core::events::EventReader` (W1B).
- Consome `mustard_core::ClaudePaths` (existente).
- Compatibilidade com OTEL collector emit (W5A): aceita `pipeline.telemetry.run` events.
- NÃO migra callers em rt (`tracker.rs`, `session_cleanup.rs`, etc.) — esses ficam em W7B.
  Esta sub-spec deixa `cargo build -p mustard-rt` QUEBRADO (signature change não-propagada).
  W7B/C migra os callers no commit imediato seguinte.

## Limites

- 5 arquivos (4 REWRITE + 1 DELETE), tudo dentro de `packages/core/src/economy/`.
- Tests inline em `#[cfg(test)] mod tests` em cada arquivo modificado (sem novos arquivos de test).
- Modelo: opus.
- Commit message: `feat(wave-7/core): W7A — economy NDJSON readers+writers+multi_project, DELETE store.rs`
