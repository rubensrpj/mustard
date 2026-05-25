# Tactical Fix: resume-bootstrap wave-index off-by-one + agent-prompt-render task slicing

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-25T18:32:00Z
### Lang: pt-BR
### Parent: 2026-05-25-mustard-deep-refactor

## Contexto

Tactical fix derivado de [[2026-05-25-mustard-deep-refactor]]. Descoberto na hora de despachar a Wave 0 inline (`/mustard:spec ar`).

Dois bugs ativos no `apps/rt` bloqueiam o despacho automático das 13 ondas:

**1. `resume-bootstrap` pula a Wave 0.**
Para a spec `2026-05-25-mustard-deep-refactor` (13 ondas W0–W12, pastas `wave-0-mixed`..`wave-12-mixed`, sem nenhum evento `pipeline.wave_complete`), `mustard-rt run resume-bootstrap` retornou:

```json
{ "currentWave": 1, "operationalSpecPath": ".../wave-1-mixed/spec.md", "waveModel": "[[0]]" }
```

Erros:
- `currentWave: 1` quando deveria ser `0` — o binário parece tratar `--wave` como 1-based no `agent-prompt-render` (`Wave number (1-based)`), mas o nome de pasta `wave-N-{role}` é 0-based no projeto Mustard. Resultado: a primeira onda dispatchável (W0 = `wave-0-mixed`) é silenciosamente pulada quando não há eventos de progresso.
- `waveModel: "[[0]]"` — o parser está lendo a coluna "Depende de" (literal `[[0]]`) da tabela markdown em `wave-plan.md`, em vez do campo de modelo. Não há coluna de modelo na tabela atual; o modelo deve vir do `meta.json` (que já tem `"model": "opus"`) ou do default por intent (feature → opus).

**2. `agent-prompt-render` não fatia tarefas por camada.**
Para wave "mixed" (W0 toca core + rt + dashboard com tarefas T0.1..T0.7), o prompt renderizado em qualquer `--role`/`--subproject` inclui o `## TASK` inteiro da spec. Se o orquestrador dispatcha 3 agentes paralelos com o mesmo TASK block, todos veem todas as tarefas e dependem só do `path_guard` para não pisar fora do subproject. Não há `--task-filter` (regex ou prefixo) para limitar o block a `T0.1|T0.5` para o core, `T0.4|T0.7` para o rt, `T0.2|T0.3|T0.6` para o dashboard.

## Critérios de Aceitação

- [ ] **AC-1.** `resume-bootstrap` retorna `currentWave: 0` para spec com 0 waves completas e pasta `wave-0-{role}` existente. Command: `rtk mustard-rt run resume-bootstrap --spec 2026-05-25-mustard-deep-refactor --json | rtk node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(j.currentWave!==0||!j.operationalSpecPath.includes('wave-0-'))process.exit(1)})"`
- [ ] **AC-2.** `resume-bootstrap` retorna `waveModel: "opus"` (ou outro modelo válido — `sonnet`/`opus`/`haiku`) para a mesma spec. Command: `rtk mustard-rt run resume-bootstrap --spec 2026-05-25-mustard-deep-refactor --json | rtk node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(!['opus','sonnet','haiku'].includes(j.waveModel))process.exit(1)})"`
- [ ] **AC-3.** `agent-prompt-render --task-filter "T0\\.(1|5)"` produz `## TASK` contendo só T0.1 e T0.5 (não T0.2..T0.7). Command: `rtk mustard-rt run agent-prompt-render --spec 2026-05-25-mustard-deep-refactor --wave 0 --role backend --subproject packages/core --task-filter "T0\\.(1|5)" | rtk node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{if(!s.includes('T0.1')||!s.includes('T0.5')||s.includes('T0.2')||s.includes('T0.6'))process.exit(1)})"`
- [ ] **AC-4.** Build verde após mudanças. Command: `rtk cargo build -p mustard-rt && rtk cargo clippy -p mustard-rt -- -D warnings`

## Arquivos

- `apps/rt/src/run/resume_bootstrap.rs` — fix wave-index inference (0-based when no progress events) + waveModel resolution (meta.json → routing default)
- `apps/rt/src/run/agent_prompt_render.rs` — add `--task-filter <regex>` flag, slice `## TASK` block by line prefix match
- `apps/rt/src/run/agent_prompt_template.md` — confirmar que `{task_block}` é o ponto único de injeção (sem mudanças se já estiver)

## Limites

OUT: qualquer mudança em `wave-plan.md` parsing além do necessário, mudanças em `event-projections`, mudanças em `meta.json` schema, mudanças em `path_guard`.

Total estimado: ≤80 LOC em 2-3 arquivos. Fix surgical.
