# NDJSON EventReader primitivo — compartilhado por todas as sub-specs downstream

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-27T09:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 1B. **NDJSON EventReader primitivo (compartilhado por TODAS as sub-specs downstream).** CREATE `packages/core/src/events/{mod,reader,types}.rs`. `EventReader` é struct concreta (sem trait — diretiva do usuário: sem abstração por hipótese): `stream(path) -> impl Iterator<Item=Event>` usa `BufReader` + `serde_json::Deserializer::from_reader().into_iter::<Event>()` (streaming linha-a-linha, zero load full-file); `cached_for_session(spec) -> &[Event]` cache em RAM com chave `(path, mtime)` process-lifetime, invalida em mtime change; `filter_kind(kind) -> impl Iterator<…>` adapter zero-alocação. `Event` é struct lenient-serde com `kind: String` + `payload: serde_json::Value` (catch-all) seguindo `core-lenient-serde-model`. Benchmark obrigatório: stream 10k linhas <50ms. **Files (4):** `packages/core/src/lib.rs` (export), `packages/core/src/events/mod.rs`, `packages/core/src/events/reader.rs`, `packages/core/src/events/types.rs` + perf test embutido em reader.rs via `#[cfg(test)]`.

## Critérios de Aceitação

- [x] AC-1B-1: benchmark embutido de stream 10k linhas reporta p95 <50ms. Command: `cargo test -p mustard-core events::reader::bench`

## Plano

## Arquivos

- `packages/core/src/lib.rs` (export)
- `packages/core/src/events/mod.rs`
- `packages/core/src/events/reader.rs`
- `packages/core/src/events/types.rs`

## Tarefas

1. `packages/core/src/events/types.rs` — CREATE: `pub struct Event { pub kind: String, pub payload: serde_json::Value }` com `#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]`; lenient: campos extras ignorados via `#[serde(flatten)]` ou `deny_unknown_fields` ausente
2. `packages/core/src/events/reader.rs` — CREATE: struct `EventReader` concreta sem trait; impl `stream(path: &Path) -> impl Iterator<Item=Event>` via `BufReader` + `serde_json::Deserializer::from_reader(...).into_iter::<Event>()`; impl `cached_for_session` com cache `HashMap<(PathBuf, SystemTime), Vec<Event>>` invalidado por `fs::metadata(path)?.modified()`; impl `filter_kind<'a>(iter: impl Iterator<Item=Event>+'a, kind: &'a str) -> impl Iterator<Item=Event>+'a` zero-alocação; bloco `#[cfg(test)]` com benchmark que gera 10k linhas NDJSON em tempfile e mede tempo de stream p95 <50ms via `std::time::Instant`
3. `packages/core/src/events/mod.rs` — CREATE: `pub mod reader; pub mod types; pub use reader::EventReader; pub use types::Event;`
4. `packages/core/src/lib.rs` — adicionar `pub mod events;` e re-export `pub use events::{EventReader, Event};`

## Dependências

(nenhuma — W1B não depende de outras sub-specs)

## Limites

- CAP RÍGIDO: ≤5 arquivos (já satisfeito por construção)
- Sem stubs preservando nomes SQLite
- Após commit: `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite" -- 'packages/**/*.rs' 'apps/**/*.rs'` count DEVE decrescer (ou ficar igual se sub-spec não toca esses arquivos — caso W1B que CRIA primitivos novos)
- Benchmarks de performance no AC são binários — passa ou falha
