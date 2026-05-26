# Deep Refactor Followups — 5 fixes pequenos descobertos no REVIEW/QA

### Stage: Plan
### Outcome: Active
### Flags: 
### Parent: 2026-05-25-mustard-deep-refactor

<!-- PRD -->

## Contexto

O deep-refactor (W0-W12) fechou via REVIEW + QA com `overall: pass`. Durante esse fechamento surgiram 5 itens DEFERRED que não bloqueavam o pipeline mas devem ser resolvidos numa sub-spec tática linkada — preserva pureza SDD do parent e mantém rastreabilidade.

Origens:
- **F1 / F2** vieram do REVIEW do `apps/rt/` (reviewer flagou como LOW UX/flaky).
- **F3** veio do REVIEW do `apps/dashboard/` (i18n inconsistente na seção nova).
- **F4** veio do W11 (tabela `economy_savings` populada mas sem números reais).
- **F5** veio do REVIEW do `apps/cli/` (descoberta opcional para usuários).

Nenhum item é load-bearing para o produto. Todos são polimento (UX, lint, instrumentação).

## Usuários

- **Rubens** (operador único): UX de busca de memória, descoberta de extras no init, leitura honesta do `/economia`.
- **Quem mantém o código**: testes de integração estáveis no CI Windows.

## Métrica

- 5/5 ACs PASS, sem regredir os 6 ACs que já passam do deep-refactor.
- `cargo test -p mustard-rt --tests` sem flakes em 3 runs consecutivos no Windows.
- `economy_savings` com ≥1 row por wave W0-W12 com `savings_tokens > 0` (numero real, não placeholder 0).

## Não-Objetivos

- Reabrir qualquer wave do deep-refactor (W0-W12 estão closed).
- Adicionar novos subcomandos ou hooks.
- Migrar dados: spec é puramente patches in-place.
- Re-instrumentar pipelines passadas — a captura de baselines começa daqui pra frente.

## Critérios de Aceitação

- [ ] **AC-F1.** Memory FTS5 lida graciosamente com hifens em queries. `memory write` + `memory search --query review-smoke` retorna ≥1 row (hoje retorna 0 porque FTS5 parseia `-` como `NOT`).
  Command: `rtk node -e "const {execSync}=require('child_process');execSync('rtk mustard-rt run memory write --verify --spec test-hyphen --summary \"review-smoke summary\" --details x');const r=execSync('rtk mustard-rt run memory search --query review-smoke').toString();const j=JSON.parse(r);if(j.length<1)process.exit(1)"`

- [ ] **AC-F2.** `apps/rt/tests/mcp.rs::mcp_server_handshakes_and_serves_all_five_tools` passa OU está marcado `#[ignore = "<reason>"]` com justificativa.
  Command: `rtk cargo test -p mustard-rt --test mcp 2>&1 | rtk node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{if(/FAILED. \d+ failed/.test(s)&&!/0 failed/.test(s))process.exit(1)})"`

- [ ] **AC-F3.** Seção "Deep Refactor Savings" em `apps/dashboard/src/pages/Economia.tsx` usa `t()` para todos os strings (sem PT/EN hardcoded inline na JSX).
  Command: `rtk node -e "const t=require('fs').readFileSync('apps/dashboard/src/pages/Economia.tsx','utf8');const m=t.match(/Deep Refactor Savings[\s\S]{0,3000}/);if(!m)process.exit(1);if(/>[À-ſ][^<]{3,}</.test(m[0]))process.exit(1)"`

- [ ] **AC-F4.** Após capturar baselines reais + reconcile, `economy_savings` tem ≥1 row por wave W0-W12 com `savings_tokens != 0`.
  Command: `rtk node -e "const {execSync}=require('child_process');const r=execSync('rtk sqlite3 .claude/.harness/telemetry.db \"SELECT COUNT(DISTINCT wave_id) FROM economy_savings WHERE savings_tokens != 0\"').toString().trim();if(parseInt(r,10)<13)process.exit(1)"`

- [ ] **AC-F5.** `mustard init` next-steps banner menciona `mustard-rt run adapt-cursor` na lista de extras (não só quando `--cursor` é passado).
  Command: `rtk node -e "const t=require('fs').readFileSync('apps/cli/src/commands/init.rs','utf8');const m=t.match(/print_next_steps[\s\S]{0,3000}/);if(!m||!/adapt-cursor/.test(m[0]))process.exit(1)"`

## Limites

- `apps/rt/src/run/memory.rs` (F1: escape/quote de hifens em queries FTS5)
- `apps/rt/tests/mcp.rs` (F2: fix root cause ou `#[ignore]` justificado)
- `apps/dashboard/src/pages/Economia.tsx` + `apps/dashboard/src/lib/i18n.ts` (F3: i18n keys novos)
- `apps/rt/src/run/economy_capture_baseline.rs` (F4: instrumentação opcional se necessário; capture real durante runs reais)
- `apps/cli/src/commands/init.rs` (F5: adicionar linha no `print_next_steps`)

OUT: tudo fora dos arquivos listados; nenhum subcomando novo; nenhum hook novo; nenhum schema novo.

## Plano

Light scope — single dispatch, 5 fixes pequenos (≤30 LOC cada). Sem PLAN phase formal, sem waves. Despache direto via `/mustard:spec ar` após approve.
