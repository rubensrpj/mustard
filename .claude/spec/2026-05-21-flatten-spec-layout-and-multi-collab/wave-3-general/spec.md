# Wave 3 — Dashboard src-tauri: emit-only + flat resolve

### Parent: [[2026-05-21-flatten-spec-layout-and-multi-collab]]
### Status: completed
### Phase: CLOSE
### Lang: pt

## Resumo

`spec_action::Close` e `Reopen` deixam de mover pastas — passam a emitir o evento `pipeline.status` correspondente e atualizar o cabeçalho (já garantido pelo lado da Wave 2 via `emit_pipeline.rs`). `resolve_spec_dir` (em `spec_views.rs`) deixa de iterar buckets e resolve diretamente em `spec/{name}/`. O carregador de `spec_view` continua o mesmo — só precisa estar alinhado com o fallback da Wave 1.

## Contexto

`apps/dashboard/src-tauri/src/spec_views.rs:402-422` ainda chama `std::fs::rename(active, completed)` no `Close`. Esse é o último ponto do produto que move dir pra mudar status. Wave 3 corta isso. `Reopen` faz o caminho inverso (completed→active) — também vira só emit. `Remove` segue lá; só passa a buscar em `spec/{name}/` direto.

## Arquivos

```
apps/dashboard/src-tauri/src/spec_views.rs       — Close/Reopen emit-only + resolve_spec_dir flat
apps/dashboard/src-tauri/tests/*                  — ajustar testes que verificavam fs::rename
```

## Tarefas

- [x] `spec_action::Close`: substituir `fs::rename` por uma chamada a `mustard-rt run emit-pipeline --kind pipeline.status --spec X --payload '{"to":"completed"}'` (ou o equivalente direto via `SqliteEventStore::append`). O sync de header acontece dentro do `emit_pipeline.rs` (Wave 2).
- [x] `spec_action::Reopen`: idem, emit `pipeline.status` com `to: "implementing"` (ou "planning" se nenhum evento prévio existia). Sem mv.
- [x] `spec_action::Remove`: deletar `spec/{name}/` (única pasta possível). Sem busca multi-bucket.
- [x] `resolve_spec_dir` (linhas 1041-1073): remover loop `for sub in ["active", "completed", "cancelled"]`. Resolve direto `repo_path / .claude / spec / name`. Sub-spec nested (wave-N dentro de parent): manter o loop apenas dentro de `spec/{parent}/{name}`.
- [x] Testes: atualizar testes que assertam `completed_spec_dir(...).exists()` após Close.

## Acceptance Criteria

- [x] AC-W3-1: Build do src-tauri compila — Command: `cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml`
- [x] AC-W3-2: Testes do src-tauri passam — Command: `cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml`
- [x] AC-W3-3: Não há mais `fs::rename` em `spec_views.rs` no path de Close/Reopen — Command: `node -e "const f=require('fs').readFileSync('apps/dashboard/src-tauri/src/spec_views.rs','utf8');const hits=f.split(/\n/).filter(l=>l.includes('fs::rename'));process.exit(hits.length===0?0:(console.error(hits),1))"`

## Limites

- `apps/dashboard/src-tauri/src/spec_views.rs`
- `apps/dashboard/src-tauri/tests/*`

## Network

- Parent: [[2026-05-21-flatten-spec-layout-and-multi-collab]]
- Depende de: [[wave-1-library]]
- Bloqueia: [[wave-4-general]], [[wave-5-general]]
