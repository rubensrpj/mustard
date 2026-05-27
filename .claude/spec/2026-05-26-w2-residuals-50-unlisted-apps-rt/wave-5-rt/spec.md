# W5 — Tests migration + one-shot cleanup + doctor verify
### Stage: Close
### Outcome: Completed
### Flags: 

## Contexto

Fechar AC-W2.9 da parent spec (testes de integração vazando `apps/rt/.claude/`) + cleanup one-shot dos 7 legados em `.claude/` raiz + verificação final via `doctor`.

`apps/rt/tests/common/mod.rs` já exporta `test_workspace()`; só falta wiring nos testes existentes que ainda usam `env::project_dir()` ou `current_dir()` cru.

## Tarefas

- [ ] **TF5.1** — Listar todos os arquivos em `apps/rt/tests/**/*.rs` que chamam `env::project_dir()` ou `std::env::current_dir()`. Command: `rtk grep -n "env::project_dir\|env::current_dir" apps/rt/tests/`.
- [ ] **TF5.2** — Para cada teste identificado: substituir o setup por `let ws = common::test_workspace(); let root = ws.root();` (já existe em `apps/rt/tests/common/mod.rs`). Não tocar lógica de asserts.
- [ ] **TF5.3** — Escrever script PowerShell em `apps/rt/scripts/cleanup-legacy-claude.ps1` que apaga:
  - `.claude/.qa-reports/`
  - `.claude/.pipeline-states/`
  - `.claude/.economy-baselines.json`
  - `.claude/.scan-dispatch.json`
  - `.claude/.detect-cache.json`
  - `.claude/.knowledge-seen.json`
  - `.claude/.memory-seen.json`
  Com `-ErrorAction SilentlyContinue` (idempotente).
- [ ] **TF5.4** — Rodar o script uma vez.
- [ ] **TF5.5** — Executar `rtk mustard-rt run doctor --check claude-paths --format json` e verificar `overall == "ok"`.
- [ ] **TF5.6** — Rodar `rtk cargo test -p mustard-rt` 10 vezes (script PowerShell já no AC-G2 da umbrella).

## Critérios de Aceitação

- [ ] **AC-W5.1** — Zero `env::project_dir()` ou `env::current_dir()` em `apps/rt/tests/**` fora de `common/mod.rs`. Command: `rtk grep -n "env::project_dir\|env::current_dir" apps/rt/tests/ | rtk grep -v "common/mod.rs"` deve ser vazio.
- [ ] **AC-W5.2 (= AC-G2 umbrella)** — 10× `rtk cargo test -p mustard-rt` consecutivos não criam `apps/rt/.claude/`. Command: `rtk powershell -Command "Remove-Item -Recurse -Force apps/rt/.claude -ErrorAction SilentlyContinue; 1..10 | ForEach-Object { cargo test -p mustard-rt --quiet }; if (Test-Path apps/rt/.claude) { exit 1 }"`.
- [ ] **AC-W5.3 (= AC-G3 umbrella)** — Raiz `.claude/` sem os 7 legados volatile. Command: `rtk node -e "const fs=require('fs');for(const p of ['.qa-reports','.pipeline-states','.economy-baselines.json','.scan-dispatch.json','.detect-cache.json','.knowledge-seen.json','.memory-seen.json']){if(fs.existsSync('.claude/'+p))process.exit(1)}"`.
- [ ] **AC-W5.4** — `rtk mustard-rt run doctor --check claude-paths --format json` retorna `overall == "ok"`.

## Limites

IN: `apps/rt/tests/**/*.rs`, `apps/rt/scripts/cleanup-legacy-claude.ps1` (novo), execução one-shot do script.
OUT: `apps/rt/src/**` (já coberto por W1-W4), outros subprojetos.

## Role

rt-impl
