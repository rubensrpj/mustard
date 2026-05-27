# Wave 5 — span-level-integration (papel: rt)

### Stage: Analyze
### Outcome: Active
### Scope: light
### Lang: pt-BR
### Parent: 2026-05-27-mustard-v4-foundation
### Checkpoint: 2026-05-27T17:56:09.926Z

## Contexto

Integra o gate W4 com o ciclo `SubagentStop` — span-level eval por filho retornado, conforme P23 (literatura 2026). Gate roda a cada filho terminar, **não** acumula até o fim da wave. Verdict por filho registrado em `_review-spans.md` (append-only, atômico). Consolidação da wave bloqueada se algum filho retornou vermelho. Estende módulos herdados da no-sqlite (`subagent_inject`, `agent_prompt_render`) — sem reescrita.

## Arquivos tocados

- `apps/rt/src/hooks/subagent_inject.rs` (ESTENDIDO — herdado da no-sqlite) — adiciona vocabulário no prompt + dispara `gate_regression_check::check_after_child_return` ao receber `SubagentStop`
- `apps/rt/src/run/agent_prompt_render.rs` (ESTENDIDO — herdado) — injection de vocabulário pré-armado no prompt do agente
- `apps/rt/src/run/review_spans.rs` (NOVO) — append atômico de verdict por filho em `_review-spans.md`
- `apps/rt/src/run/mod.rs` (ESTENDIDO) — re-export do `review_spans`

## Funções tocadas

### Em `apps/rt/src/hooks/` (ESTENDIDO)
- `subagent_inject::dispatch` — adiciona vocabulário + span-level check

### Em `apps/rt/src/run/` (ESTENDIDO + NOVO)
- `agent_prompt_render::run` — adiciona injection de vocabulário
- `review_spans::append_verdict` (NOVO) — escreve uma linha atômica em `_review-spans.md`

## Acceptance Criteria

Subset relevante desta wave:
- AC-A-5: Span-level eval roda a cada `SubagentStop`, nunca acumula até fim de wave
- AC-A-7 (cobertura cruzada): vermelho de qualquer filho bloqueia consolidação

## Tarefas

- [ ] T5.1: Estender `apps/rt/src/hooks/subagent_inject::dispatch` (herdado da no-sqlite) injetando vocabulário pré-armado no prompt do agente
- [ ] T5.2: Estender `subagent_inject::dispatch` disparando `gate_regression_check::check_after_child_return` (W4) ao receber `SubagentStop` — span-level, não acumula (AC-A-5)
- [ ] T5.3: Estender `apps/rt/src/run/agent_prompt_render::run` (herdado) adicionando injection de vocabulário no prompt renderizado
- [ ] T5.4: Criar `apps/rt/src/run/review_spans.rs` com `append_verdict` escrevendo uma linha atômica em `_review-spans.md` (append-only)
- [ ] T5.5: Estender `apps/rt/src/run/mod.rs` re-exportando `review_spans`
- [ ] T5.6: Garantir bloqueio de consolidação da wave quando qualquer linha de `_review-spans.md` registrar verdict vermelho (AC-A-7)
- [ ] T5.7: Adicionar teste de integração disparando 3 filhos sequenciais — verifica que `append_verdict` é chamado a cada `SubagentStop` (AC-A-5)

## Dependências (waves anteriores)

- W4 (gate `check_after_child_return` precisa existir)