# Migrate remaining rt SQLite readers to NDJSON + filesystem (W8A-1 — resume_bootstrap + spec_children_tree + session_cleanup + otel cleanup)

### Stage: planned
### Outcome: Active
### Flags:
### Scope: rt
### Checkpoint: 2026-05-27T22:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec W8A-1 da [[2026-05-26-no-sqlite-git-source-of-truth]]. W8 do wave-plan original previa
W8A (delete store/ + telemetry/) + W8B (cleanup Cargo.toml + tests) + W8C (smoke), mas o
trabalho real exige migrar PRIMEIRO os consumers em `apps/rt/` que ainda referenciam
`SqliteEventStore::for_project`, `SqliteSpecReader::for_project` e
`mustard_core::telemetry::TelemetryStore::for_project`. Esta sub-spec é a primeira parte de
W8A — migra os consumers em rt antes do W8D deletar os módulos core. Sem migração-antes-de-deleção
o build quebra.

### Estado atual (entrada)

Consumers SQLite em rt (post-W7):

| Arquivo | Linha | Coupling |
|---|---|---|
| `apps/rt/src/run/resume_bootstrap.rs` | L29, L135 | `use mustard_core::store::sqlite_store::SqliteEventStore;` + `SqliteEventStore::for_project(&project).ok().and_then(\|store\| store.replay().ok())` |
| `apps/rt/src/run/spec_children_tree.rs` | L36, L179 | `use mustard_core::{..., SqliteSpecReader, ...};` + `SqliteSpecReader::for_project(project)` |
| `apps/rt/src/hooks/session_cleanup.rs` | L454-462 | `mustard_core::telemetry::TelemetryStore::for_project(cwd)` + `telemetry::writer::prune_older_than_days(store.conn(), ...)` |
| `apps/rt/src/run/otel/collector.rs` | L119 | `mustard_core::telemetry::CONSUMED_METRICS` (constante `&[&str]` de 4 entradas) |

Note: stale doc comments em `event_route.rs`, `economy_capture_baseline.rs`, `economy_reconcile.rs`,
`auto_capture_summary.rs`, `otel/mod.rs` ficam pra W8A-3 (cleanup). Esta sub-spec só migra
consumers de produção.

### Estado alvo (saída)

1. **`resume_bootstrap.rs`** — substitui `SqliteEventStore::for_project(...).replay()` por walk
   NDJSON workspace-wide (`.claude/spec/*/.events/*.ndjson` + `.claude/.session/*/.events/*.ndjson`).
   O walker já existe em `apps/rt/src/run/event_projections.rs::read_events` mas é privado;
   nesta sub-spec promovemos `read_events` + `ndjson_to_harness` para `pub(crate)` (mínimo)
   ou criamos `apps/rt/src/run/event_read.rs` com a função exportada — preferência: `pub(crate)`
   no `event_projections.rs` para evitar criar arquivo novo. O caller fica:
   ```rust
   let events = crate::run::event_projections::read_workspace_events(&project);
   let view = pipeline_state_from_events(&events, spec, Some(&spec_dir));
   ```
   Comportamento preservado: `view` continua sendo `Option<PipelineStateView>` (None quando
   spec não tem eventos), e `pipeline_state_from_events` é a mesma função usada pelas readers
   já migradas em W2A.

2. **`spec_children_tree.rs`** — substitui `SqliteSpecReader::for_project(project)` por
   construção direta via projections:
   ```rust
   let events = crate::run::event_projections::read_workspace_events(project);
   let waves: Vec<WaveChild> = mustard_core::projection::project_waves(spec, &events)
       .into_iter().map(WaveChild::from).collect();
   let acs: Vec<AcChild> = {
       let rollup = mustard_core::projection::project_quality(spec, &events);
       rollup.criteria.into_iter().map(|c| AcChild { id: c.id, label: c.label, status: c.status, last_run_at: c.last_run_at, evidence: c.fail_reason }).collect()
   };
   ```
   Drop `SqliteSpecReader` + `SpecReader` do import `use mustard_core::{...}`. Comportamento
   preservado: `WaveChild::from(WaveView)` + AC mapping exatamente como antes.

3. **`session_cleanup.rs::prune_telemetry`** — substitui inteiramente a função. O dado SQLite
   sumiu; a função agora **deleta arquivos NDJSON `.events/*.ndjson` por idade** sob
   `.claude/spec/*/.events/` + `.claude/.session/*/.events/`. Threshold mantido em
   `TELEMETRY_RETENTION_DAYS` (consulta constante atual). Implementação:
   ```rust
   fn prune_telemetry(cwd: &str) {
       let cutoff_ms = now_millis().saturating_sub(TELEMETRY_RETENTION_DAYS as u128 * 86_400_000);
       let cutoff = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(cutoff_ms as u64);
       let Ok(paths) = ClaudePaths::for_project(Path::new(cwd)) else { return; };
       // Walk both roots
       for events_root in [paths.spec_dir(), paths.session_dir()] {
           let Ok(parents) = std::fs::read_dir(&events_root) else { continue; };
           for parent in parents.flatten() {
               let events_dir = parent.path().join(".events");
               let Ok(files) = std::fs::read_dir(&events_dir) else { continue; };
               for file in files.flatten() {
                   let p = file.path();
                   if p.extension().and_then(|x| x.to_str()) != Some("ndjson") { continue; }
                   if let Ok(meta) = file.metadata() {
                       if let Ok(mtime) = meta.modified() {
                           if mtime < cutoff {
                               let _ = std::fs::remove_file(&p);
                           }
                       }
                   }
               }
           }
       }
   }
   ```
   `paths.session_dir()` precisa existir em `ClaudePaths`; se não existir, usar
   `paths.claude_dir().join(".session")` direto. Fail-open total — qualquer erro de IO
   é swallowed, igual antes. **AC-PRUNE**: teste unitário com tempdir + 2 arquivos NDJSON
   (um mtime antigo, um recente) confirma que só o antigo é removido.

4. **`otel/collector.rs`** — inline o slice de 4 strings (`CONSUMED_METRICS`) como `const` local
   no próprio `collector.rs`:
   ```rust
   /// The only `usage_totals` metric names the dashboard ever reads. Was a
   /// re-export from `mustard_core::telemetry::CONSUMED_METRICS`; moved here
   /// when the SQLite telemetry module was deleted.
   const CONSUMED_METRICS: &[&str] = &[
       "claude_code.cost.usage",
       "claude_code.session.count",
       "claude_code.active_time.total",
       "claude_code.token.usage",
   ];
   ```
   Drop `mustard_core::telemetry::CONSUMED_METRICS` referência. `otel/mod.rs` doc-comment
   stale: fica pra W8A-3.

5. **`event_projections.rs`** — expor `read_events` + `ndjson_to_harness` como `pub(crate)`
   sob o nome `read_workspace_events` (escolha didática). Rename interno para alinhar com o
   contrato compartilhado.

### Hard rule — sem stub

`prune_telemetry` precisa REALMENTE deletar arquivos NDJSON antigos. Sem `if false { ... }`,
sem early-return sem walk. Teste binário confirma.

`view` em `resume_bootstrap` precisa de fato voltar um `PipelineStateView` quando há eventos
NDJSON pra o spec. Sem `let view = None` definitivo. Teste smoke: tempdir com 1 evento
`pipeline.scope` → `view.is_some()`.

`waves` e `acs` em `spec_children_tree::build_tree` precisam de fato voltar dados quando
há eventos. Sem `Vec::new()` definitivo. Teste smoke: tempdir com 1 `pipeline.wave.complete`
→ `tree.waves.len() == 1`.

## Critérios de Aceitação

- [ ] AC-W8A1-1: `cargo build -p mustard-rt` verde. Command: `cargo build -p mustard-rt`
- [ ] AC-W8A1-2: `cargo test -p mustard-rt --no-run` compila. Command: `cargo test -p mustard-rt --no-run`
- [ ] AC-W8A1-3: `apps/rt/src/run/resume_bootstrap.rs` não importa `SqliteEventStore`. Command: `node -e "const s=require('fs').readFileSync('apps/rt/src/run/resume_bootstrap.rs','utf8'); if(/SqliteEventStore|sqlite_store/.test(s)){process.exit(1)}"`
- [ ] AC-W8A1-4: `apps/rt/src/run/spec_children_tree.rs` não importa `SqliteSpecReader`. Command: `node -e "const s=require('fs').readFileSync('apps/rt/src/run/spec_children_tree.rs','utf8'); if(/SqliteSpecReader/.test(s)){process.exit(1)}"`
- [ ] AC-W8A1-5: `apps/rt/src/hooks/session_cleanup.rs` não referencia `TelemetryStore`. Command: `node -e "const s=require('fs').readFileSync('apps/rt/src/hooks/session_cleanup.rs','utf8'); if(/TelemetryStore|telemetry::writer|mustard_core::telemetry::/.test(s)){process.exit(1)}"`
- [ ] AC-W8A1-6: `apps/rt/src/run/otel/collector.rs` não referencia `mustard_core::telemetry::CONSUMED_METRICS`. Command: `node -e "const s=require('fs').readFileSync('apps/rt/src/run/otel/collector.rs','utf8'); if(/mustard_core::telemetry::CONSUMED_METRICS/.test(s)){process.exit(1)}"`
- [ ] AC-W8A1-7: AC-PRUNE — teste unitário `prune_telemetry_removes_old_ndjson` existe e passa (compila). Command: `cargo test -p mustard-rt --test session_cleanup_prune_ndjson --no-run`
- [ ] AC-W8A1-8: AC-ANTI-STUB — `prune_telemetry` contém `std::fs::remove_file` (não é stub-noop). Command: `node -e "const s=require('fs').readFileSync('apps/rt/src/hooks/session_cleanup.rs','utf8'); if(!/remove_file/.test(s.match(/fn prune_telemetry[\\s\\S]*?\\n\\}/)[0])){process.exit(1)}"`
- [ ] AC-W8A1-9: AC-ANTI-STUB — `build_tree` em `spec_children_tree.rs` chama `project_waves` (não retorna `Vec::new` early). Command: `node -e "const s=require('fs').readFileSync('apps/rt/src/run/spec_children_tree.rs','utf8'); if(!/project_waves/.test(s)){process.exit(1)}"`
- [ ] AC-W8A1-10: invariante decrescente — count SQLite global cai de 30 (entrada). Command: `bash -c 'count=$(git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite|TelemetryStore|TelemetryReader|rusqlite::" -- "packages/**/*.rs" "apps/**/*.rs" | wc -l); test "$count" -lt 30'`

## Plano

## Arquivos

- `apps/rt/src/run/resume_bootstrap.rs` — REWRITE leitura de events
- `apps/rt/src/run/spec_children_tree.rs` — REWRITE leitura de waves+acs
- `apps/rt/src/hooks/session_cleanup.rs` — REWRITE `prune_telemetry` (deleção de NDJSON por idade)
- `apps/rt/src/run/otel/collector.rs` — inline `CONSUMED_METRICS`
- `apps/rt/src/run/event_projections.rs` — promover `read_events`/`ndjson_to_harness` para `pub(crate)` como `read_workspace_events`
- `apps/rt/tests/session_cleanup_prune_ndjson.rs` — NEW integration test para AC-PRUNE

(6 arquivos — 1 acima do cap. Justificativa: o test de AC-PRUNE precisa de arquivo próprio
em `tests/` para ser detectado pelo `cargo test --test session_cleanup_prune_ndjson`. Se o
agent julgar que dá pra fazer o teste inline em `#[cfg(test)] mod tests` dentro do próprio
`session_cleanup.rs`, pode reduzir pra 5 arquivos — mas só se mantiver o invariante AC-PRUNE
binário verificável.)

## Tarefas

1. **`event_projections.rs`**:
   - Renomeia `fn read_events` → `pub(crate) fn read_workspace_events` (mesma assinatura,
     mesma implementação).
   - Renomeia `fn ndjson_to_harness` → `pub(crate) fn ndjson_to_harness` (visibilidade
     elevada).
   - Atualiza todos os call-sites internos (8 chamadas em `read_events`).

2. **`resume_bootstrap.rs`**:
   - Remove `use mustard_core::store::sqlite_store::SqliteEventStore;`.
   - Substitui o bloco L134-138 por:
     ```rust
     let events = crate::run::event_projections::read_workspace_events(&project);
     let view: Option<PipelineStateView> =
         pipeline_state_from_events(&events, spec, Some(&spec_dir));
     ```
   - Mantém todo o resto do fluxo idêntico (`view.as_ref().and_then(...)`, fallback FS, etc.).
   - Atualiza doc-comment do módulo: remove "missing event store" → "missing events dir".

3. **`spec_children_tree.rs`**:
   - Update import: `use mustard_core::{AcStatus, Outcome, SpecState, Stage, WaveStatus, WaveView};`
     (drop `SpecReader, SqliteSpecReader`).
   - Add imports: `use mustard_core::projection::{project_waves, project_quality};`.
   - Reescreve `build_tree` substituindo o `match SqliteSpecReader::for_project(...)` pelo
     padrão `let events = read_workspace_events(project); let waves = project_waves(spec, &events)...`.
   - Atualiza doc-comment do módulo: remove menções a `SpecReader::waves` / `SpecReader::quality`,
     substitui por "projections em `mustard_core::projection::{project_waves, project_quality}`".

4. **`session_cleanup.rs`**:
   - Reescreve `fn prune_telemetry(cwd: &str)` conforme spec acima.
   - Drop `TelemetryStore::for_project` + `telemetry::writer::prune_older_than_days`.
   - Mantém threshold `TELEMETRY_RETENTION_DAYS` (já é `const` local no arquivo).
   - Mantém fail-open: nenhum erro propaga.

5. **`otel/collector.rs`**:
   - Adiciona `const CONSUMED_METRICS: &[&str] = &[...]` local com as 4 strings.
   - Substitui `mustard_core::telemetry::CONSUMED_METRICS.contains(...)` por `CONSUMED_METRICS.contains(...)`.
   - Update doc-comments removendo `mustard_core::telemetry::CONSUMED_METRICS` references
     (L32, L107) — substituir por "`CONSUMED_METRICS` (módulo-local)".

6. **`apps/rt/tests/session_cleanup_prune_ndjson.rs`** (NEW):
   - Cria tempdir com `.claude/spec/test-spec/.events/`.
   - Escreve 2 arquivos NDJSON: `old.ndjson` (mtime = now − 60 dias), `new.ndjson` (mtime = now).
   - Chama a função `prune_telemetry` (precisa expor como `pub(crate)` ou via observer).
     Alternativa: invocar via `SessionCleanup.observe(input, ctx)` com `Trigger::SessionEnd`
     e MUSTARD_DB_PATH apontando pra tempdir.
   - Assert: `old.ndjson` removido, `new.ndjson` preservado.
   - Use `std::fs::set_permissions` + `filetime` crate? Não há `filetime` no Cargo.toml; usar
     `std::os::unix::fs::FileTimesExt` em Unix ou método nativo. **Decisão**: usar
     `filetime`-like sem crate via `utimensat` Unix ou `SetFileTime` Windows direto?
     Mais simples: criar com mtime original, esperar 1ms, escrever o "new" com sleep(2ms).
     Como o cutoff é 60 dias, qualquer arquivo criado nos últimos 60s qualifica como "novo";
     o "antigo" precisa de mtime forçada. **Solução**: tornar `prune_telemetry` parametrizada
     por uma função de "now" injetável ou aceitar `cutoff_ms: i64` em uma assinatura interna
     `fn prune_telemetry_with_cutoff(cwd: &str, cutoff_ms: i64)`. O test passa um cutoff que
     considera o arquivo "novo" como expirado. Mantém SRP + testabilidade sem touch mtime.

7. **Verify**: `rtk cargo build -p mustard-rt` + `rtk cargo test -p mustard-rt --no-run` + AC grep.

## Dependências

- Consome `mustard_core::projection::{project_waves, project_quality}` (existente).
- Consome `mustard_core::events::EventReader` indiretamente via `event_projections::read_workspace_events` (existente).
- Consome `mustard_core::ClaudePaths` (existente).
- NÃO toca `store/` ou `telemetry/` em `packages/core` — esses ficam em W8A-4 (wave-29-core).
- W8A-2 (wave-27-dashboard) migra `spec_views.rs` em paralelo — pasta disjunta.

## Limites

- 6 arquivos (cap 5 + 1 NEW test, justificado).
- Modelo: opus.
- Commit message: `feat(wave-8/rt): W8A-1 — resume_bootstrap+spec_children_tree+session_cleanup NDJSON, otel constant inlined`
