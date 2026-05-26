# W2 residuals: 89 unlisted `.join(".claude")` callsites in apps/rt + integration-test fixture wiring + legacy artifact cleanup

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Lang: pt-BR
### Checkpoint: 2026-05-26T12:00:00.000Z
### Parent: 2026-05-26-claude-paths-single-source

## Contexto

Resíduo de [[2026-05-26-claude-paths-single-source]] W2. A wave-plan declarou "33 arquivos" mas a contagem real (grep agnóstico em `apps/rt/src/`, fora de testes e de `ClaudePaths`) bate em **89 callsites espalhados por 52 arquivos** — escopo grande demais para um único agent rt-impl (a primeira tentativa, em 2026-05-26, estourou contexto após 246 tool calls). Refatorado em ondas mecânicas com lista de arquivos enumerada por wave.

AC-W2.9 (`cargo test -p mustard-rt` não vaza `apps/rt/.claude/`) também ficou aberto porque os testes de integração chamam `env::project_dir()` direto, em vez do helper `test_workspace()` que já existe em `apps/rt/tests/common/mod.rs`.

ACs W2.6/W2.7 dependem de cleanup one-shot dos arquivos legados em `.claude/` raiz que os writers novos deixaram de tocar.

## Estrutura

Wave plan: [[wave-plan]]

| Wave | Role | Arquivos | Violações |
|------|------|----------|-----------|
| 1 — hooks + mcp | rt | 9 | 12 |
| 2 — run/ emit/memory/amend | rt | 8 | 16 |
| 3 — run/ skills + spec helpers + scan/ | rt | 13 | 24 |
| 4 — run/ misc tail + scan_md_validate | rt | 22 | 37 |
| 5 — tests migration + cleanup script + doctor verify | rt | varies | — |

## Critérios de Aceitação (globais)

- [ ] **AC-G1.** Zero `.join(".claude")` em `apps/rt/src/` fora de `ClaudePaths` callers, tests gated por `#[cfg(test)]`, e do próprio `claude_paths.rs`. O script reconhece `// ClaudePaths-exempt` inline e devolve PASS quando zerar. Command: `rtk node apps/rt/scripts/ac_check_claude_join.js`
- [ ] **AC-G2.** Rodar `cargo test -p mustard-rt` não cria `apps/rt/.claude/` (test exit code é ignorado — falhas de teste pré-existentes em `mcp::*`, `agent_prompt_render::*`, `memory::*` não bloqueiam o leak check, que mede APENAS a presença do diretório). Command: `rtk powershell -Command "Remove-Item -Recurse -Force apps/rt/.claude -ErrorAction SilentlyContinue; cargo test -p mustard-rt --quiet 2>$null | Out-Null; if (Test-Path apps/rt/.claude) { exit 1 } else { exit 0 }"`
- [ ] **AC-G3.** Raiz `.claude/` não contém legados volatile. Command: `rtk node -e "const fs=require('fs');for(const p of ['.qa-reports','.pipeline-states','.economy-baselines.json','.scan-dispatch.json','.detect-cache.json','.knowledge-seen.json','.memory-seen.json']){if(fs.existsSync('.claude/'+p))process.exit(1)}"`

ACs específicos por wave ficam dentro de cada `wave-N-rt/spec.md`.

## Limites

IN: `apps/rt/src/{hooks,run,mcp,scan}/**`, `apps/rt/tests/**`, script one-shot PowerShell em raiz.
OUT: dashboard, cli, core, packages (não tocar — `ClaudePaths` em `packages/core` já está pronto).

## Dependências

- **Parent**: [[2026-05-26-claude-paths-single-source]] (Closed). Esta sub-spec não bloqueia ninguém.
- **Pode rodar em paralelo a**: [[2026-05-26-template-agnostic-audit]] (escopos disjuntos).

## Histórico

- 2026-05-26 — primeira tentativa flat (agent rt-impl 22min/246 tool calls) sweepou parcialmente, contexto estourou. Reescrita em wave-plan, ACs globais preservados.
