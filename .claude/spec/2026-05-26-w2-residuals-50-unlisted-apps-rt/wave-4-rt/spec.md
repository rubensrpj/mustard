# W4 — run/ misc tail sweep (37 violations / 22 files)
### Stage: Close
### Outcome: Completed
### Flags: 

## Contexto

Cauda longa do sweep em `apps/rt/src/run/`. Concentra em poucos arquivos com múltiplas violações (status, unhook, sync_detect, db_maintain, transcript_watcher, scan_md_validate) + uma longa lista de arquivos com 1 violação cada. Cada arquivo individual é trivial (1 Read + 1 Edit); o desafio é o volume.

Recomendação ao agent: processar em ordem de prioridade (multi-violation primeiro), batched cargo check a cada ~7 arquivos para detectar regressão precoce.

## Arquivos (lista enumerada)

### Multi-violação (6 arquivos, 19 violações)

| # | Arquivo | Violações |
|---|---------|-----------|
| 1 | `apps/rt/src/run/scan_md_validate.rs` | 5 (linhas 188, 240, 243, 252, 330) |
| 2 | `apps/rt/src/run/status.rs` | 4 |
| 3 | `apps/rt/src/run/sync_detect.rs` | 3 |
| 4 | `apps/rt/src/run/unhook.rs` | 3 (linha 293 em test fixture — manter; outras 2 migrar) |
| 5 | `apps/rt/src/run/db_maintain.rs` | 2 (linhas 93, 105) |
| 6 | `apps/rt/src/run/transcript_watcher.rs` | 2 |

### Singleton (16 arquivos, 16 violações)

| # | Arquivo |
|---|---------|
| 7 | `apps/rt/src/run/analyze_validation.rs` (linha 227) |
| 8 | `apps/rt/src/run/bugfix_cache.rs` (linha 58) |
| 9 | `apps/rt/src/run/claude_dir_prune.rs` (linha 276) |
| 10 | `apps/rt/src/run/dependency_precheck.rs` (linha 100 — `if dir.join(".claude").exists()`; checar se precisa ClaudePaths ou se é probe genérica) |
| 11 | `apps/rt/src/run/docs_stale_check.rs` (linha 318) |
| 12 | `apps/rt/src/run/doctor_workspace_leaks.rs` (linha 79) |
| 13 | `apps/rt/src/run/env.rs` (linha 133) |
| 14 | `apps/rt/src/run/knowledge.rs` (linha 49) |
| 15 | `apps/rt/src/run/mark_checklist_item.rs` (linha 27) |
| 16 | `apps/rt/src/run/otel/store.rs` (linha 249) |
| 17 | `apps/rt/src/run/prd_build.rs` (linha 291) |
| 18 | `apps/rt/src/run/scan_structural.rs` (linha 489) |
| 19 | `apps/rt/src/run/sync_registry.rs` |
| 20 | `apps/rt/src/run/verify_emit.rs` |
| 21 | `apps/rt/src/run/worktree_gc.rs` |
| 22 | (reserva — script `count_violations.js` deve estar zerado ao fim) |

## Tarefas

- [ ] **TF4.1** — Processar 6 multi-violação primeiro. `rtk cargo check -p mustard-rt` após cada.
- [ ] **TF4.2** — Processar 16 singletons em batches de ~5 com `rtk cargo check -p mustard-rt` por batch.
- [ ] **TF4.3** — Para `dependency_precheck.rs:100`, decidir caso a caso: se `dir` é um path arbitrário sendo probado (não um workspace root), manter literal + comentário; senão migrar.
- [ ] **TF4.4** — `rtk cargo test -p mustard-rt --quiet` no final (não apenas check) para pegar quebras em ACs SQLite.

## Critérios de Aceitação

- [ ] **AC-W4.1** — `rtk node "C:/Users/ruben/.claude/jobs/3922ef93/count_violations.js"` retorna `Total: 0 violations` (combinado com W1-W3 já fechadas).
- [ ] **AC-W4.2** — `rtk cargo check -p mustard-rt` passa.
- [ ] **AC-W4.3** — `rtk cargo test -p mustard-rt --quiet` passa.

## Limites

IN: 22 arquivos listados em `apps/rt/src/run/` (excluindo os já cobertos por W2/W3).
OUT: hooks/, mcp/, scan/, tests/, qualquer arquivo de outras crates.

## Role

rt-impl
