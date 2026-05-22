# Unificar `mustard-specsdb` em `mustard-core`

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-20T23:30:00Z
### Lang: pt

## PRD

## Contexto

A camada de domínio SDD do Mustard hoje está dividida em dois crates Rust: `mustard-core` (event store SQLite, FS, config, env, modelos crus de evento e pipeline) e `mustard-specsdb` (ViewModels tipadas, funções de projeção puras, trait `SpecReader` e adapters Sqlite/InMemory). A justificativa documentada no header de `packages/specsdb/src/lib.rs` é Clean Architecture: "core owns infrastructure, specsdb owns domain". Na prática a fronteira não fecha — `mustard-core::model::event::HarnessEvent` e `mustard-core::model::pipeline::*` são tipos de domínio puros vivendo no crate "de infraestrutura", então a separação por crate não reflete uma diferença real de papel arquitetural nem de cone de dependência (rt, dashboard e cli já carregam `rusqlite` por caminhos próprios). Com um único domínio modelado (specs/waves/QA) e três consumidores compartilhando o mesmo set de dependências pesadas, o split entrega cerimônia sem isolamento: um `Cargo.toml` extra, um membro de workspace extra, um namespace duplicado em todo lugar que importa. Soma-se a isso o nome `specsdb`, que sugere "banco de specs" mas não é nem banco (o banco é o `SqliteEventStore`, em core) nem genérico — é projeção. O resultado é uma fronteira que confunde quem chega novo no monorepo e força manutenção paralela de imports redundantes em rt e dashboard.

## Usuários/Stakeholders

Mantenedores do Mustard (em particular Rubens, que apontou o cheiro arquitetural em 2026-05-20 ao perguntar "por que specsdb não ficou em core?"). Indiretamente: qualquer dev futuro que abrir o monorepo e quiser entender por que `HarnessEvent` mora em "core" mas `SpecView` mora em "specsdb". Decisão registrada inline: se um dia algum consumidor precisar de cone de dependência reduzido (e.g. um `mustard-kernel` sem `rusqlite`), splittamos de volta a partir do módulo já isolado — fica fácil.

## Métrica de sucesso

- `packages/specsdb/` não existe mais no filesystem nem no `Cargo.toml` raiz.
- `mustard-core` expõe quatro módulos de papel definido: `model::view` (tipos de leitura), `projection` (folds puros), `reader` (trait + adapters), `store` (renomeado de `io/`, persistência).
- Nenhuma referência a `mustard_specsdb` ou `mustard-specsdb` permanece em arquivos `.rs` ou `Cargo.toml` do workspace.
- `cargo build --workspace` (excl. crates em execução) e `cargo test -p mustard-core -p mustard-rt -p mustard-dashboard` passam.
- Dashboard frontend compila (`pnpm --filter mustard-dashboard build`).

## Não-Objetivos

- **Não alterar comportamento.** Toda a lógica de projeção, todos os tipos de evento, toda a trait `SpecReader` permanecem byte-equivalentes em comportamento e API pública (apenas o caminho de import muda).
- **Não criar um `mustard-kernel` à parte.** O split kernel/store fica para uma decisão futura, se algum consumidor precisar de cone de dependência reduzido. Por agora, um crate único.
- **Não tocar no schema SQLite** nem nas tabelas materializadas (`specs`, `metrics_projection`). O `rebuild_specs` continua igual, só com imports atualizados.
- **Não renomear o crate `mustard-core`** — só reorganizar módulos internos.
- **Não introduzir compatibilidade transitória** (re-export de `mustard_specsdb` apontando para `mustard_core::...`). Mustard está em dev (memória `feedback_no_migration_dev_phase`), refator é direto.
- **Não tocar `apps/cli`** — Grep confirma que não importa `mustard_specsdb`.

## Critérios de Aceitação

Critérios binários, executáveis. Cada um roda da raiz do projeto; exit 0 = passou. Padrão `node -e "..."` cross-shell-safe (memória `feedback_ac_cross_shell_windows.md`).

- [x] AC-1: Workspace compila (excl. crates em execução) — Command: `cargo build --workspace --exclude mustard-rt --exclude mustard-dashboard`
- [x] AC-2: Testes de core passam — Command: `cargo test -p mustard-core`
- [x] AC-3: mustard-rt compila e testes passam — Command: `cargo test -p mustard-rt`
- [x] AC-4: Dashboard backend testes passam — Command: `cargo test -p mustard-dashboard`
- [x] AC-5: Dashboard frontend compila — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-6: Diretório `packages/specsdb` foi removido — Command: `node -e "process.exit(require('fs').existsSync('packages/specsdb')?1:0)"`
- [x] AC-7: Workspace `Cargo.toml` não lista specsdb — Command: `node -e "process.exit(require('fs').readFileSync('Cargo.toml','utf8').includes('packages/specsdb')?1:0)"`
- [x] AC-8: Cargo.toml de consumidores não referencia mustard-specsdb — Command: `node -e "const fs=require('fs');for(const p of ['apps/rt/Cargo.toml','apps/dashboard/src-tauri/Cargo.toml','apps/cli/Cargo.toml']){if(fs.readFileSync(p,'utf8').includes('mustard-specsdb'))process.exit(1)}"`
- [x] AC-9: Nenhum arquivo .rs do workspace importa mustard_specsdb — Command: `node -e "const fs=require('fs'),path=require('path');const SKIP=new Set(['target','node_modules','.git','.claude']);const walk=(d)=>{for(const e of fs.readdirSync(d,{withFileTypes:true})){if(e.isDirectory()){if(!SKIP.has(e.name))walk(path.join(d,e.name))}else if(e.name.endsWith('.rs')&&fs.readFileSync(path.join(d,e.name),'utf8').includes('mustard_specsdb')){console.error('hit:',path.join(d,e.name));process.exit(1)}}};walk('.')"`
- [x] AC-10: Módulo `store/` existe em core (renomeado de `io/`) — Command: `node -e "const fs=require('fs');process.exit(fs.existsSync('packages/core/src/store/mod.rs')&&!fs.existsSync('packages/core/src/io/mod.rs')?0:1)"`
- [x] AC-11: Módulos `projection/`, `reader/`, `model/view/` existem em core — Command: `node -e "const fs=require('fs');for(const p of ['packages/core/src/projection/mod.rs','packages/core/src/reader/mod.rs','packages/core/src/model/view/mod.rs']){if(!fs.existsSync(p))process.exit(1)}"`
- [x] AC-12: `SpecReader`, `SqliteSpecReader`, `SpecView` re-exportados na raiz de `mustard-core` — Command: `node -e "const c=require('fs').readFileSync('packages/core/src/lib.rs','utf8');for(const s of ['SpecReader','SqliteSpecReader','SpecView']){if(!c.includes(s))process.exit(1)}"`

## Plano

## Informações da Entidade

Sem entidade nova. Esta spec reorganiza módulos existentes preservando API pública:

| Hoje (specsdb) | Vira (core) |
|---|---|
| `mustard_specsdb::model::*` | `mustard_core::model::view::*` |
| `mustard_specsdb::projection::*` | `mustard_core::projection::*` |
| `mustard_specsdb::reader::*` | `mustard_core::reader::*` |
| `mustard_specsdb::ReadError` | `mustard_core::reader::ReadError` (tipo distinto, mora junto da trait) |
| `mustard_core::io::*` | `mustard_core::store::*` (rename de papel) |

`ReadError` permanece como enum separado de `mustard_core::error::Error` (semântica de read-side é diferente: Io/Decode/Invalid vs. Io/NotFound/Parse/Config/Env/InvalidInput/CheckFailed/Sqlite). A conversão `From<crate::error::Error> for ReadError` substitui a antiga `From<mustard_core::error::Error>`.

Re-exports na raiz de `mustard-core/src/lib.rs` espelham os atuais de `mustard-specsdb/src/lib.rs`: `SpecReader`, `SqliteSpecReader`, `InMemorySpecReader`, todas as ViewModels (`SpecView`, `WaveView`, `QualityRollup`, `TimelineNode`, `WorkspaceSummary`, ...) e enums (`Phase`, `Scope`, `SpecStatus`, `AcStatus`, `WaveStatus`, ...).

## Arquivos

```
# Reorganização interna de core
packages/core/src/lib.rs                                  ~ declara mods: store (renomeia io), projection, reader; expõe re-exports de view+reader
packages/core/src/error.rs                                ~ atualiza doc-comment (referência crate::io → crate::store)
packages/core/src/metrics.rs                              ~ ajusta use crate::io::fs → crate::store::fs

# Renomear io/ → store/
packages/core/src/store/mod.rs                            ~ ex io/mod.rs
packages/core/src/store/event_store.rs                    ~ ex io/event_store.rs
packages/core/src/store/sqlite_store.rs                   ~ ex io/sqlite_store.rs (ajusta `use crate::io::event_store::EventSink` em testes)
packages/core/src/store/pipeline_repo.rs                  ~ ex io/pipeline_repo.rs (ajusta `use crate::io::fs` → `crate::store::fs`)
packages/core/src/store/migrations.rs                     ~ ex io/migrations.rs
packages/core/src/store/fs.rs                             ~ ex io/fs.rs

# Absorver specsdb/model → core/model/view
packages/core/src/model/mod.rs                            ~ adiciona pub mod view; re-export pub use view::{…}
packages/core/src/model/view/mod.rs                       + ex specsdb/src/model/mod.rs
packages/core/src/model/view/spec.rs                      + ex specsdb/src/model/spec_view.rs (renomeado, perde sufixo _view dentro de view/)
packages/core/src/model/view/wave.rs                      + ex specsdb/src/model/wave_view.rs
packages/core/src/model/view/quality.rs                   + ex specsdb/src/model/quality_view.rs
packages/core/src/model/view/timeline.rs                  + ex specsdb/src/model/timeline_view.rs
packages/core/src/model/view/workspace.rs                 + ex specsdb/src/model/workspace_view.rs
packages/core/src/model/view/filter.rs                    + ex specsdb/src/model/filter.rs

# Absorver specsdb/projection → core/projection
packages/core/src/projection/mod.rs                       + ex specsdb/src/projection/mod.rs
packages/core/src/projection/card.rs                      + ex specsdb/src/projection/card.rs
packages/core/src/projection/waves.rs                     + ex specsdb/src/projection/waves.rs
packages/core/src/projection/quality.rs                   + ex specsdb/src/projection/quality.rs
packages/core/src/projection/timeline.rs                  + ex specsdb/src/projection/timeline.rs
packages/core/src/projection/workspace.rs                 + ex specsdb/src/projection/workspace.rs

# Absorver specsdb/reader → core/reader
packages/core/src/reader/mod.rs                           + ex specsdb/src/reader/mod.rs (expõe sub-tipo ReadError no escopo do reader)
packages/core/src/reader/error.rs                         + ex specsdb/src/error.rs (renomeado, From<mustard_core::error::Error> → From<crate::error::Error>)
packages/core/src/reader/sqlite.rs                        + ex specsdb/src/reader/sqlite.rs
packages/core/src/reader/memory.rs                        + ex specsdb/src/reader/memory.rs

# Teste de contrato
packages/core/tests/reader_contract.rs                    + ex specsdb/tests/reader_contract.rs (ajusta use mustard_specsdb → mustard_core)

# Deletar (Wave 3)
packages/specsdb/                                         — diretório inteiro

# Workspace
Cargo.toml                                                ~ remove "packages/specsdb" da lista members

# Consumidores
apps/rt/Cargo.toml                                        ~ remove linha mustard-specsdb = { path = ... }
apps/rt/src/run/qa_run_all.rs                             ~ use mustard_specsdb::… → use mustard_core::…
apps/rt/src/run/rebuild_specs.rs                          ~ idem (todas as ocorrências, incl. mustard_specsdb::Phase::…)

apps/dashboard/src-tauri/Cargo.toml                       ~ remove linha mustard-specsdb = { path = ... }
apps/dashboard/src-tauri/src/spec_views.rs                ~ ~10 ocorrências de mustard_specsdb:: → mustard_core::
```

Legenda: `~` modificado, `+` criado/movido, `—` deletado.

## Tarefas

### Wave 1 — core: renomear `io/` → `store/`

- [x] Mover fisicamente `packages/core/src/io/*.rs` para `packages/core/src/store/*.rs` (6 arquivos: mod, event_store, sqlite_store, pipeline_repo, migrations, fs).
- [x] Em `packages/core/src/lib.rs`: trocar `pub mod io;` por `pub mod store;`.
- [x] Atualizar imports internos: `use crate::io::fs` → `use crate::store::fs` (em `metrics.rs`, `store/pipeline_repo.rs`).
- [x] Atualizar `use crate::io::event_store::EventSink` em testes de `store/sqlite_store.rs`.
- [x] Atualizar doc-comments em `error.rs` (`crate::io::sqlite_store::SqliteEventStore` → `crate::store::sqlite_store::SqliteEventStore`) e em `lib.rs` (header comment).
- [x] Build verde: `cargo build -p mustard-core && cargo test -p mustard-core`.

### Wave 2 — core: absorver `mustard-specsdb`

- [x] Criar diretório `packages/core/src/model/view/` e mover os 7 arquivos de `packages/specsdb/src/model/*.rs`, renomeando para perder o sufixo `_view` redundante (`spec_view.rs` → `spec.rs`, etc.). `filter.rs` e `mod.rs` mantêm nome.
- [x] Em `packages/core/src/model/view/mod.rs`: atualizar `pub mod spec_view` → `pub mod spec`, etc., preservando os `pub use` de cada item.
- [x] Em `packages/core/src/model/mod.rs`: adicionar `pub mod view;` + `pub use view::{…}` espelhando o que `specsdb/src/lib.rs` re-exportava.
- [x] Criar `packages/core/src/projection/` e mover os 6 arquivos de `packages/specsdb/src/projection/`. Atualizar imports internos: `use crate::model::…` agora resolve em `core::model::view::…` (ajustar caminhos).
- [x] Em `packages/core/src/lib.rs`: adicionar `pub mod projection;`.
- [x] Criar `packages/core/src/reader/` e mover os 3 arquivos de `packages/specsdb/src/reader/` (mod, sqlite, memory).
- [x] Mover `packages/specsdb/src/error.rs` para `packages/core/src/reader/error.rs`. Trocar `From<mustard_core::error::Error>` por `From<crate::error::Error>`. Atualizar doc-comment.
- [x] Em `packages/core/src/reader/mod.rs`: adicionar `pub mod error;` + `pub use error::{ReadError, Result as ReadResult}`. Renomear o alias para evitar colisão com `crate::error::Result`.
- [x] Em `packages/core/src/lib.rs`: adicionar `pub mod reader;` e re-exports raiz: `pub use reader::{InMemorySpecReader, ReadError, SpecReader, SqliteSpecReader}` + `pub use model::view::{AcStatus, AcceptanceCriterion, FileCount, Phase, PhaseSegment, QualityRollup, Scope, SegmentState, SpecFilter, SpecStatus, SpecStatusFilter, SpecSummary, SpecTrack, SpecView, TimeWindow, TimelineKind, TimelineNode, WaveStatus, WaveView, WorkspaceAlert, WorkspaceAlertKind, WorkspaceSummary}` (espelha o lib.rs do specsdb).
- [x] Mover `packages/specsdb/tests/reader_contract.rs` → `packages/core/tests/reader_contract.rs`. Trocar `use mustard_specsdb::` por `use mustard_core::`.
- [x] Atualizar header doc de `packages/core/src/lib.rs` para mencionar as camadas novas (`projection`, `reader`, `model::view`) e remover a justificativa "infra-only".
- [x] Build verde: `cargo build -p mustard-core && cargo test -p mustard-core`.

### Wave 3 — consumidores + delete

- [x] `apps/rt/Cargo.toml`: remover linha `mustard-specsdb = { path = "../../packages/specsdb" }`.
- [x] `apps/rt/src/run/qa_run_all.rs`: trocar `use mustard_specsdb::{SpecFilter, SpecStatusFilter, SqliteSpecReader, SpecReader}` por `use mustard_core::{SpecFilter, SpecStatusFilter, SqliteSpecReader, SpecReader}`. Trocar `mustard_specsdb::TimeWindow::All` por `mustard_core::TimeWindow::All`.
- [x] `apps/rt/src/run/rebuild_specs.rs`: trocar todas as ocorrências de `mustard_specsdb::` por `mustard_core::` (use statements + qualifiers em `Phase::Analyze`, `SpecView`, etc.).
- [x] `apps/dashboard/src-tauri/Cargo.toml`: remover linha `mustard-specsdb = { path = "../../../packages/specsdb" }`.
- [x] `apps/dashboard/src-tauri/src/spec_views.rs`: trocar as ~10 ocorrências de `mustard_specsdb::` por `mustard_core::`.
- [x] `Cargo.toml` raiz: remover `"packages/specsdb"` da lista `members`.
- [x] Deletar diretório `packages/specsdb/` inteiro.
- [x] Rodar `cargo build --workspace --exclude mustard-dashboard` para regenerar `Cargo.lock` sem a entrada `mustard-specsdb`.
- [x] Build verde: `cargo build --workspace --exclude mustard-dashboard && cargo test -p mustard-rt && cargo test -p mustard-dashboard && pnpm --filter mustard-dashboard build`.

## Dependências

Wave 1 → Wave 2 → Wave 3 (sequencial; cada wave depende do build verde da anterior — não há paralelismo aproveitável).

## Limites

Tocar apenas:
- `packages/core/src/**`, `packages/core/tests/**`, `packages/core/Cargo.toml` (se ajuste necessário)
- `packages/specsdb/**` (delete na Wave 3)
- `apps/rt/Cargo.toml`, `apps/rt/src/run/qa_run_all.rs`, `apps/rt/src/run/rebuild_specs.rs`
- `apps/dashboard/src-tauri/Cargo.toml`, `apps/dashboard/src-tauri/src/spec_views.rs`
- `Cargo.toml` raiz (members)
- `Cargo.lock` (regenerado automaticamente)

NÃO tocar:
- `apps/cli/**` (não importa specsdb — Grep confirma)
- `apps/dashboard/src/**` (frontend — não usa specsdb)
- `.claude/**` (specs, scripts, configs — refactor é Rust-only)
- Schema SQLite, tabelas materializadas, lógica de projeção (apenas paths/imports)
