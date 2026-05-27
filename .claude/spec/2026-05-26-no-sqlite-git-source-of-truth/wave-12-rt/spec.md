# Run: spec_extract + spec_children + skills + verify_emit + DELETE wikilink.rs

### Stage: Plan
### Outcome: Active
### Flags:
### Scope: light
### Checkpoint: 2026-05-27T10:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 4A (renumbered wave-12-rt). **Run subcommands de extração/listagem de specs + skills/verify migrados, e wikilink.rs deletado.**

- `apps/rt/src/run/spec_extract.rs` substitui `SqliteEventStore::for_project(...)` + `economy::writer::record_context_cost` (cost tracking via SQLite) por noop ou emissão de evento `pipeline.economy.context_cost` no NDJSON canônico. Mantém o JSON `measure` line idêntico.
- `apps/rt/src/run/spec_children.rs` substitui `SqliteSpecReader::children_of` + `SqliteSpecReader::waves` por: (a) filesystem walk de `.claude/spec/*/spec.md` já implementado em `scan_filesystem` (mantém); (b) remove a branch `Set A: events` (SqliteSpecReader) — output passa a ser só Header-driven (todos `source = Header`). Remove `correlate_waves` (wave attribution via SqliteSpecReader.waves) — header-only não tem `started_at` então `wave = None` sempre. Decisão consciente: SQLite era a única fonte para `started_at`/`completed_at`/`reason`/`wave` correlation — filesystem header não carrega esses dados. Sub-spec follow-up pode reintroduzir correlação via `.events/*.ndjson` walk se necessário, mas escopo aqui é só remover SQLite.
- `apps/rt/src/run/skills.rs` substitui `scan_invocations` (replay via `SqliteEventStore::for_project`) por leitura de `.events/*.ndjson` filtrando `kind = "skill.invoked"` via `EventReader::filter_kind`. Walk cross-spec usa `ClaudePaths::spec_dir()` + `.events/` per spec.
- `apps/rt/src/run/verify_emit.rs` já lê de NDJSON (W2 migrado) — só remove os imports residuais SQLite se houverem (auditar e cleanup); na prática este arquivo já está limpo após W2C, mas validar.
- **DELETE** `apps/rt/src/run/wikilink.rs` via `git rm`. Decisão usuário 2026-05-26: Obsidian renderiza `[[]]` clicável nativamente, Claude usa Grep direto, dashboard (se precisar) usa `MarkdownStore::find_backlinks` sob demanda. Remove a entrada `pub mod wikilink;` em `apps/rt/src/run/mod.rs` + a variante `RunCmd::WikilinkExtract` + o branch no `match` do dispatcher.

**Files (5):** `apps/rt/src/run/spec_extract.rs`, `apps/rt/src/run/spec_children.rs`, `apps/rt/src/run/skills.rs`, `apps/rt/src/run/wikilink.rs` (DELETE), `apps/rt/src/run/mod.rs`.

**Verify:** `cargo build -p mustard-rt` + invariante decrescente + `wikilink.rs` ausente.

## Critérios de Aceitação

- [ ] AC-4A-1: `cargo build -p mustard-rt` passa após migração. Command: `cargo build -p mustard-rt`
- [ ] AC-4A-2: Nenhum dos 4 arquivos modificados (spec_extract.rs, spec_children.rs, skills.rs, verify_emit.rs) referencia `SqliteEventStore` / `sqlite_store` / `SqliteSpecReader` / `memory_sqlite`. Command: `bash -c "! git grep -nE 'SqliteEventStore|sqlite_store|SqliteSpecReader|memory_sqlite' -- apps/rt/src/run/spec_extract.rs apps/rt/src/run/spec_children.rs apps/rt/src/run/skills.rs apps/rt/src/run/verify_emit.rs"`
- [ ] AC-4A-3: Arquivo `apps/rt/src/run/wikilink.rs` foi removido. Command: `node -e "if(require('fs').existsSync('apps/rt/src/run/wikilink.rs'))process.exit(1)"`
- [ ] AC-4A-4: `apps/rt/src/run/mod.rs` não tem mais `pub mod wikilink;` nem `RunCmd::WikilinkExtract`. Command: `bash -c "! git grep -nE 'mod wikilink|WikilinkExtract' -- apps/rt/src/run/mod.rs"`

## Plano

## Arquivos

- `apps/rt/src/run/spec_extract.rs`
- `apps/rt/src/run/spec_children.rs`
- `apps/rt/src/run/skills.rs`
- `apps/rt/src/run/wikilink.rs` (DELETE)
- `apps/rt/src/run/mod.rs`

## Tarefas

1. `spec_extract.rs` — remover `use mustard_core::store::sqlite_store::SqliteEventStore;` + `use rusqlite::Connection;` + `use mustard_core::economy::writer;` + `use mustard_core::economy::{AgentId, ContextCostFrame, ...};` + helpers `economy_db_path` / `open_economy_conn` / `record_extract_frame`. Cost tracking via SQLite vira no-op (decisão: economy layer fica para W7; até lá, drop cost frames de spec-extract — eram telemetria, não load-bearing). Manter `measure` JSON line + chamadas a `extract_wave` / `extract_acceptance_criteria` idênticas.
2. `spec_children.rs` — remover imports SQLite (`use mustard_core::{SpecChild, SpecReader, SqliteSpecReader, WaveView};`) + função `correlate_waves` + função `child_from_event` + branch "Set A: events" em `list_children`. Output passa a ser puramente Header-driven (todos `source = ChildSource::Header`). Tests que usavam `SqliteEventStore` + `EventSink` removidos (DELETE os 3 tests `correlate_waves_*` e o test `seed_event` helper); manter os 7 tests filesystem-only que já passam.
3. `skills.rs` — substituir `scan_invocations` por implementação que: (a) `ClaudePaths::for_project(project_dir).spec_dir()`; (b) walk `.events/*.ndjson` per spec; (c) `EventReader::filter_kind(stream, "skill.invoked")` extrai os ts + skill name do payload; (d) mantém mapping skill → latest ts. Remove `use mustard_core::store::sqlite_store::SqliteEventStore;`.
4. `verify_emit.rs` — auditar e limpar quaisquer imports SQLite residuais (na prática já está limpo após W2C — só validar via grep no commit final).
5. `wikilink.rs` — `git rm apps/rt/src/run/wikilink.rs`.
6. `mod.rs` — remover `pub mod wikilink;`, remover `RunCmd::WikilinkExtract` variant, remover branch correspondente em `match cmd` no dispatcher.

## Dependências

Depende de W3A-D (todos comitados em `dev_rubens`). Consome `mustard_core::EventReader`, `mustard_core::ClaudePaths`.

## Limites

- CAP RÍGIDO: ≤5 arquivos (4 MODIFY + 1 DELETE)
- Sem stubs preservando nomes SQLite — DELETE callers/usos diretamente
- Invariante decrescente: após commit `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite"` DEVE decrescer
- Behavior change W4A documentado: `spec-children` perde `started_at`/`completed_at`/`reason`/`wave` correlation (eram SQLite-only); follow-up pode reintroduzir via NDJSON walk
- Behavior change W4A documentado: `spec-extract --measure` JSON line preservado, mas o registro de `context_cost_frames` no SQLite é dropado (era telemetria pura)
- Commit message sugerido: `feat(wave-4/rt): W4A — spec_extract+children+skills via NDJSON, DELETE wikilink.rs`
