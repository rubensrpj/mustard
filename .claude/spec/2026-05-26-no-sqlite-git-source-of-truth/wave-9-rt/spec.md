# Hooks de amend + spec hygiene + auto-capture para filesystem

### Stage: planned
### Outcome: Active
### Flags:
### Scope: light
### Checkpoint: 2026-05-27T10:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 3C (renumbered wave-9-rt). **Hooks de amend + spec hygiene + auto-capture para filesystem.** `apps/rt/src/hooks/amend_capture.rs` substitui `AmendWindow` SQLite por arquivo `.claude/spec/{name}/.amend-window.json` (atomic write); `spec_hygiene.rs` lê de filesystem; `auto_capture_summary.rs` lê NDJSON; `prompt_gate.rs` lê NDJSON; `path_guard.rs` lê NDJSON.

**Files (5):** `apps/rt/src/hooks/amend_capture.rs`, `apps/rt/src/hooks/spec_hygiene.rs`, `apps/rt/src/hooks/auto_capture_summary.rs`, `apps/rt/src/hooks/prompt_gate.rs`, `apps/rt/src/hooks/path_guard.rs`.

**Verify:** `cargo build -p mustard-rt` + invariante decrescente.

## Critérios de Aceitação

- [ ] AC-3C-1: `cargo build -p mustard-rt` passa após migração. Command: `cargo build -p mustard-rt`
- [ ] AC-3C-2: Nenhum dos 5 arquivos referencia `SqliteEventStore` / `sqlite_store` / `memory_sqlite`. Command: `bash -c "! git grep -nE 'SqliteEventStore|sqlite_store|memory_sqlite' -- apps/rt/src/hooks/amend_capture.rs apps/rt/src/hooks/spec_hygiene.rs apps/rt/src/hooks/auto_capture_summary.rs apps/rt/src/hooks/prompt_gate.rs apps/rt/src/hooks/path_guard.rs"`

## Plano

## Arquivos

- `apps/rt/src/hooks/amend_capture.rs`
- `apps/rt/src/hooks/spec_hygiene.rs`
- `apps/rt/src/hooks/auto_capture_summary.rs`
- `apps/rt/src/hooks/prompt_gate.rs`
- `apps/rt/src/hooks/path_guard.rs`

## Tarefas

1. `apps/rt/src/hooks/amend_capture.rs` — eliminar `AmendWindow` SQLite; persistir janela em `.claude/spec/{name}/.amend-window.json` via write atomic (tmpfile + rename); leitura idempotente (arquivo ausente → janela default fechada); chaves: `{ "opened_at": iso, "expires_at": iso, "files": [..] }`
2. `apps/rt/src/hooks/spec_hygiene.rs` — substituir queries por filesystem walk de `.claude/spec/*/spec.md` + parse de header (`### Stage:` / `### Outcome:`); manter regras de hygiene
3. `apps/rt/src/hooks/auto_capture_summary.rs` — trocar leitura via SQL por `EventReader::filter_kind` (kinds de `pipeline.complete` / `pipeline.status`); chamar `summary::writer` se necessário (a partir de W4C); aqui só migra a fonte de leitura
4. `apps/rt/src/hooks/prompt_gate.rs` — trocar leitura de "specs pendentes em closed-followup" por filesystem walk + parse de header; arquivamento permanece o mesmo
5. `apps/rt/src/hooks/path_guard.rs` — substituir lookup de boundary da spec ativa via SQL por leitura de `.events/*.ndjson` (último `pipeline.scope` / `pipeline.status`) ou fallback a header da spec

## Dependências

Depende de W1A+W1B+W1C e do batch W2 (já comitado em `dev_rubens`). `summary::writer` final integrate é em W4C (não nesta sub-spec).

## Limites

- CAP RÍGIDO: ≤5 arquivos
- Sem stubs preservando nomes SQLite — DELETE callers/usos diretamente
- Invariante decrescente: após commit `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite"` DEVE decrescer
- `.amend-window.json` versionado? Sim — vive ao lado do spec.md, viaja em git; é estado da spec
- Commit message sugerido: `feat(wave-3/rt): W3C — amend+spec-hygiene+auto-capture hooks via filesystem+NDJSON`
