# W8 — Context injection optimization (scope-by-spec + auto-capture + skill-resolve signal)
### Stage: Plan
### Outcome: Active
### Flags: 

## Contexto

Injeção atual no `SessionStart` é global (top-5 patterns + top-5 decisions + top-5 lessons ≈ 1500 tokens) sem filtrar pela spec ativa ([[feedback_resume_flow_bloat]] menciona ~60K só para começar). Esta wave entrega scope-by-spec + observers + `context-slice` estendido + signal de relevância via `skill-resolve` (W1.T1.4).

## Tarefas

- [x] **T8.1** — `SessionStart` hook scope-by-spec: top-3 da spec atual + top-2 globais (em vez de top-15 indiscriminados). Em `apps/rt/src/hooks/session_start.rs`.
- [x] **T8.2** — `UserPromptSubmit` adiciona 1 linha "Pipeline em curso" quando não é `/mustard:*` E há spec ativa.
- [x] **T8.3** — Novo hook `subagent_inject` em `apps/rt/src/hooks/subagent_inject.rs`. Para Task sem SKILL declarada, injeta slice mínimo do `CONTEXT.md` + skills resolvidas via W1.T1.4.
- [x] **T8.4** — `PostToolUse(Task)` observer `auto_capture_summary` em `apps/rt/src/hooks/auto_capture_summary.rs`. Parse `<MEMORY>` ou seção `Resumo:` no output do Task, grava em `agent_memory` (W7).
- [x] **T8.5** — `SubagentStop` observer: bump `last_used` da memória que apareceu no output.
- [x] **T8.6** — `SessionEnd` consolidação: `agent_memory` com confidence ≥ 0.85 vira `memory_decisions/lessons` permanentes.
- [x] **T8.7** — `PreCompact` adiciona até 3 `agent_memory` recentes ao snapshot.
- [x] **T8.8** — `context-slice` estendido para `CLAUDE.md` (hoje só `CONTEXT.md`). Em `apps/rt/src/run/context_slice.rs`.
- [x] **T8.9** — Flag `--budget-tokens` em `agent-prompt-render`: trunca placeholders para caber no orçamento. `skill-resolve` (W1) é signal de relevância para escolher quais skills entram primeiro.

- [x] **T8.10** — Carregamento seletivo de `memory/` da spec ativa. `SessionStart` hook NÃO auto-injeta memória da spec por padrão (caso contrário a janela de contexto explode). Em vez disso: hook `subagent_inject` (T8.3) consulta `skill-resolve` (W1.T1.4) para escolher quais princípios da `memory/` da spec ativa carregar **por dispatch**, não por sessão. Memory user-global (preferências de comportamento) continua auto-load nativo do Claude Code. Memory de specs arquivadas: zero carga automática — acessível só via Grep/Read sob demanda.

## Critérios de Aceitação

- [x] **AC-W8.1** — `session_start` scope-by-spec. Command: `rtk node -e "const t=require('fs').readFileSync('apps/rt/src/hooks/session_start.rs','utf8');if(!/scope_by_spec|current_spec/.test(t))process.exit(1)"`
- [x] **AC-W8.2** — Hooks novos registrados em `apps/rt/src/registry.rs`. Command: `rtk node -e "const t=require('fs').readFileSync('apps/rt/src/registry.rs','utf8');for(const h of ['subagent_inject','auto_capture_summary']){if(!t.includes(h))process.exit(1)}"`
- [x] **AC-W8.3** — `context-slice` aceita `--context-claude-md` flag. Command: `rtk mustard-rt run context-slice --help`
- [x] **AC-W8.4** — `agent-prompt-render --budget-tokens N` respeita orçamento. Smoke: baseline=4387c, --budget-tokens 50000=4387c (no trim), --budget-tokens 800=2176c (50% trim, stderr summary emitted).

## Limites

`apps/rt/src/hooks/{session_start,subagent_inject,auto_capture_summary,stop_observer}.rs`, `apps/rt/src/run/context_slice.rs`, `apps/rt/src/run/agent_prompt_render.rs`, `apps/rt/src/registry.rs`.

OUT: tudo fora.

## Role

rt
