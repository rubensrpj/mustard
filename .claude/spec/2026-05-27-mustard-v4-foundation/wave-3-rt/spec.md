# Wave 3 — wave-summary-context-format (papel: rt)

### Stage: Analyze
### Outcome: Active
### Scope: light
### Lang: pt-BR
### Parent: 2026-05-27-mustard-v4-foundation
### Checkpoint: 2026-05-27T17:56:09.926Z

## Contexto

Entrega o formato canônico do `_summary.md` (gerado ao fim de cada wave) e do `_context.md` (gerado para a wave N+1). Schema do `_summary.md` tem 7 seções obrigatórias (objetivo, herança, decisões, código, AC, verdict, próximos passos); `_context.md` recebe objetivo + herança + memória + posição no mapa de waves + sugestão de próximos passos. Templates idempotentes via wikilinks; escrita atômica via `mustard_core::atomic_md::write_atomic`. Insumo crítico para W6 (resume-bootstrap consome esses arquivos).

## Arquivos tocados

- `apps/rt/src/run/wave_summary.rs` (NOVO) — `build` + `write` do `_summary.md`
- `apps/rt/src/run/wave_context.rs` (NOVO) — `build` + `write` do `_context.md` da wave N+1
- `apps/cli/templates/skills/wave-summary-format.md` (NOVO) — skill que documenta o schema das 7 seções obrigatórias
- `apps/rt/src/run/mod.rs` (ESTENDIDO) — re-export dos novos módulos
- `apps/rt/src/main.rs` (ESTENDIDO) — wiring de subcomando opcional `mustard-rt run wave-summary --spec <name> --wave <N>`

## Funções tocadas

### Em `apps/rt/src/run/` (NOVO)
- `wave_summary::build`
- `wave_summary::write`
- `wave_context::build`
- `wave_context::write`

## Acceptance Criteria

Subset relevante desta wave:
- AC-A-8: `_summary.md` gerado por wave tem as 7 seções obrigatórias do schema
- AC-A-9: `_context.md` da wave N+1 ≤8.000 palavras quando gerado contra spec com 12 waves anteriores

## Tarefas

- [ ] T3.1: Criar `apps/rt/src/run/wave_summary.rs` com `build` e `write` produzindo `_summary.md` com as 7 seções obrigatórias (objetivo, herança, decisões, código, AC, verdict, próximos passos) (AC-A-8)
- [ ] T3.2: Criar `apps/rt/src/run/wave_context.rs` com `build` e `write` produzindo `_context.md` da wave N+1 (objetivo + herança + memória + posição no mapa + sugestão) com cap ≤8.000 palavras (AC-A-9)
- [ ] T3.3: Garantir escrita atômica de ambos via `mustard_core::atomic_md::write_atomic` e idempotência via wikilinks
- [ ] T3.4: Criar `apps/cli/templates/skills/wave-summary-format.md` documentando o schema das 7 seções obrigatórias (referência consumida pelos agentes) (AC-A-8)
- [ ] T3.5: Estender `apps/rt/src/run/mod.rs` re-exportando `wave_summary` e `wave_context`
- [ ] T3.6: Estender `apps/rt/src/main.rs` adicionando o subcomando `mustard-rt run wave-summary --spec <name> --wave <N>` cobrindo `build` + `write`
- [ ] T3.7: Adicionar teste rodando `wave_context::build` contra spec sintética com 12 waves anteriores — confirma ≤8.000 palavras (AC-A-9)

## Dependências (waves anteriores)

- W0 (formato canônico de `## Funções tocadas` parseado e disponível)