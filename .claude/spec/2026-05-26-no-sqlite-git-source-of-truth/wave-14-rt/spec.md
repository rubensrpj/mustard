# Run: complete_spec + amend_finalize + epic_fold

### Stage: Plan
### Outcome: Active
### Flags:
### Scope: light
### Checkpoint: 2026-05-27T10:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 4C (renumbered wave-14-rt). **Subcommands de close/finalize/fold migrados para NDJSON + filesystem.**

- `apps/rt/src/run/complete_spec.rs` substitui:
  - `collect_affected_files`: `SqliteEventStore::for_project(...).replay()` → `EventReader::cached_for_session` sobre `.events/*.ndjson` per-spec via `ClaudePaths::for_spec(spec).events_dir()`. Mantém git diff branch + toolBreakdown branches inalterados.
  - `mark_followup`: substituir `SqliteEventStore::for_project(...) → store.append(...)` por emissão NDJSON via `crate::run::event_writer_ndjson::write_event_with_ts` (canonical sink, já usado em `emit_phase_close`). `pipeline_state_from_events` projection lê do NDJSON.
  - `emit_completed_status`: idem (status emit via NDJSON).
  - `archive_followups`: `store.distinct_specs()` → filesystem walk `.claude/spec/*/` + parse de header (`### Stage:` / `### Outcome:`). Para cada spec, ler `.events/*.ndjson` + `pipeline_state_from_events` → check status. `archive_followups_legacy_fs` fallback removido (era para o caso SQLite indisponível; agora NDJSON sempre disponível).
  - Drop `chama summary::writer` no close (sub-spec follow-up — não escala em cap aqui).
- `apps/rt/src/run/amend_finalize.rs` REIMPLEMENTAÇÃO completa (resolve pendência W3B: `amend_finalize::run` foi removido de `session_cleanup`; W4C re-introduz com leitura do `.amend-window.json` criado em W3C):
  - Substituir `AmendWindow` SQLite por leitura do `.amend-window.json` per-spec (mesmo schema usado em W3C `amend_capture.rs`).
  - `run` / `run_with_root` / `run_cli`: walk `.claude/spec/*/.amend-window.json`, filtrar por `session_id` (campo no schema), para cada window: `finalize_window` (compute status via `decide_status`, lê eventos amend via `EventReader` filtrando kinds `pipeline.amend.*`, append `## Amendments` block em `spec.md`, set `closed: true` no `.amend-window.json` via write atomic, emit `pipeline.amend_close` event via NDJSON).
  - Schema do `.amend-window.json` (de W3C `amend_capture.rs::WindowState`): `{ opened_at, expires_at, files, subprojects, drift, drift_emitted, last_activity_at, build_verde_at, closed, session_id?, spec_id? }`. Se `session_id`/`spec_id` ausentes no schema atual, derivar pelo path (`.../{spec_id}/.amend-window.json`) e adicionar campo no schema (compatibilidade forward — leitor antigo ignora; ainda só está em uso interno).
  - `EVENT_PIPELINE_AMEND_OPEN` + `EVENT_PIPELINE_AMEND_ACTIVITY` + `EVENT_PIPELINE_AMEND_INTENT` + `EVENT_PIPELINE_AMEND_DRIFT` events para construir `## Amendments` block: lidos via `EventReader::filter_kind` per kind, sobre `.events/*.ndjson` do spec.
- `apps/rt/src/run/epic_fold.rs`:
  - `detect_completed_epics`: continua lendo `.pipeline-states/*.json` (que **continuam sendo escritos** por hooks legacy — não removidos nesta spec; eventual cleanup pode vir em onda dedicada). `state_phase` chama `crate::run::emit_phase::last_phase_for_spec` que já lê NDJSON (W2 migrado).
  - `fold_epic`: substituir `SqliteEventStore::for_project(...).replay()` por leitura cross-spec via `EventReader::stream` sobre `.events/*.ndjson` do epic + filhos (via `ClaudePaths::for_spec(name).events_dir()` per spec). Aggregation logic idêntica.
  - `emit_event`: já roteia via `event_route::emit` (NDJSON sink); remover param `_store: &SqliteEventStore` e callers que o passam.
  - `write_knowledge_entry`: substituir `upsert_knowledge_pattern` (SQLite) por `MarkdownStore::write_atomic(.claude/knowledge/epic-{epic}.md, doc)`. Frontmatter: `{ kind: "epic-summary", confidence: 0.85, source: "epic-fold", concluded_at: <iso>, spec_children: [...] }`. Body: description.

**Files (3):** `apps/rt/src/run/complete_spec.rs`, `apps/rt/src/run/amend_finalize.rs`, `apps/rt/src/run/epic_fold.rs`.

**Verify:** `cargo build -p mustard-rt` + invariante decrescente.

## Critérios de Aceitação

- [ ] AC-4C-1: `cargo build -p mustard-rt` passa após migração. Command: `cargo build -p mustard-rt`
- [ ] AC-4C-2: Nenhum dos 3 arquivos `.rs` referencia `SqliteEventStore` / `sqlite_store` / `memory_sqlite` / `rusqlite::`. Command: `bash -c "! git grep -nE 'SqliteEventStore|sqlite_store|memory_sqlite|rusqlite::' -- apps/rt/src/run/complete_spec.rs apps/rt/src/run/amend_finalize.rs apps/rt/src/run/epic_fold.rs"`
- [ ] AC-4C-3: `amend_finalize::run` existe e tem assinatura compatível com `session_cleanup` re-wire (reimplementa pendência W3B). Command: `bash -c "git grep -nE 'pub fn run\\(' -- apps/rt/src/run/amend_finalize.rs | grep -q '.'"`

## Plano

## Arquivos

- `apps/rt/src/run/complete_spec.rs`
- `apps/rt/src/run/amend_finalize.rs`
- `apps/rt/src/run/epic_fold.rs`

## Tarefas

1. `complete_spec.rs` — substituir todos `SqliteEventStore::for_project(...)` por `EventReader` lookup sobre `.events/*.ndjson`. `mark_followup` emite via `event_writer_ndjson::write_event_with_ts`. Drop branch fallback `legacy-json` (era para SQLite-indisponível; NDJSON sempre disponível). `archive_followups`: filesystem walk de specs + `pipeline_state_from_events` per spec via NDJSON. Drop `archive_followups_legacy_fs`. Drop imports `use mustard_core::store::event_store::EventSink;` + `use mustard_core::store::sqlite_store::SqliteEventStore;`. Tests SQLite-backed (em `#[cfg(test)] mod tests`) reescritos para usar NDJSON fixtures via `crate::run::event_writer_ndjson::write_event_with_ts` + projection helpers; manter os 6+ tests existentes (parse_iso_millis, mark_followup, archive idempotency, archive_followups, etc.) com mesma cobertura semântica. Se tests inflarem cap, mover 2 dos mais simples para um sibling test file noutra sub-spec (W11) — anotar inline.
2. `amend_finalize.rs` — REWRITE completo. Drop imports `AmendWindow`, `SqliteEventStore`, `EventSink`. New struct interna `LoadedWindow { spec_id: String, session_id: String, state: WindowState }` (WindowState reusa o schema de W3C amend_capture — mas amend_capture é `pub(crate)` na crate rt; ou inline cópia mínima neste módulo, ou tornar `pub use crate::hooks::amend_capture::WindowState`). Implementar `read_windows_for_session(project_root, session_id)`: walk `.claude/spec/*/.amend-window.json`, deserialize, filter por `session_id`. `finalize_window`: lê eventos amend per spec via `EventReader::stream(.events/*.ndjson)` filtrando kinds amend.*, decide_status idêntico, build_amendments_block idêntico, append em spec.md via `mustard_core::fs::write_atomic`, set `closed: true` no .amend-window.json, emit `pipeline.amend_close` via NDJSON. `run` / `run_cli`: signature compatível com session_cleanup wiring (mesmo `pub fn run(session_id: &str) -> Result<RunReport>` ou variante sem store param). Re-wire em `session_cleanup.rs` é OUT-OF-SCOPE desta sub-spec (cap apertado) — anotado como follow-up.
3. `epic_fold.rs` — substituir `SqliteEventStore::for_project(...).replay()` por loop sobre spec_set carregando `.events/*.ndjson` per spec via `EventReader::stream`. `emit_event`: drop param `_store`. `write_knowledge_entry`: usa `MarkdownStore::write_atomic(.claude/knowledge/epic-{epic}.md, MarkdownDoc { frontmatter: { kind: "epic-summary", confidence: 0.85, source: "epic-fold", concluded_at, spec_children }, body: description })`. Drop import `use crate::run::memory::upsert_knowledge_pattern;`. Tests inline reescritos para usar NDJSON fixtures (criar `.events/test.ndjson` files via `std::fs::write`).

## Dependências

Depende de W1B (`EventReader`), W1C (`MarkdownStore`), W3 batch (já comitado). Consome `mustard_core::EventReader`, `mustard_core::atomic_md::{MarkdownStore, MarkdownDoc}`, `mustard_core::projection::pipeline_state_from_events`, `crate::run::event_writer_ndjson`.

## Limites

- CAP RÍGIDO: ≤5 arquivos (3 nesta sub-spec)
- Sem stubs preservando nomes SQLite — DELETE callers/usos diretamente
- Invariante decrescente: após commit `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite"` DEVE decrescer
- Reimplementação de `amend_finalize::run` (resolve pendência W3B onde foi removido de `session_cleanup`) — re-wire em `session_cleanup` é follow-up (out-of-scope)
- `summary::writer` integration no close (W1 entrega) é follow-up — não escala em cap aqui; só migra a camada de leitura SQLite, não adiciona summary write
- Commit message sugerido: `feat(wave-4/rt): W4C — complete_spec+amend_finalize+epic_fold via NDJSON+MarkdownStore`
