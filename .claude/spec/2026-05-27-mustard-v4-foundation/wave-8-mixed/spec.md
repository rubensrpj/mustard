# Wave 8 — qa-and-close-followups (papel: mixed)

### Stage: Analyze
### Outcome: Active
### Scope: light
### Lang: pt-BR
### Parent: 2026-05-27-mustard-v4-foundation
### Checkpoint: 2026-05-27T17:56:09.926Z

## Contexto

Wave de fechamento. QA-functional roda todos os AC binários (AC-A-1 a AC-A-17) via `mustard-rt run qa-run --spec 2026-05-27-mustard-v4-foundation`; quality-ledger inaugural recebe snapshot de métricas de baseline (tempo de bootstrap, tamanho típico de `_summary.md`, taxa de falso-positivo do gate no review W7); emissão de `pipeline.status` Completed; CLOSE da spec A; preparação dos follow-ups documentados para Spec B (briefing, AC tipado).

## Arquivos tocados

- `.claude/spec/2026-05-27-mustard-v4-foundation/qa-results.md` (NOVO) — resultado de cada AC binário (pass/fail/output literal)
- `.claude/spec/2026-05-27-mustard-v4-foundation/quality-ledger.md` (NOVO) — snapshot de métricas de baseline (entrada inaugural)
- `.claude/spec/2026-05-27-mustard-v4-foundation/meta.json` (MODIFICADO) — atualiza `stage: Close`, `outcome: Completed`, `currentWave: 8`
- `.claude/spec/2026-05-27-mustard-v4-foundation/spec.md` (MODIFICADO) — atualiza `### Stage:` e `### Outcome:` no header

## Funções tocadas

Nenhuma — wave de fechamento, sem mudança de código (apenas validação e atualização de metadata).

## Acceptance Criteria

Cobertura total:
- AC-A-1 a AC-A-17 (todos os 17 — QA-functional valida cada um)
- Quality-ledger ganha entrada inaugural
- Spec A move pra `Stage: Close, Outcome: Completed`

## Tarefas

- [ ] T8.1: Rodar `mustard-rt run qa-run --spec 2026-05-27-mustard-v4-foundation` executando todos os AC binários (AC-A-1 a AC-A-17)
- [ ] T8.2: Escrever `.claude/spec/2026-05-27-mustard-v4-foundation/qa-results.md` registrando pass/fail/output literal por AC (AC-A-1 a AC-A-17)
- [ ] T8.3: Validar que cada AC retornou pass; em caso de fail, bloquear CLOSE e abrir sub-spec tática
- [ ] T8.4: Escrever `.claude/spec/2026-05-27-mustard-v4-foundation/quality-ledger.md` com snapshot de métricas de baseline (tempo de bootstrap, tamanho típico de `_summary.md`, taxa de falso-positivo do gate no review W7) — entrada inaugural
- [ ] T8.5: Atualizar `.claude/spec/2026-05-27-mustard-v4-foundation/meta.json` setando `stage: Close`, `outcome: Completed`, `currentWave: 8`
- [ ] T8.6: Atualizar `.claude/spec/2026-05-27-mustard-v4-foundation/spec.md` no header — `### Stage: Close` e `### Outcome: Completed`
- [ ] T8.7: Emitir `pipeline.status` Completed e preparar follow-ups documentados (briefing + AC tipado) pra Spec B

## Dependências (waves anteriores)

- W1, W1.5, W2, W3, W4, W5, W6, W7 (todas as anteriores)