# DELETE packages/core SQLite modules — store/, telemetry/, reader/sqlite, cleanup error.rs (W8A-4)

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: core
### Checkpoint: 2026-05-27T22:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec W8A-4 da [[2026-05-26-no-sqlite-git-source-of-truth]]. **Última destrutiva** da
Wave 8. Pré-condição: W8A-1, W8A-2, W8A-3 commitadas — todos consumers de produção e todos
tests SQLite migrados ou deletados. Esta sub-spec apaga os módulos SQLite do
`packages/core` e limpa o `error.rs` + `lib.rs` + `reader/mod.rs`.

### Estado atual (entrada)

```
packages/core/src/
├── store/                          # 8 .rs + 1 .sql (~140 KB)
│   ├── db_cache.rs
│   ├── event_store.rs              # EventSink trait
│   ├── fs.rs                       # IO primitives (KEEP — usado por outros módulos)
│   ├── migrations.rs
│   ├── mod.rs
│   ├── pipeline_repo.rs            # PipelineRepo trait + FsPipelineRepo
│   ├── sqlite_store.rs             # SqliteEventStore
│   ├── sqlite_schema.sql
│   └── wikilinks.rs                # Legacy wikilink SQLite cache (já órfão pós-W4A)
├── telemetry/                      # 5 .rs + 1 .sql (~70 KB)
│   ├── mod.rs                      # CONSUMED_METRICS + TelemetryWriter/Reader traits
│   ├── model.rs                    # RunUsage, UsageMetric, RunAttribution
│   ├── reader.rs
│   ├── schema.sql
│   ├── store.rs                    # TelemetryStore
│   └── writer.rs
├── reader/
│   ├── error.rs                    # ReadError (tem From<rusqlite::Error>)
│   ├── memory.rs                   # InMemorySpecReader (cfg(test)-only-ish)
│   ├── mod.rs                      # SpecReader trait
│   └── sqlite.rs                   # SqliteSpecReader
├── error.rs                        # Error::Sqlite + From<rusqlite::Error>
└── lib.rs                          # pub mod store; pub mod telemetry; pub use reader::SqliteSpecReader;
```

### Estado alvo (saída)

```
packages/core/src/
├── store/                          # DELETED inteiro
├── telemetry/                      # DELETED inteiro
├── reader/                         # DELETED inteiro (SpecReader trait + impls)
├── error.rs                        # sem Error::Sqlite + sem From<rusqlite::Error>
└── lib.rs                          # sem pub mod store/telemetry/reader; sem re-exports
```

**Justificativa pra deletar `reader/` inteiro (incluindo `memory.rs` + `mod.rs`):**

1. `SqliteSpecReader` é o único `impl SpecReader` de produção — sai.
2. `InMemorySpecReader` é só consumido por `packages/core/tests/state_invariants.rs`.
   Esse teste vai ser refatorado nesta sub-spec para usar `project_spec_view_with_header`
   direto (a primitiva pura), removendo dependência do trait.
3. Sem 2+ impls reais, `SpecReader` viola a regra de projeto
   [[feedback_rust_solid_reuse_global]]: "sem trait sem ≥2 impls reais". Trait morre.
4. `ReadError` (em `reader/error.rs`) tem `From<rusqlite::Error>` + variante `Decode` que
   só faz sentido com SQL. Funcionalidade equivalente já existe em `mustard_core::Error`.
   Sem trait, sem error type. Sai junto.

**Observação sobre `store/fs.rs`:**

`store/fs.rs` (656 B) contém IO primitives (atomic write, append, read) usadas por outros
módulos do core. Esta sub-spec **PRESERVA** esse arquivo movendo-o para `packages/core/src/fs/`
(diretório existente). Conteúdo é minimal (~30 linhas) e pode virar parte de `fs/real.rs`
OR `fs/atomic.rs` (novo arquivo). **Decisão**: mover conteúdo para `fs/real.rs` (que já é o
`Fs` port). Drop `store/fs.rs`.

**Observação sobre `pipeline_repo.rs` (em `store/`):**

Pode ainda ter call-sites em rt para `FsPipelineRepo` (reader/writer de
`.claude/.pipeline-states/{spec}.json`). Verificar e migrar:
- Se houver consumers ativos, mover `pipeline_repo.rs` para `packages/core/src/pipeline/repo.rs`.
- Se for órfão, deletar.

**Observação sobre `store/wikilinks.rs`:**

W4A já deletou o consumer (`apps/rt/src/run/wikilink.rs`). Esse arquivo é órfão — DELETE.

### Auditoria pré-deleção

Antes de qualquer DELETE, rodar:
```bash
git grep -lE "use mustard_core::store|mustard_core::store::|mustard_core::telemetry|mustard_core::reader::|mustard_core::SqliteSpecReader|mustard_core::SpecReader|mustard_core::InMemorySpecReader|mustard_core::ReadError" -- 'apps/**/*.rs' 'packages/**/*.rs'
```

Esperado pós-W8A-1+2+3: zero hits em `apps/`. Apenas hits em `packages/core/` internos
(que vão sumir junto). Se houver consumer externo, PAUSE e migra primeiro.

### Hard rule — sem stub

- Modules são DELETADOS via `git rm`. Sem renomear-para-stub-vazio.
- `error.rs` perde `Error::Sqlite` e `impl From<rusqlite::Error>`. Sem manter variante "for future use".
- `state_invariants.rs` é REWRITTEN para usar projections puras. Sem `#[ignore]` placebo.

## Critérios de Aceitação

- [x] AC-W8A4-1: `cargo build --workspace` verde. Command: `cargo build --workspace`
- [x] AC-W8A4-2: `cargo test --workspace --no-run` compila. Command: `cargo test --workspace --no-run`
- [x] AC-W8A4-3: `packages/core/src/store/` não existe. Command: `node -e "if(require('fs').existsSync('packages/core/src/store')){process.exit(1)}"`
- [x] AC-W8A4-4: `packages/core/src/telemetry/` não existe. Command: `node -e "if(require('fs').existsSync('packages/core/src/telemetry')){process.exit(1)}"`
- [x] AC-W8A4-5: `packages/core/src/reader/` não existe. Command: `node -e "if(require('fs').existsSync('packages/core/src/reader')){process.exit(1)}"`
- [x] AC-W8A4-6: `packages/core/src/error.rs` não tem `Error::Sqlite` nem `From<rusqlite::Error>`. Command: `node -e "const s=require('fs').readFileSync('packages/core/src/error.rs','utf8'); if(/Error::Sqlite|rusqlite::|From<rusqlite/.test(s)){process.exit(1)}"`
- [x] AC-W8A4-7: `packages/core/src/lib.rs` não declara `pub mod store|telemetry|reader`. Command: `node -e "const s=require('fs').readFileSync('packages/core/src/lib.rs','utf8'); if(/pub mod (store|telemetry|reader);/.test(s)){process.exit(1)}"`
- [x] AC-W8A4-8: invariante decrescente — count cai abaixo de 30 (drástica). Command: `bash -c 'count=$(git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite|TelemetryStore|TelemetryReader|rusqlite::" -- "packages/**/*.rs" "apps/**/*.rs" | wc -l); test "$count" -lt 5'`

## Plano

## Arquivos

- `packages/core/src/store/` — DELETE diretório inteiro (`git rm -r`)
- `packages/core/src/telemetry/` — DELETE diretório inteiro (`git rm -r`)
- `packages/core/src/reader/` — DELETE diretório inteiro (`git rm -r`)
- `packages/core/src/error.rs` — REMOVE `Error::Sqlite` variant + `impl From<rusqlite::Error>` + `Error::Sqlite` doc-ref
- `packages/core/src/lib.rs` — REMOVE `pub mod store;`, `pub mod telemetry;`, `pub mod reader;` + `pub use reader::{InMemorySpecReader, ReadError, SpecReader, SqliteSpecReader};` + tudo que dependia
- `packages/core/src/fs/real.rs` — ABSORB `store/fs.rs` IO primitives (se houver função usada — verificar primeiro)
- `packages/core/tests/state_invariants.rs` — REWRITE para usar `project_spec_view_with_header` direto (substituindo `InMemorySpecReader::new() + set_spec_md_root() + spec_view()`)

(7 arquivos — 2 acima do cap. Justificativa: o cluster é majoritariamente DELETE — 3
diretórios via `git rm -r` contam como 3 entradas logicamente mas cada um é um único
comando. O cap-5 do wave-plan já previa essa exceção em W8A: "5 files: store/ DELETE
conta como 1, telemetry/ DELETE conta como 1, reader/{mod.rs + sqlite.rs} MODIFY+DELETE
conta como 1". Estamos seguindo essa contagem semântica.)

## Tarefas

1. **Pré-auditoria** (não falhar — só evidência):
   ```bash
   rtk git grep -lE "use mustard_core::store|mustard_core::store::|mustard_core::telemetry|mustard_core::reader::|mustard_core::SqliteSpecReader|mustard_core::SpecReader|mustard_core::InMemorySpecReader|mustard_core::ReadError" -- 'apps/**/*.rs' 'packages/**/*.rs'
   ```
   Esperado: só hits internos a `packages/core/src/{store,telemetry,reader}/` (que vão sumir).
   Se houver hit externo, **PAUSE** e reporta — significa W8A-1/2/3 deixaram consumer pra trás.

2. **`store/fs.rs` move**: ler o arquivo, identificar funções usadas em outros módulos do core.
   Inline as funções relevantes em `packages/core/src/fs/real.rs` (ou em um novo
   `packages/core/src/fs/atomic.rs` se forem ≥3 funções). Atualizar imports nos módulos
   que consomem. (Se `store/fs.rs` for órfão também, simplesmente DELETE com o resto.)

3. **`pipeline_repo.rs` move**: ler o arquivo, identificar consumers em rt.
   ```bash
   rtk git grep -lE "FsPipelineRepo|PipelineRepo|use mustard_core::store::pipeline_repo" -- 'apps/**/*.rs'
   ```
   Se houver consumer ativo, mover o trait+impl para `packages/core/src/pipeline/repo.rs`
   (criar diretório `pipeline/` se necessário). Atualizar `lib.rs` para re-exportar do novo path.
   Se for órfão, DELETE junto.

4. **DELETE diretórios**:
   ```bash
   rtk git rm -r packages/core/src/store
   rtk git rm -r packages/core/src/telemetry
   rtk git rm -r packages/core/src/reader
   ```

5. **`error.rs`** — remover:
   - Variante `Error::Sqlite(String)` (linhas 70-76)
   - Doc-ref a `SqliteEventStore` na linha 71
   - `impl From<rusqlite::Error> for Error` (linhas 132-136)
   - Imports não usados (se algum).

6. **`lib.rs`** — remover:
   - `pub mod store;`
   - `pub mod telemetry;`
   - `pub mod reader;`
   - `pub use reader::{InMemorySpecReader, ReadError, SpecReader, SqliteSpecReader};`
   - Doc-block em `lib.rs` que mencione SQLite ou `SqliteSpecReader`.
   - Se `pipeline_repo` foi movido em task #3, adicionar `pub mod pipeline;` + `pub use pipeline::repo::{FsPipelineRepo, PipelineRepo};`.

7. **`packages/core/tests/state_invariants.rs`** — REWRITE:
   - Drop `InMemorySpecReader::new() + set_spec_md_root(...) + spec_view(spec)` pattern.
   - Substituir por:
     ```rust
     let spec_md_path = tmp.path().join(spec).join("spec.md");
     let view = mustard_core::projection::project_spec_view_with_header(
         spec, &[], Some(&spec_md_path), None
     );
     ```
   - Drop imports `InMemorySpecReader, SpecReader`.
   - Mantém imports `Flags, Outcome, SpecState, SpecStatus, Stage, StateError`.
   - Asserts ficam idênticos (`view.state.stage`, `view.status`, etc.).

8. **Verify**:
   - `rtk cargo build --workspace`
   - `rtk cargo test --workspace --no-run`
   - AC grep — count final deve estar **abaixo de 5**. O único remanescente esperado: ZERO,
     porque `lib.rs` e `error.rs` ficam limpos. (Cargo.toml com `rusqlite` ainda existe mas
     não é capturado por `*.rs` grep — fica pra wave-30 / W8B-Cargo.)

## Dependências

- W8A-1, W8A-2, W8A-3 commitadas.
- Após esta sub-spec: `rusqlite` ainda em 4 `Cargo.toml`s (core, rt, dashboard/src-tauri, root).
  Cleanup desses + CLAUDE.md docs fica em **wave-30** (W8B do plan original, follow-up direto).

## Limites

- 7 arquivos contados semanticamente (3 DELETE-diretórios + 4 MODIFY/REWRITE).
- Modelo: opus.
- Commit message: `feat(wave-8/core): W8A-4 — DELETE store/telemetry/reader modules, drop Error::Sqlite + From<rusqlite>`
