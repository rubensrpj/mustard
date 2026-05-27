# W1 — hooks + mcp sweep (12 violations / 9 files)
### Stage: Close
### Outcome: Completed
### Flags: 

## Contexto

Sweep mecânico de `.join(".claude")` → método `ClaudePaths` em `apps/rt/src/hooks/` e `apps/rt/src/mcp/`. Padrão de substituição:

```rust
// ANTES
let dir = cwd.join(".claude").join(".harness");
// DEPOIS
let paths = ClaudePaths::for_project(&cwd);
let dir = paths.harness();
```

Os 7 arquivos de `hooks/` + 2 de `mcp/` são independentes; não há reentrância entre eles. Ler `packages/core/src/claude_paths.rs` UMA vez no início para mapear métodos disponíveis (`harness()`, `spec()`, `skills()`, `entity_registry()`, etc.).

## Arquivos (lista enumerada)

| # | Arquivo | Violações |
|---|---------|-----------|
| 1 | `apps/rt/src/hooks/auto_capture_summary.rs` | 1 (linha 132) |
| 2 | `apps/rt/src/hooks/knowledge.rs` | 1 (linha 899) |
| 3 | `apps/rt/src/hooks/pre_compact.rs` | 1 (linha 213 — comentário; remover ou atualizar referência) |
| 4 | `apps/rt/src/hooks/session_cleanup.rs` | 1 (linha 340 — note: `home.join(".claude")`, fora de project scope; verificar se precisa) |
| 5 | `apps/rt/src/hooks/stop.rs` | 2 (linhas 90, 130) |
| 6 | `apps/rt/src/hooks/stop_observer.rs` | 3 (linhas 53, 172, 260) |
| 7 | `apps/rt/src/hooks/tracker.rs` | 1 (linha 625) |
| 8 | `apps/rt/src/mcp/mod.rs` | 1 (linha 460) |
| 9 | `apps/rt/src/mcp/tests.rs` | 1 (linha 25 — verificar se está em `#[cfg(test)]` mod; se sim, deixar como está e justificar) |

## Tarefas

- [ ] **TF1.1** — Ler `packages/core/src/claude_paths.rs` uma vez. Anotar métodos disponíveis.
- [ ] **TF1.2** — Para cada arquivo em ordem, abrir, substituir, salvar.
- [ ] **TF1.3** — Decidir sobre `hooks/session_cleanup.rs:340` (`home.join(".claude")`): se for path do `~/.claude` global do user, NÃO migrar (ClaudePaths é per-project). Documentar inline com comentário curto.
- [ ] **TF1.4** — Decidir sobre `hooks/pre_compact.rs:213`: se for comentário legacy, atualizar ou remover.
- [ ] **TF1.5** — `rtk cargo check -p mustard-rt` no final. Se falhar, reverter o último arquivo problemático e reportar.

## Critérios de Aceitação

- [ ] **AC-W1.1** — Zero `.join(".claude")` em `apps/rt/src/hooks/**` e `apps/rt/src/mcp/**` fora de `ClaudePaths` callers, tests gated, e justificativas explícitas (home dir, comentário legacy). Command: `rtk node apps/rt/scripts/ac_check_claude_join.js 2>&1 | rtk grep "hooks/\|mcp/"` deve retornar zero linhas de FAIL.
- [ ] **AC-W1.2** — `rtk cargo check -p mustard-rt` passa. Command: `rtk cargo check -p mustard-rt --quiet`.

## Limites

IN: `apps/rt/src/hooks/**`, `apps/rt/src/mcp/**`.
OUT: `apps/rt/src/run/**`, `apps/rt/tests/**`, qualquer arquivo fora de `apps/rt`.

## Role

rt-impl
