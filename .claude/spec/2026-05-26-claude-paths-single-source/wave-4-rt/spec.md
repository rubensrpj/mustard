# W4 — Limpeza retroativa one-shot dos rastros do bug

### Stage: Execute
### Outcome: Active
### Flags:
### Checkpoint: 2026-05-26T00:00:00Z

## Contexto

W1-W3 corrigem o mecanismo (walker + struct + doctor). Falta uma operação **one-shot, idempotente, escrita por humano** para apagar os rastros que o bug TS→Rust deixou nos diretórios do repo Mustard e em projetos-alvo conhecidos (`c:\Atiz\sialia`).

Esta wave é **roteiro de limpeza + verificação**, não código novo. O `mustard-rt` já ganhou `doctor --check workspace-leaks` e `--check i1` em W3. Aqui só executa, verifica que ficou limpo, e congela o resultado via AC.

Evidências dos rastros (coletadas no diagnóstico):

- `c:\Atiz\mustard\apps\cli\.claude\.harness\mustard.db` — banco órfão (contradiz [[feedback_no_attach_sqlite]])
- `c:\Atiz\mustard\apps\rt\.claude\.harness\mustard.db` — idem
- `c:\Atiz\mustard\apps\dashboard\.claude\.harness\mustard.db` — idem
- `c:\Atiz\mustard\apps\rt\.claude\.pipeline-states\{2026-05-25-fix-one-thing,2026-05-25-fix-null-guard,2026-05-26-fix-one-thing,2026-05-26-fix-null-guard,epic-1}.json` — fixtures de teste vazadas
- `c:\Atiz\mustard\apps\rt\.claude\spec\2026-05-25-fix-{one-thing,null-guard}\.events\*.ndjson` — centenas de eventos NDJSON de specs que nunca existiram
- `c:\Atiz\mustard\apps\rt\.claude\.agent-state\main-context.counter.json`
- `c:\Atiz\sialia\.claude\.claude\.metrics\{bash-native-redirect,rtk-rewrite}.jsonl` — violação I1 ativa
- `c:\Atiz\mustard\.claude\.claude\` (se existir) — violação I1 no próprio repo

Subprojeto `c:\Atiz\sialia\backend\Sialia.Backend\.claude\` contém **dois tipos** misturados: output legítimo do scan (commands, skills, agents, services.json — fica) e estado vivo vazado (`.harness/`, `.agent-state/`, `.agent-memory/`, `.metrics/`, `memory/`, `plans/` — sai). A limpeza desse subprojeto é **seletiva**, não recursiva total.

## Tarefas

- [ ] **T4.1** — Rodar `mustard-rt run doctor --check workspace-leaks --format json` em `c:\Atiz\mustard` **antes** de qualquer delete. Salvar output em `.claude/spec/2026-05-26-claude-paths-single-source/wave-4-rt/leaks-before.json` para auditoria.

- [ ] **T4.2** — Rodar `mustard-rt run doctor --check i1 --format json` em `c:\Atiz\mustard`. Salvar output em `wave-4-rt/i1-before-mustard.json`.

- [ ] **T4.3** — Limpeza no repo Mustard (PowerShell, executar uma vez):

  ```powershell
  Remove-Item -Recurse -Force c:\Atiz\mustard\apps\cli\.claude -ErrorAction SilentlyContinue
  Remove-Item -Recurse -Force c:\Atiz\mustard\apps\rt\.claude -ErrorAction SilentlyContinue
  Remove-Item -Recurse -Force c:\Atiz\mustard\apps\dashboard\.claude -ErrorAction SilentlyContinue
  Remove-Item -Recurse -Force c:\Atiz\mustard\.claude\.claude -ErrorAction SilentlyContinue
  ```

- [ ] **T4.4** — *(removida — sialia fora de escopo)*

- [ ] **T4.5** — *(removida — sialia fora de escopo)*

- [ ] **T4.6** — Re-rodar `doctor --check workspace-leaks` e `doctor --check i1` no repo Mustard. Salvar outputs em `wave-4-rt/{leaks,i1}-after-mustard.json`. AC compara antes/depois.

## Critérios de Aceitação

- [ ] **AC-W4.1** — `c:\Atiz\mustard\apps\{cli,rt,dashboard}\.claude\` não existem. Command: `rtk powershell -Command "foreach ($p in @('c:\Atiz\mustard\apps\cli\.claude','c:\Atiz\mustard\apps\rt\.claude','c:\Atiz\mustard\apps\dashboard\.claude')) { if (Test-Path $p) { exit 1 } }"`
- [ ] **AC-W4.2** — `c:\Atiz\mustard\.claude\.claude\` não existe. Command: `rtk powershell -Command "if (Test-Path c:\Atiz\mustard\.claude\.claude) { exit 1 }"`
- [ ] **AC-W4.3** — *(removida — sialia fora de escopo)*
- [ ] **AC-W4.4** — *(removida — sialia fora de escopo)*
- [ ] **AC-W4.5** — `doctor --check workspace-leaks` em `c:\Atiz\mustard` retorna `{ok: true, divergences: []}`. Command: `rtk mustard-rt run doctor --check workspace-leaks --format json | rtk node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(!j.ok||(j.divergences||[]).length>0)process.exit(1)})"`
- [ ] **AC-W4.6** — `doctor --check i1` em `c:\Atiz\mustard` retorna `{ok: true, violations: []}`. Command: `rtk mustard-rt run doctor --check i1 --format json | rtk node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(!j.ok||(j.violations||[]).length>0)process.exit(1)})"`
- [ ] **AC-W4.7** — Outputs antes/depois (só Mustard) arquivados em `wave-4-rt/`. Command: `rtk node -e "const fs=require('fs');for(const f of ['leaks-before.json','leaks-after-mustard.json','i1-before-mustard.json','i1-after-mustard.json']){if(!fs.existsSync('.claude/spec/2026-05-26-claude-paths-single-source/wave-4-rt/'+f))process.exit(1)}"`

## Limites

`.claude/spec/2026-05-26-claude-paths-single-source/wave-4-rt/*.json` (outputs de auditoria). Operação no filesystem dos repos Mustard e sialia (delete only, sem moves). Nenhum código fonte tocado.

OUT: código `apps/`, `packages/`, templates. Limpeza só roda **depois** de W1-W3 (mecanismo precisa estar correto antes de cleanup, senão regride).

## Role

rt (operação manual orquestrada por `mustard-rt run doctor`)
