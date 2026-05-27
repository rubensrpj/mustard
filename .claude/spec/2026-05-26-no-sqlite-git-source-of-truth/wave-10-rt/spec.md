# Hooks restantes: tool_result, notification, subagent_inject, knowledge-hook

### Stage: Plan
### Outcome: Active
### Flags:
### Scope: light
### Checkpoint: 2026-05-27T10:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 3D (renumbered wave-10-rt). **Hooks restantes: tool_result, notification, subagent_inject, knowledge-hook.** `tool_result.rs` lê última decisão de NDJSON via `EventReader::cached_for_session`; `notification.rs` lê NDJSON; `subagent_inject.rs` injeta de filesystem (memory/knowledge md via `MarkdownStore::scan_dir`); `knowledge.rs` (hook) escreve em `.claude/knowledge/{slug}.md` atomicamente via `MarkdownStore::write_atomic`.

**Files (4):** `apps/rt/src/hooks/tool_result.rs`, `apps/rt/src/hooks/notification.rs`, `apps/rt/src/hooks/subagent_inject.rs`, `apps/rt/src/hooks/knowledge.rs`.

**Verify:** `cargo build -p mustard-rt`.

## Critérios de Aceitação

- [ ] AC-3D-1: `cargo build -p mustard-rt` passa após migração. Command: `cargo build -p mustard-rt`
- [ ] AC-3D-2: Nenhum dos 4 arquivos referencia `SqliteEventStore` / `sqlite_store` / `memory_sqlite`. Command: `bash -c "! git grep -nE 'SqliteEventStore|sqlite_store|memory_sqlite' -- apps/rt/src/hooks/tool_result.rs apps/rt/src/hooks/notification.rs apps/rt/src/hooks/subagent_inject.rs apps/rt/src/hooks/knowledge.rs"`

## Plano

## Arquivos

- `apps/rt/src/hooks/tool_result.rs`
- `apps/rt/src/hooks/notification.rs`
- `apps/rt/src/hooks/subagent_inject.rs`
- `apps/rt/src/hooks/knowledge.rs`

## Tarefas

1. `apps/rt/src/hooks/tool_result.rs` — substituir lookup de "última decisão" via SQL por `EventReader::cached_for_session` filtrando kinds relevantes (`pipeline.tool.result`, `pipeline.decision.*`); manter o reporting downstream inalterado
2. `apps/rt/src/hooks/notification.rs` — substituir leitura de `notifications`/`pipeline_events` via SQL por `EventReader::filter_kind` agregando contadores de notificação em RAM
3. `apps/rt/src/hooks/subagent_inject.rs` — trocar SELECT de knowledge/memory por `MarkdownStore::scan_dir(.claude/knowledge)` + `MarkdownStore::scan_dir(.claude/memory)`; injeção de top-K segue idêntica
4. `apps/rt/src/hooks/knowledge.rs` (hook PostToolUse/SessionEnd que extrai decisões) — substituir INSERT em `memory_decisions`/`knowledge_patterns` por `MarkdownStore::write_atomic(.claude/knowledge/{slug}.md, doc)`; YAML frontmatter inclui `{ kind, captured_at, source_event, spec }`; corpo MD é a decisão

## Dependências

Depende de W1A+W1B+W1C e do batch W2. Consome `mustard_core::EventReader`, `mustard_core::Event`, `mustard_core::atomic_md::MarkdownStore`. W3E (wikilink_footer) rodando em paralelo pode tocar `hooks/mod.rs` e `registry.rs` — se houver conflito, orquestrador rebaseia.

## Limites

- CAP RÍGIDO: ≤5 arquivos (4 nesta sub-spec)
- Sem stubs preservando nomes SQLite — DELETE callers/usos diretamente
- Invariante decrescente: após commit `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite"` DEVE decrescer
- Slug do markdown em `knowledge.rs` deve ser determinístico (hash curto do conteúdo ou timestamp) para evitar collisions; tasks subjacente especifica
- Commit message sugerido: `feat(wave-3/rt): W3D — tool_result+notification+subagent+knowledge hooks via NDJSON+MarkdownStore`
