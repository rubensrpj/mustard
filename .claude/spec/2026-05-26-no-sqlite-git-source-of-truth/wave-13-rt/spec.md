# Memory + memory_ingest → markdown atomic (+ stop_observer cleanup)

### Stage: planned
### Outcome: Active
### Flags:
### Scope: light
### Checkpoint: 2026-05-27T10:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 4B (renumbered wave-13-rt). **Memory + memory_ingest migrados para markdown atomic, e pendência de W3B (`stop_observer.rs`) resolvida.**

- `apps/rt/src/run/memory.rs`: `agent` subcommand mantém escrita em `.claude/.agent-memory/` JSON (rolling cap 20 já filesystem-only). `decision` / `knowledge` subcommands migram para `MarkdownStore::write_atomic`: cada decisão/lesson/pattern vira `.claude/memory/{slug}.md` (decisions/lessons) ou `.claude/knowledge/{slug}.md` (patterns). YAML frontmatter carrega `{ kind, captured_at, source, spec, confidence? }`. `list` reads via `MarkdownStore::scan_dir(.claude/{memory,knowledge})`. `write` (W7 agent_memory write) + `search` + `feedback` (W7 SQLite-backed): TODOS migrados para `.claude/memory/agent/{slug}.md` (write) + scan + filter (search) + append-only feedback log per memory (`.claude/memory/agent/{slug}.feedback.ndjson`). Drop helpers `ensure_agent_memory_fts`, `insert_agent_memory`, `insert_memory_feedback`, `touch_last_used`, `search_agent_memory`, `default_injection_select`, `parse_iso8601_secs`, `effective_confidence` se eles só servem SQLite; preservar logic decay via NDJSON ts walking se viável dentro do cap.
- `apps/rt/src/run/memory_ingest.rs`: subcommand `memory-ingest` migra para ler legacy `.json` files (knowledge.json, decisions.json, lessons.json) E `.agent-memory/*.json` E escrever em `.claude/{memory,knowledge}/*.md` via `MarkdownStore::write_atomic`. Remove todos imports SQLite (`SqliteEventStore`, `Connection`, `rusqlite::params`). Output JSON shape preservado: `{ "ingested": { "knowledge": N, "decisions": M, "lessons": K, "agent_memory": Z }, "deleted": bool, "errors": [...] }`.
- `apps/rt/src/hooks/stop_observer.rs` (PENDÊNCIA W3B): `bump_last_used` migra para varrer `.claude/memory/agent/*.md` via `MarkdownStore::scan_dir` + atualizar campo `last_used` em frontmatter via `MarkdownStore::write_atomic` quando summary é substring do output. `promote_high_confidence` migra para `MarkdownStore::scan_dir(.claude/memory/agent)`, filtra `confidence >= 0.85 && status == active`, escreve novo `.claude/memory/{decisions,lessons}/{slug}.md` (classificado via `classify(summary)`) e atualiza source `.md` setando `status: promoted` no frontmatter. `recent_agent_memory` (PreCompact) lê via `MarkdownStore::scan_dir(.claude/memory/agent)`, ordena por `last_used` desc, retorna top 3.
- CREATE `.claude/knowledge/.gitkeep` + `.claude/memory/.gitkeep`.

**Files (5):** `apps/rt/src/run/memory.rs`, `apps/rt/src/run/memory_ingest.rs`, `apps/rt/src/hooks/stop_observer.rs`, `.claude/knowledge/.gitkeep` + `.claude/memory/.gitkeep` (par CREATE conta como 1), `apps/cli/templates/.gitignore` (manter knowledge/ e memory/ rastreados; ignorar .events/, .blobs/, .harness/).

**Verify:** `cargo build -p mustard-rt` + invariante decrescente.

## Critérios de Aceitação

- [ ] AC-4B-1: `cargo build -p mustard-rt` passa após migração. Command: `cargo build -p mustard-rt`
- [ ] AC-4B-2: Nenhum dos 3 arquivos `.rs` modificados referencia `SqliteEventStore` / `sqlite_store` / `memory_sqlite` / `rusqlite::`. Command: `bash -c "! git grep -nE 'SqliteEventStore|sqlite_store|memory_sqlite|rusqlite::' -- apps/rt/src/run/memory.rs apps/rt/src/run/memory_ingest.rs apps/rt/src/hooks/stop_observer.rs"`
- [ ] AC-4B-3: Arquivos `.gitkeep` existem. Command: `node -e "['.claude/knowledge/.gitkeep','.claude/memory/.gitkeep'].forEach(f=>{if(!require('fs').existsSync(f))process.exit(1)})"`

## Plano

## Arquivos

- `apps/rt/src/run/memory.rs`
- `apps/rt/src/run/memory_ingest.rs`
- `apps/rt/src/hooks/stop_observer.rs`
- `.claude/knowledge/.gitkeep` + `.claude/memory/.gitkeep` (CREATE — par)
- `apps/cli/templates/.gitignore`

## Tarefas

1. `memory.rs` — substituir `run_decision` / `run_knowledge` / `run_list` / `run_write` / `run_search` / `run_feedback` / `default_injection_select` para usar `MarkdownStore`. Layout disco: `.claude/memory/decisions/{slug}.md` / `.claude/memory/lessons/{slug}.md` / `.claude/knowledge/{slug}.md` / `.claude/memory/agent/{slug}.md`. Slug: `{captured_at_compact}-{hash8}` (mesmo formato do `knowledge.rs` hook). Frontmatter sempre inclui `kind`, `captured_at`, `confidence` (quando aplicável), `source`, `spec`, `status`. Para search: `MarkdownStore::scan_dir` + match em `summary`/body (LIKE behavior); decay lazily computado se `last_used` no frontmatter. Para feedback: append linha JSON em `{slug}.feedback.ndjson` sibling; deprecate/supersede setam `status` no frontmatter via re-write.
2. `memory_ingest.rs` — drop imports SQLite. Função `ingest_knowledge` escreve cada entry como `.claude/knowledge/{slug}.md`. `ingest_memory_file` (decisions/lessons) escreve como `.claude/memory/{decisions,lessons}/{slug}.md`. `ingest_agent_memory_dir` escreve cada `.agent-memory/*.json` como `.claude/memory/agent/{slug}.md`. JSON output shape preservado.
3. `stop_observer.rs` — `bump_last_used`: substituir SELECT `agent_memory ORDER BY at DESC LIMIT 200` por `MarkdownStore::scan_dir(.claude/memory/agent)`, ler frontmatter de cada doc, se `summary` é substring de `text`: ler doc completo via `MarkdownStore::read_one`, atualizar `last_used: <now_iso>` no frontmatter, escrever via `MarkdownStore::write_atomic`. `promote_high_confidence`: scan + filter `confidence >= 0.85 && status == active`, escrever `.claude/memory/{decisions,lessons}/{slug}.md` via classify, atualizar source `.md` setando `status: promoted`. `recent_agent_memory` (PreCompact): scan + sort por `last_used` desc + take 3 summaries.
4. `.gitkeep` CREATE (par).
5. `.gitignore` — manter `knowledge/` + `memory/` rastreados; ignorar `.events/`, `.blobs/`, `.harness/` em `.claude/` quando ainda não estiver ignorado.

## Dependências

Depende de W1C (`MarkdownStore`) e W3 batch (já comitado em `dev_rubens`). Consome `mustard_core::atomic_md::{MarkdownStore, MarkdownDoc, frontmatter::Frontmatter}`.

## Limites

- CAP RÍGIDO: ≤5 arquivos
- Sem stubs preservando nomes SQLite — DELETE callers/usos diretamente
- Invariante decrescente: após commit `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite"` DEVE decrescer
- FTS5 search é dropado — passa a ser scan + LIKE-em-RAM (≤ algumas centenas de entries em projeto típico, performance aceitável)
- Tests legacy SQLite (em `apps/rt/tests/memory_sqlite_test.rs`) NÃO migrados aqui — ficam para W11 (delete-rusqlite-deps); cap apertado
- Commit message sugerido: `feat(wave-4/rt): W4B — memory+memory_ingest+stop_observer via MarkdownStore`
