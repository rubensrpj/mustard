# Migrate dashboard spec_views.rs from SqliteSpecReader to direct projections (W8A-2)

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: dashboard
### Checkpoint: 2026-05-27T22:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec W8A-2 da [[2026-05-26-no-sqlite-git-source-of-truth]]. Última consumer SQLite no
dashboard pós-W6: `apps/dashboard/src-tauri/src/spec_views.rs` ainda tem 5 call-sites de
`mustard_core::SqliteSpecReader::for_project(repo_path)`. W6 já migrou `db.rs`, `economy.rs`,
`telemetry.rs`, `telemetry_agg.rs`, mas o cluster `spec_views_v2`/`spec_waves_v2`/
`spec_quality_v2`/`spec_timeline_v2`/`workspace_summary_v2` ficou pendente porque dependia
do `SpecReader` trait — que vai morrer em W8A-4.

### Estado atual (entrada)

5 funções em `spec_views.rs` usam o reader trait:

| Função | Linha | Reader call |
|---|---|---|
| `spec_card_v2` | L536-552 | `reader.spec_view(spec)` + `reader.children_of(spec)` |
| `spec_waves_v2` | L556-562 | `reader.waves(spec)` |
| `spec_quality_v2` | L565-571 | `reader.quality(spec)` |
| `spec_timeline_v2` | L576-584 | `reader.timeline(spec, TimeWindow::All)` |
| `workspace_summary_v2` | L761-789 | `reader.workspace_summary()` |

### Estado alvo (saída)

Cada função usa as projections puras de `mustard_core::projection::*` diretamente sobre um
`Vec<HarnessEvent>` lido via filesystem walk. Padrão:

```rust
fn read_workspace_events(repo_path: &str) -> Vec<HarnessEvent> { ... }
// (helper local privado idêntico ao read_workspace_events de event_projections.rs;
//  copiado aqui porque o dashboard não pode importar de apps/rt — só de mustard-core)
```

**Decisão de design (importante)**: ao invés de duplicar o helper, esta sub-spec **adiciona
`pub fn read_workspace_events(project_root: &Path) -> Vec<HarnessEvent>` ao crate `mustard-core`**
em `packages/core/src/projection/mod.rs` (pure-IO walker — fail-open, escopo crate). Tanto o
dashboard quanto o rt consumem o mesmo helper. Em W8A-1 (wave-26) o rt promoveu `read_events`
de `event_projections.rs` para `pub(crate)`; agora essa promoção é substituída por
`mustard_core::projection::read_workspace_events`, e o rt deleta sua cópia para evitar
duplicação. **Ordem**: W8A-1 commitar primeiro com a versão `pub(crate)`; esta sub-spec
move a função pro core e atualiza rt para consumir do core.

Após isso, cada `*_v2` no dashboard fica:

```rust
pub fn spec_card_v2(repo_path: &str, spec: &str) -> Result<Option<SpecCard>, String> {
    let project = std::path::PathBuf::from(repo_path);
    let events = mustard_core::projection::read_workspace_events(&project);
    let spec_md = project.join(".claude/spec").join(spec).join("spec.md");
    let view = mustard_core::projection::project_spec_view_with_header(
        spec, &events, Some(&spec_md), None
    );
    if view.is_empty() { return Ok(None); }
    // children_count: re-fold via spec.link events filtered to parent==spec
    let children_count: u32 = events.iter()
        .filter(|e| e.event == "spec.link")
        .filter_map(|e| e.payload.get("parent").and_then(|p| p.as_str()))
        .filter(|p| *p == spec)
        .count() as u32;
    Ok(Some(spec_card_from_view(&view, children_count)))
}
```

Helper auxiliar `view.is_empty()` precisa existir em `SpecView` (verifica `started_at.is_none()
&& last_event_at.is_none()`). Se não existir, adicionar como `pub fn is_empty(&self) -> bool`
em `packages/core/src/model/view/spec.rs` (1 método trivial).

### Hard rule — sem stub

Nenhuma das 5 funções pode retornar `Ok(None)` / `Ok(Vec::new())` / default quando há eventos
NDJSON correspondentes para o spec. Verify visual: cada função tem um `.iter().filter(...)`
ou `project_*(spec, &events)` no corpo.

## Critérios de Aceitação

- [x] AC-W8A2-1: `cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml` verde. Command: `cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml`
- [x] AC-W8A2-2: `cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml --no-run` compila. Command: `cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml --no-run`
- [x] AC-W8A2-3: `spec_views.rs` não importa `SqliteSpecReader` nem `SpecReader`. Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/spec_views.rs','utf8'); if(/SqliteSpecReader|use mustard_core::SpecReader/.test(s)){process.exit(1)}"`
- [x] AC-W8A2-4: `mustard_core::projection::read_workspace_events` é `pub fn`. Command: `node -e "const s=require('fs').readFileSync('packages/core/src/projection/mod.rs','utf8'); if(!/pub fn read_workspace_events/.test(s)){process.exit(1)}"`
- [x] AC-W8A2-5: AC-ANTI-STUB — `spec_card_v2` chama `project_spec_view_with_header`. Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/spec_views.rs','utf8'); const m=s.match(/fn spec_card_v2[\\s\\S]*?\\n\\}/); if(!m || !/project_spec_view_with_header/.test(m[0])){process.exit(1)}"`
- [x] AC-W8A2-6: AC-ANTI-STUB — `spec_waves_v2` chama `project_waves`. Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/spec_views.rs','utf8'); const m=s.match(/fn spec_waves_v2[\\s\\S]*?\\n\\}/); if(!m || !/project_waves/.test(m[0])){process.exit(1)}"`
- [x] AC-W8A2-7: AC-ANTI-STUB — `workspace_summary_v2` chama `project_workspace`. Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/spec_views.rs','utf8'); const m=s.match(/fn workspace_summary_v2[\\s\\S]*?\\n\\}/); if(!m || !/project_workspace/.test(m[0])){process.exit(1)}"`
- [x] AC-W8A2-8: invariante decrescente — count cai. Command: `bash -c 'count=$(git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite|TelemetryStore|TelemetryReader|rusqlite::" -- "packages/**/*.rs" "apps/**/*.rs" | wc -l); test "$count" -lt 30'`

## Plano

## Arquivos

- `packages/core/src/projection/mod.rs` — ADD `pub fn read_workspace_events(project: &Path) -> Vec<HarnessEvent>` + helper `pub fn ndjson_to_harness(e: Event) -> HarnessEvent` (move-do-rt + público)
- `apps/dashboard/src-tauri/src/spec_views.rs` — REWRITE 5 funções (`*_v2`), drop `SqliteSpecReader` imports
- `apps/rt/src/run/event_projections.rs` — drop `read_workspace_events`/`ndjson_to_harness` locais (delegar pro core)
- `apps/rt/src/run/resume_bootstrap.rs` — atualizar call-site (de `crate::run::event_projections::read_workspace_events` → `mustard_core::projection::read_workspace_events`)
- `apps/rt/src/run/spec_children_tree.rs` — atualizar call-site igual ao item acima
- `packages/core/src/model/view/spec.rs` — ADD `pub fn is_empty(&self) -> bool` em SpecView (1 método)

(6 arquivos — 1 acima do cap. Justificativa: criar o helper no core obriga atualizar os 2
callers do rt no mesmo commit, senão build quebra. Alternativa seria duplicar `read_workspace_events`
no dashboard mas isso viola DRY que é a regra de projeto em [[feedback_rust_solid_reuse_global]].)

## Tarefas

1. **`packages/core/src/projection/mod.rs`** — copiar `read_events` + `ndjson_to_harness`
   de `apps/rt/src/run/event_projections.rs` (versão W8A-1 que já é `pub(crate)`) para o core
   como `pub fn read_workspace_events` + `pub fn ndjson_to_harness`. Adicionar
   `use crate::model::event::HarnessEvent;` + `use crate::ClaudePaths;` + `use crate::events::{Event, EventReader};`.

2. **`apps/rt/src/run/event_projections.rs`** — remover `fn read_workspace_events`,
   `fn ndjson_to_harness`. Atualizar todos call-sites internos para
   `mustard_core::projection::read_workspace_events(cwd)` + `mustard_core::projection::ndjson_to_harness(e)`.

3. **`apps/rt/src/run/resume_bootstrap.rs`** — substituir `crate::run::event_projections::read_workspace_events(&project)`
   por `mustard_core::projection::read_workspace_events(&project)`.

4. **`apps/rt/src/run/spec_children_tree.rs`** — substituir call-site igual.

5. **`packages/core/src/model/view/spec.rs`** — adicionar:
   ```rust
   impl SpecView {
       /// True when the view holds no event evidence (both timestamps absent).
       #[must_use]
       pub fn is_empty(&self) -> bool {
           self.started_at.is_none() && self.last_event_at.is_none()
       }
   }
   ```

6. **`apps/dashboard/src-tauri/src/spec_views.rs`** — REWRITE 5 funções:
   - `spec_card_v2`: `read_workspace_events` + `project_spec_view_with_header` + count `spec.link` events.
   - `spec_waves_v2`: `read_workspace_events` + `project_waves(spec, &events)`.
   - `spec_quality_v2`: `read_workspace_events` + `project_quality(spec, &events)`.
   - `spec_timeline_v2`: `read_workspace_events` + `project_timeline(spec, &events, TimeWindow::All)`.
   - `workspace_summary_v2`: `read_workspace_events` + `project_workspace(&events, now_ms)` (precisa de `now_ms` parameter — usar `std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)`).
   - Drop `use mustard_core::SpecReader;` em todos os 5 escopos locais.
   - Preserva o override de `top_files_today` via `db::with_db` (esse já é filesystem-based pós-W6).
   - Preserva o filtro `is_terminal_status` no `spec_tracks`.

7. **Verify**: `rtk cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml` + `rtk cargo build -p mustard-rt` + `rtk cargo build -p mustard-core` + AC grep.

## Dependências

- **Depende de W8A-1 (wave-26-rt) commitada**: precisa que `read_workspace_events` esteja
  acessível como `pub(crate)` em `apps/rt/src/run/event_projections.rs` antes de mover ao core.
- Consome `mustard_core::projection::{project_spec_view_with_header, project_waves, project_quality, project_timeline, project_workspace}` (existentes).
- Consome `mustard_core::TimeWindow` (existente).
- NÃO toca `store/`, `telemetry/`, `reader/` em core — esses ficam em W8A-4 (wave-29-core).

## Limites

- 6 arquivos (cap 5 + 1 cross-crate alinhamento, justificado).
- Modelo: opus.
- Commit message: `feat(wave-8/dashboard): W8A-2 — spec_views.rs via core::projection helpers, lift read_workspace_events to core`
