# Tactical-fix — dashboard src-tauri lib.rs + watcher.rs flatten

### Parent: [[2026-05-21-flatten-spec-layout-and-multi-collab]]
### Status: completed
### Phase: CLOSE
### Lang: pt
### Checkpoint: 2026-05-21

## Resumo

A wave-3 do parent flattenou apenas `spec_views.rs`. Os Tauri commands `dashboard_spec_complete`/`dashboard_spec_cancel` em `lib.rs` (~lines 895-928) ainda chamam `move_spec_dir` que usa `fs::rename` entre buckets, e `watcher.rs:38` ainda filtra por `spec/active`. Esta fix mata os dois — Close/Cancel passam a emit-only via SqliteEventStore (mesmo padrão de spec_views.rs), e watcher.rs passa a observar `.claude/spec/` flat.

## Arquivos

```
apps/dashboard/src-tauri/src/lib.rs       — remover move_spec_dir; Close/Cancel emit-only
apps/dashboard/src-tauri/src/watcher.rs   — filtrar spec/{name}/ flat
```

## Tarefas

- [x] `lib.rs`: remover função `move_spec_dir`. `dashboard_spec_complete` e `dashboard_spec_cancel` passam a emitir `pipeline.status` (`completed` / `cancelled`) via `SqliteEventStore::append` + duplicar `sync_spec_status_header` inline (~20 LOC).
- [x] `watcher.rs`: substituir o pattern-match `spec/active` por reconhecer qualquer `spec/{name}/spec.md` (path depth + extension match).
- [x] Verificar `spec_views.rs:1-hit` — confirmado como texto inerte em comentário de doc; não editado.

## Acceptance Criteria

- [x] AC-TF-B-1: `rg -n 'spec/(active|completed|superseded)|move_spec_dir|fs::rename' apps/dashboard/src-tauri/src` retorna vazio.
- [x] AC-TF-B-2: `cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml` passa.
- [x] AC-TF-B-3: `cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml` passa.

## Limites

- `apps/dashboard/src-tauri/src/lib.rs`
- `apps/dashboard/src-tauri/src/watcher.rs`
- `apps/dashboard/src-tauri/src/spec_views.rs` (apenas o hit residual, surgical)

OUT: frontend TS/TSX; outros src-tauri modules.
