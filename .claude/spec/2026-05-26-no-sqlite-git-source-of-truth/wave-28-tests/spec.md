# Delete orphan SQLite tests + rewrite remaining rt tests to filesystem fixtures (W8A-3)

### Stage: planned
### Outcome: Active
### Flags:
### Scope: mixed
### Checkpoint: 2026-05-27T22:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec W8A-3 da [[2026-05-26-no-sqlite-git-source-of-truth]]. Após W8A-1 (rt readers) e
W8A-2 (dashboard readers) migrarem todos os consumers de produção, restam 9 arquivos de
teste que ainda dependem de `SqliteEventStore` / `rusqlite::Connection`. Esta sub-spec
limpa essa dívida ANTES de W8A-4 deletar `store/`+`telemetry/` em core (senão `cargo test
--no-run` quebra junto com o build).

### Estado atual (entrada)

9 testes SQLite-acoplados:

| Arquivo | Decisão | Razão |
|---|---|---|
| `apps/rt/tests/memory_sqlite_test.rs` | **DELETE** | Substituto já existe (`memory_markdown_test.rs` de W4B). Teste obsoleto. |
| `apps/rt/tests/emit_pipeline_kinds.rs` | **REWRITE** | Valida 8 `pipeline.*` kinds — ainda relevante. NDJSON via `event_projections::read_workspace_events`. |
| `apps/rt/tests/pipeline_state_projection_test.rs` | **REWRITE** | Cobre projeção `pipeline_state_from_events` — relevante. NDJSON fixtures. |
| `apps/rt/tests/amend_finalize.rs` | **REWRITE** | Valida `amend_finalize::run` end-to-end. Trocar seed SQLite por NDJSON + `.amend-window.json` (W3C já criou esse arquivo). |
| `apps/rt/tests/rtk_rewrite_emission.rs` | **REWRITE** | Valida emit `pipeline.economy.savings.rtk-rewrite`. NDJSON. |
| `packages/core/tests/sqlite_fts5_smoke.rs` | **DELETE** | FTS5 só faz sentido com SQLite. Comportamento substituído: search por `MarkdownStore::scan_dir` em memory/knowledge `.md`. |
| `packages/core/tests/parity.rs` | **DELETE** | Comparava JS vs Rust paths em SQLite. Sem SQLite, não há paridade a testar. |
| `packages/core/tests/reader_contract.rs` | **DELETE** | Testava `SpecReader` trait + `SqliteSpecReader`. Sem trait, sem teste. Comportamento equivalente via `state_invariants.rs` (que será refatorada em W8A-4). |
| `packages/core/tests/amend_window_projection.rs` | **DELETE** | Projeção amend-window SQLite. Substituída pela leitura de `.amend-window.json` (W3C). Teste será re-introduzido se necessário em wave futura. |

### Estado alvo (saída)

- 4 arquivos DELETE (sem substituto — comportamento coberto por outros testes ou descontinuado).
- 4 REWRITE em `apps/rt/tests/` para filesystem fixtures + NDJSON.

### Detalhes de REWRITE

#### `emit_pipeline_kinds.rs`

Antes: abre `SqliteEventStore::for_project(project)`, chama `mustard-rt run emit-pipeline --kind X`,
faz `store.replay()` e verifica que o evento foi gravado.

Depois: chama `mustard-rt run emit-pipeline --kind X`, depois lê `<project>/.claude/spec/*/.events/*.ndjson`
via `EventReader::stream` e verifica o evento. Helper `read_pipeline_events(project: &Path) -> Vec<HarnessEvent>`
local ao teste.

#### `pipeline_state_projection_test.rs`

Antes: `store.append(...)` para seedar eventos; `store.replay()` + `pipeline_state_from_events`.

Depois: helper `seed_ndjson(project, spec, event)` escreve uma linha NDJSON em
`<project>/.claude/spec/<spec>/.events/test-session.ndjson`; depois
`event_projections::read_workspace_events(project)` + `pipeline_state_from_events`.

#### `amend_finalize.rs`

Antes: `store.append(...)` para seedar `pipeline.scope` + `pipeline.intent_capture` + `tool.use`.

Depois: NDJSON fixture (3 linhas) + `.amend-window.json` no diretório da spec. Chama
`mustard-rt run amend-finalize --session-id ...` ou diretamente `amend_finalize::run(sid)`.
Valida `pipeline.amend_close` event gravado no NDJSON e `.amend-window.json` removido/marcado finalized.

#### `rtk_rewrite_emission.rs`

Antes: abre `Connection`, lê `savings_records` table.

Depois: chama `mustard-rt run rtk-gain --emit` (ou equivalente), lê
`<project>/.claude/spec/<spec>/.events/<session>.ndjson`, filtra por
`event == "pipeline.economy.savings.rtk-rewrite"`, valida payload.

### Hard rule — sem stub

REWRITE preserva o **mesmo objetivo de teste**. Cada `#[test]` deve ainda fazer um
`assert!` ou `assert_eq!` que falharia se o código de produção retornasse default. Sem
`assert!(true)` placeholder.

DELETE só é aceitável quando o comportamento alvo:
1. Sumiu (FTS5 sem SQLite),
2. Foi substituído por outro teste vigente (memory_sqlite_test → memory_markdown_test),
3. Era específico ao SQLite e não tem analogue (reader contract).

## Critérios de Aceitação

- [ ] AC-W8A3-1: `cargo build --workspace` verde. Command: `cargo build --workspace`
- [ ] AC-W8A3-2: `cargo test --workspace --no-run` compila. Command: `cargo test --workspace --no-run`
- [ ] AC-W8A3-3: 4 arquivos DELETE não existem mais. Command: `node -e "const fs=require('fs'); for(const p of ['apps/rt/tests/memory_sqlite_test.rs','packages/core/tests/sqlite_fts5_smoke.rs','packages/core/tests/parity.rs','packages/core/tests/reader_contract.rs','packages/core/tests/amend_window_projection.rs']){if(fs.existsSync(p)){process.exit(1)}}"`
- [ ] AC-W8A3-4: 4 arquivos REWRITE não importam `SqliteEventStore`/`rusqlite::`. Command: `node -e "const fs=require('fs'); for(const p of ['apps/rt/tests/emit_pipeline_kinds.rs','apps/rt/tests/pipeline_state_projection_test.rs','apps/rt/tests/amend_finalize.rs','apps/rt/tests/rtk_rewrite_emission.rs']){const s=fs.readFileSync(p,'utf8'); if(/SqliteEventStore|rusqlite::|sqlite_store/.test(s)){process.exit(1)}}"`
- [ ] AC-W8A3-5: AC-ANTI-STUB — `emit_pipeline_kinds` lê eventos via `EventReader` ou `read_workspace_events`. Command: `node -e "const s=require('fs').readFileSync('apps/rt/tests/emit_pipeline_kinds.rs','utf8'); if(!/EventReader|read_workspace_events|\\.events.*ndjson/.test(s)){process.exit(1)}"`
- [ ] AC-W8A3-6: AC-ANTI-STUB — testes REWRITE têm pelo menos 1 `assert_eq!` ou `assert!`. Command: `node -e "const fs=require('fs'); for(const p of ['apps/rt/tests/emit_pipeline_kinds.rs','apps/rt/tests/pipeline_state_projection_test.rs','apps/rt/tests/amend_finalize.rs','apps/rt/tests/rtk_rewrite_emission.rs']){const s=fs.readFileSync(p,'utf8'); if(!/assert_eq!|assert!\\(/.test(s)){process.exit(1)}}"`
- [ ] AC-W8A3-7: invariante decrescente — count cai abaixo de 30. Command: `bash -c 'count=$(git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite|TelemetryStore|TelemetryReader|rusqlite::" -- "packages/**/*.rs" "apps/**/*.rs" | wc -l); test "$count" -lt 30'`

## Plano

## Arquivos

- `apps/rt/tests/memory_sqlite_test.rs` — DELETE
- `packages/core/tests/sqlite_fts5_smoke.rs` — DELETE
- `packages/core/tests/parity.rs` — DELETE
- `packages/core/tests/reader_contract.rs` — DELETE
- `packages/core/tests/amend_window_projection.rs` — DELETE
- `apps/rt/tests/emit_pipeline_kinds.rs` — REWRITE (NDJSON)
- `apps/rt/tests/pipeline_state_projection_test.rs` — REWRITE (NDJSON)
- `apps/rt/tests/amend_finalize.rs` — REWRITE (NDJSON + `.amend-window.json`)
- `apps/rt/tests/rtk_rewrite_emission.rs` — REWRITE (NDJSON)

(9 arquivos — 4 acima do cap. Justificativa: cluster mecânico — 5 DELETEs são `git rm` sem
lógica; 4 REWRITEs seguem o mesmo padrão de "abrir NDJSON em `.events/`". O ganho de
agrupar é evitar 9 commits separados quebrando `cargo test --no-run` no meio. Se o agent
sentir overflow, dividir em W8A-3a (5 DELETEs + 1 REWRITE) e W8A-3b (3 REWRITEs).)

## Tarefas

1. **DELETE**: `git rm` em:
   - `apps/rt/tests/memory_sqlite_test.rs`
   - `packages/core/tests/sqlite_fts5_smoke.rs`
   - `packages/core/tests/parity.rs`
   - `packages/core/tests/reader_contract.rs`
   - `packages/core/tests/amend_window_projection.rs`

2. **REWRITE `apps/rt/tests/emit_pipeline_kinds.rs`** — para cada `#[test]`:
   - Use `tempfile::tempdir()` como antes.
   - Substitui `SqliteEventStore::for_project(project).expect("open store")` por helper
     local `fn read_events(project: &Path) -> Vec<HarnessEvent>` que walka
     `project/.claude/spec/*/.events/*.ndjson` (copiando pattern de
     `apps/rt/src/run/event_projections.rs::read_workspace_events`) — ou se o helper já
     foi exportado em W8A-2, importar `mustard_core::projection::read_workspace_events`.
   - Assert idêntico ao original (mesmo `pipeline.*` kind detectado).

3. **REWRITE `apps/rt/tests/pipeline_state_projection_test.rs`** — análogo:
   - Helper `seed_ndjson_event(project, spec, event_json)` escreve linha em
     `<project>/.claude/spec/<spec>/.events/test-session.ndjson`.
   - Substitui `store.append(...)` por `seed_ndjson_event(...)`.
   - Substitui `store.replay()` por `mustard_core::projection::read_workspace_events(project)`.
   - Assert idêntico.

4. **REWRITE `apps/rt/tests/amend_finalize.rs`** — análogo:
   - Helper `seed_ndjson_event(...)` + escrita de `.amend-window.json` no spec dir.
   - Chama `mustard_rt::run::amend_finalize::run(sid)` (já é função pub).
   - Assert: arquivo `.amend-window.json` removido OR `pipeline.amend_close` event presente no NDJSON.

5. **REWRITE `apps/rt/tests/rtk_rewrite_emission.rs`** — análogo:
   - Substitui `rusqlite::Connection` por leitura de NDJSON.
   - Filtra eventos por `event.starts_with("pipeline.economy.savings.")`.
   - Assert payload (`tokens_saved` > 0).

6. **Verify**:
   - `rtk cargo build --workspace`
   - `rtk cargo test --workspace --no-run`
   - AC grep.

## Dependências

- W8A-1 (wave-26-rt) commitada.
- W8A-2 (wave-27-dashboard) commitada (se a função `read_workspace_events` foi movida pro core).
- NÃO toca `packages/core/src/{store,telemetry,reader}` — esses são deletados em W8A-4 (wave-29-core).
- Após esta sub-spec, todos os tests em `cargo test --no-run` compilam SEM `rusqlite`. Próxima sub-spec deleta os módulos.

## Limites

- 9 arquivos (cap 5 + 4, justificado: cluster mecânico de deleção+REWRITE alinhado).
- Modelo: opus.
- Commit message: `chore(wave-8/tests): W8A-3 — delete 5 orphan SQLite tests, rewrite 4 rt tests to NDJSON fixtures`
