# Hooks de session + stop para filesystem

### Stage: Plan
### Outcome: Active
### Flags:
### Scope: light
### Checkpoint: 2026-05-27T10:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 3B (renumbered wave-8-rt). **Hooks de session + stop para filesystem.** `apps/rt/src/hooks/session_start.rs` substitui SELECT do knowledge/memory por glob `.claude/{knowledge,memory}/*.md` (depende de W4B para conteúdo real, mas o reader já pode existir lendo diretórios vazios via `MarkdownStore::scan_dir`); `session_cleanup.rs` lê NDJSON; `stop.rs` lê estado de close de filesystem; `stop_observer.rs` idem; `pre_compact.rs` idem.

**Files (5):** `apps/rt/src/hooks/session_start.rs`, `apps/rt/src/hooks/session_cleanup.rs`, `apps/rt/src/hooks/stop.rs`, `apps/rt/src/hooks/stop_observer.rs`, `apps/rt/src/hooks/pre_compact.rs`.

**Verify:** `cargo build -p mustard-rt`.

## Critérios de Aceitação

- [ ] AC-3B-1: `cargo build -p mustard-rt` passa após migração. Command: `cargo build -p mustard-rt`
- [ ] AC-3B-2: Nenhum dos 5 arquivos referencia `SqliteEventStore` / `sqlite_store` / `memory_sqlite`. Command: `bash -c "! git grep -nE 'SqliteEventStore|sqlite_store|memory_sqlite' -- apps/rt/src/hooks/session_start.rs apps/rt/src/hooks/session_cleanup.rs apps/rt/src/hooks/stop.rs apps/rt/src/hooks/stop_observer.rs apps/rt/src/hooks/pre_compact.rs"`

## Plano

## Arquivos

- `apps/rt/src/hooks/session_start.rs`
- `apps/rt/src/hooks/session_cleanup.rs`
- `apps/rt/src/hooks/stop.rs`
- `apps/rt/src/hooks/stop_observer.rs`
- `apps/rt/src/hooks/pre_compact.rs`

## Tarefas

1. `apps/rt/src/hooks/session_start.rs` — substituir SELECT do `knowledge_patterns` / `memory_decisions` por `MarkdownStore::scan_dir(.claude/knowledge)` e `MarkdownStore::scan_dir(.claude/memory)`; aceitar diretórios vazios (top-N pode ser vazio até W4B popular o conteúdo). Manter ordem de injeção idêntica.
2. `apps/rt/src/hooks/session_cleanup.rs` — trocar leitura de estado terminal via SQL por `EventReader::filter_kind("pipeline.status")` filtrando estados terminais; remoção de pipeline-states + stale state files permanece igual
3. `apps/rt/src/hooks/stop.rs` — trocar leitura do estado de close (events `pipeline.complete` / `pipeline.close`) por `EventReader::cached_for_session`; lógica de hand-off para CLOSE inalterada
4. `apps/rt/src/hooks/stop_observer.rs` — idem: substituir queries por `EventReader::filter_kind` + análise em RAM
5. `apps/rt/src/hooks/pre_compact.rs` — snapshot pré-compaction passa a montar a partir de `EventReader` (últimos N eventos da sessão atual) + leitura de `MarkdownStore` (top-K knowledge/memory)

## Dependências

Depende de W1A+W1B+W1C e do batch W2 (já comitado em `dev_rubens`). `MarkdownStore::scan_dir` aceita diretórios vazios — não bloqueia W4B.

## Limites

- CAP RÍGIDO: ≤5 arquivos
- Sem stubs preservando nomes SQLite — DELETE callers/usos diretamente
- Invariante decrescente: após commit `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite"` DEVE decrescer
- Top-N do session_start pode ficar vazio até W4B; isso é esperado e não falha o build
- Commit message sugerido: `feat(wave-3/rt): W3B — session+stop hooks via NDJSON+MarkdownStore`
