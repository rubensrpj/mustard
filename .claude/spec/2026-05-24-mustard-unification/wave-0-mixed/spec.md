# W0 — Stop the bleeding (bloqueadores)

## Contexto

Três fixes independentes que estavam bloqueando o pipeline:

1. Clippy reclamava de `use serde_json::json;` em `apps/rt/src/hooks/session_start.rs:754` (já trazido por `use super::*;` do escopo do arquivo).
2. `docs-stale-check` achava hits em `.claude/worktrees/agent-a19b5122f2df4ee44/` (worktree órfão de uma run anterior).
3. `verify-pipeline` em projeto Rust caía no fallback `npm test` (root do monorepo tem `package.json` da pnpm workspace) e estourava o timeout de 120 s.

Esta onda já foi executada nesta sessão e está fechada. Documenta o que foi feito.

## Tarefas

- [x] **T0.1.** Remover linha `use serde_json::json;` em `apps/rt/src/hooks/session_start.rs:754` (era duplicada — `use super::*;` no mod tests já cobre).
- [x] **T0.2.** Colapsar três pares de `if X { if Y { ... } }` em `apps/rt/src/run/unhook.rs:189,196,203` para `if X && Y { ... }` (surgiu como erro novo do clippy após T0.1).
- [x] **T0.3.** Adicionar `"worktrees"` e `".worktrees"` em `IGNORE_DIRS` de `apps/rt/src/run/docs_stale_check.rs:46`.
- [x] **T0.4.** Reescrever `discover_defaults` em `apps/rt/src/run/verify_pipeline.rs:146` para preferir `Cargo.toml` antes de `package.json`. Adicionado também `go.mod` e `pyproject.toml`.
- [x] **T0.5.** Adicionar `effective_timeout(command)` em `verify_pipeline.rs` com timeouts por stack (Rust 600 s, TS 120 s, Python 180 s) overridable via env (`MUSTARD_VERIFY_TIMEOUT_RUST` etc.).
- [ ] **T0.6.** Apagar manualmente o diretório `.claude/worktrees/agent-a19b5122f2df4ee44/` (sistêmico fica em W1 via `worktree-gc`). Aguarda confirmação do user.

## Files

- `apps/rt/src/hooks/session_start.rs` (T0.1)
- `apps/rt/src/run/unhook.rs` (T0.2)
- `apps/rt/src/run/docs_stale_check.rs` (T0.3)
- `apps/rt/src/run/verify_pipeline.rs` (T0.4, T0.5)

## Critérios de Aceitação

- [x] AC-W0-1: `rtk cargo clippy -p mustard-rt -- -D warnings` sai com "No issues found". Command: `rtk cargo clippy -p mustard-rt -- -D warnings 2>&1 | grep -q "No issues found"`
- [x] AC-W0-2: `docs-stale-check` não enxerga `**/worktrees/**`. Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/run/docs_stale_check.rs','utf8');if(!/\"worktrees\"/.test(t)||!/\".worktrees\"/.test(t))process.exit(1)"`
- [x] AC-W0-3: `verify_pipeline` prefere `Cargo.toml` antes de `package.json`. Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/run/verify_pipeline.rs','utf8');const m=t.match(/discover_defaults[\\s\\S]*?fn /);if(!m||!/Cargo\\.toml[\\s\\S]*?package\\.json/.test(m[0]))process.exit(1)"`
- [x] AC-W0-4: Helper `effective_timeout` existe e respeita env `MUSTARD_VERIFY_TIMEOUT_RUST`. Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/run/verify_pipeline.rs','utf8');if(!/fn effective_timeout/.test(t)||!/MUSTARD_VERIFY_TIMEOUT_RUST/.test(t))process.exit(1)"`
- [x] AC-W0-5: `rtk cargo test -p mustard-rt verify_pipeline docs_stale_check` passa. Command: `rtk cargo test -p mustard-rt verify_pipeline 2>&1 | grep -q "0 failed" && rtk cargo test -p mustard-rt docs_stale_check 2>&1 | grep -q "0 failed"`
