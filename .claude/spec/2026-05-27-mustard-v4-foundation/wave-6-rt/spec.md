# Wave 6 — resume-bootstrap-disciplined (papel: rt)

### Stage: Analyze
### Outcome: Active
### Scope: light
### Lang: pt-BR
### Parent: 2026-05-27-mustard-v4-foundation
### Checkpoint: 2026-05-27T17:56:09.926Z

## Contexto

Estende `resume_bootstrap::run` (herdado da no-sqlite) com disciplina de orçamento. Hoje o resume gasta ~60k tokens só pra começar (memória [[feedback_resume_flow_bloat]]). Esta wave impõe orçamento ≤10.000 tokens por bootstrap, faz pruning via wikilinks (carrega apenas os `_summary.md` cujos wikilinks aparecem no contexto da wave atual), e gera `_context.md` on-resume via `wave_context::build` (W3). Resultado: bootstrap rápido e enxuto mesmo em specs com 12+ waves anteriores.

## Arquivos tocados

- `apps/rt/src/run/resume_bootstrap.rs` (ESTENDIDO — herdado da no-sqlite) — adiciona pruning por orçamento + integração com `wave_context::build`
- `apps/rt/src/run/token_budget.rs` (NOVO) — primitiva para medir orçamento de tokens estimado por bloco de texto
- `apps/rt/src/run/mod.rs` (ESTENDIDO) — re-export do `token_budget`

## Funções tocadas

### Em `apps/rt/src/run/` (ESTENDIDO + NOVO)
- `resume_bootstrap::run` — adiciona pruning por orçamento ≤10k tokens
- `token_budget::estimate_tokens` (NOVO) — estima tokens via heuristic (chars/4 para LLMs típicos)
- `token_budget::prune_to_budget` (NOVO) — recebe lista de candidatos ordenados por prioridade e corta no orçamento

## Acceptance Criteria

Subset relevante desta wave:
- AC-A-10: `resume-bootstrap` em spec com 12 waves usa ≤10.000 tokens (medido pelo orçamento exportado)

## Tarefas

- [ ] T6.1: Criar `apps/rt/src/run/token_budget.rs` com `estimate_tokens` (heuristic chars/4 para LLMs típicos)
- [ ] T6.2: Implementar `token_budget::prune_to_budget` em `apps/rt/src/run/token_budget.rs` — recebe lista de candidatos ordenados por prioridade e corta no orçamento (AC-A-10)
- [ ] T6.3: Estender `apps/rt/src/run/resume_bootstrap::run` (herdado da no-sqlite) com pruning por orçamento ≤10.000 tokens via `token_budget::prune_to_budget`
- [ ] T6.4: Estender `resume_bootstrap::run` carregando apenas os `_summary.md` cujos wikilinks aparecem no contexto da wave atual (pruning por wikilinks)
- [ ] T6.5: Integrar `wave_context::build` (W3) em `resume_bootstrap::run` para gerar `_context.md` on-resume
- [ ] T6.6: Estender `apps/rt/src/run/mod.rs` re-exportando `token_budget`
- [ ] T6.7: Adicionar teste rodando `resume_bootstrap::run` contra spec sintética com 12 waves anteriores — confirma ≤10.000 tokens (AC-A-10)

## Dependências (waves anteriores)

- W3 (`wave_context::build` precisa existir)

<!-- wikilinks-footer-start -->
- [feedback_resume_flow_bloat](?) ⚠ não resolvido
<!-- wikilinks-footer-end -->