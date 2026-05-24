# Wave 2 — Core reader per-spec + projection refresh

## PRD

### Contexto

A Wave 1 mudou onde os eventos vivem (NDJSON per-spec + mini-tabela `pipeline_events`). O dashboard, hoje, consome eventos via `packages/core/src/reader/sqlite.rs` que lê da tabela `events` agora dropada. Sem um reader novo, o dashboard fica cego. Esta wave entrega um leitor NDJSON que varre arquivos `events/*.ndjson` de uma spec/wave, faz **k-way merge por timestamp** entre múltiplos arquivos da mesma pasta (cada subagent escreveu o seu), resolve `$ref` de blobs sob demanda (lazy load — timeline não baixa blob até o usuário expandir), e mantém o **mesmo contrato externo** que o reader SQLite expunha — só a fonte muda. As projeções `projection/timeline.rs`, `projection/card.rs` e `model/view/timeline.rs` ganham os campos novos do shape (tokens, duração, status, input/output com `$ref` opcional, parent_id). O subcomando `rebuild-specs` é reescrito pra repopular a tabela `specs` a partir de `pipeline_events` (não mais de `events`). `metrics_projection` passa a derivar de NDJSON via varredura periódica disparada em CLOSE.

### Acceptance Criteria

- [ ] AC-W2-1: `cargo build -p mustard-core` passa — Command: `cargo build -p mustard-core`
- [ ] AC-W2-2: `cargo test -p mustard-core --test reader_ndjson_contract` passa (3 ndjsons sintéticos → eventos retornam ordenados por timestamp) — Command: `cargo test -p mustard-core --test reader_ndjson_contract`
- [ ] AC-W2-3: `cargo test -p mustard-core --test blob_lazy_resolution` passa (evento com `$ref` retorna sem ler blob até `resolve()` ser chamado) — Command: `cargo test -p mustard-core --test blob_lazy_resolution`
- [ ] AC-W2-4: `cargo test -p mustard-rt --test rebuild_specs_from_pipeline_events` passa — Command: `cargo test -p mustard-rt --test rebuild_specs_from_pipeline_events`
- [ ] AC-W2-5: `mustard-rt run event-projections --view pipeline-state --spec {test-spec}` retorna JSON válido após gerar eventos sintéticos — Command: `cargo test -p mustard-rt --test event_projections_after_w2`
- [ ] AC-W2-6: `cargo clippy --workspace -- -D warnings` passa — Command: `cargo clippy --workspace -- -D warnings`

## Plano

### Arquivos

- `packages/core/src/reader/ndjson.rs` (novo) — `EventReader::for_spec(spec_dir)` + `for_wave(wave_dir)` + `for_session(slug)`; k-way merge
- `packages/core/src/reader/blob.rs` (novo) — `BlobResolver::read(sha)` com cache LRU pequeno
- `packages/core/src/reader/mod.rs` (edição) — expor `ndjson::EventReader`; trait `EventSource` continua estável
- `packages/core/src/reader/sqlite.rs` (edição) — passa a ler `pipeline_events` (lifecycle); remove leitura de `events`
- `packages/core/src/reader/memory.rs` (edição) — adapter in-memory pra testes do novo shape
- `packages/core/src/projection/timeline.rs` (edição) — consumir novo shape; preservar contract pro dashboard
- `packages/core/src/projection/card.rs` (edição) — adaptar para campos novos
- `packages/core/src/model/view/timeline.rs` (edição) — adicionar `tokens_in: Option<u64>`, `tokens_out: Option<u64>`, `duration_ms: Option<u64>`, `status: Option<EventStatus>`, `input_ref: Option<BlobRef>`, `output_ref: Option<BlobRef>`, `parent_id: Option<String>`
- `packages/core/src/model/view/mod.rs` (edição) — re-export tipos
- `apps/rt/src/run/rebuild_specs.rs` (edição) — usar `pipeline_events` como fonte
- `apps/rt/src/run/event_projections.rs` (edição) — adaptar views (`pipeline-state`, `cross-session-timeline`, `agent-visibility`, `session-summary`)
- `packages/core/tests/reader_ndjson_contract.rs` (novo)
- `packages/core/tests/blob_lazy_resolution.rs` (novo)
- `apps/rt/tests/rebuild_specs_from_pipeline_events.rs` (novo)
- `apps/rt/tests/event_projections_after_w2.rs` (novo)

### Tarefas

#### Library Agent (Wave 2)

- [ ] Criar `packages/core/src/reader/ndjson.rs` (varre dir, k-way merge por `ts`, lazy `BlobRef`)
- [ ] Criar `reader/blob.rs` com cache LRU (size cap configurável; default 32 entries)
- [ ] Atualizar `reader/sqlite.rs` para ler `pipeline_events` (lifecycle only)
- [ ] Estender `model/view/timeline.rs` com campos opcionais novos
- [ ] Atualizar `projection/timeline.rs` para consumir o shape novo, mantendo função pública estável
- [ ] Atualizar `projection/card.rs` se afetado
- [ ] Refatorar `event_projections.rs` (4 views) para combinar NDJSON + `pipeline_events`
- [ ] Refatorar `rebuild_specs.rs` para usar `pipeline_events`
- [ ] Testes: contract + blob lazy + rebuild + projections
- [ ] `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings`

### Dependências

Wave 1 (precisa do writer NDJSON funcionando e do `sqlite_schema.sql` atualizado).

### Limites

- **Tocar:** `packages/core/src/reader/**`, `packages/core/src/projection/**`, `packages/core/src/model/view/**`, `apps/rt/src/run/{rebuild_specs,event_projections}.rs`, testes correspondentes.
- **NÃO tocar:** `apps/rt/src/events_ndjson/**` (W1), `apps/dashboard/**` (W3), schema SQLite (W1 já fez).
