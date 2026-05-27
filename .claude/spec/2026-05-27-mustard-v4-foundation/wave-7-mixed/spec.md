# Wave 7 — review-cobertura-w6 (papel: mixed)

### Stage: Analyze
### Outcome: Active
### Scope: light
### Lang: pt-BR
### Parent: 2026-05-27-mustard-v4-foundation
### Checkpoint: 2026-05-27T17:56:09.926Z

## Contexto

Wave de validação consolidada. Roda a Spec A inteira contra a fixture do caso W6 (capturada em W0); mede quantos pontos do gate disparam (espera-se ≥3 dos 4 pontos críticos — AC-A-1); ajusta thresholds do gate W4 e o vocabulário inicial em `.claude/vocab/regression.toml` baseado nos disparos reais. Sem código novo — só configuração + relatório. Decisão §16 #2 cravada: trabalha contra **fixture controlada** por default; override pra dado real só por justificativa documentada na sub-wave.

## Arquivos tocados

- `.claude/vocab/regression.toml` (MODIFICADO) — ajustes de pesos baseados no review
- `.claude/spec/2026-05-27-mustard-v4-foundation/review-w7-report.md` (NOVO) — relatório de quantos pontos disparam contra a fixture W6
- `apps/rt/src/run/gate_regression_check.rs` (MODIFICADO) — ajustes de thresholds (verde/amarelo/vermelho) baseados no review

## Funções tocadas

### Em `apps/rt/src/run/` (MODIFICADO)
- `gate_regression_check::run` — ajusta thresholds de classificação verdict (sem mudança de signature)

## Acceptance Criteria

Subset relevante desta wave:
- AC-A-1: Caso W6 reproduzido dispara o gate em ≥3 dos 4 pontos críticos (validação consolidada — depende de W4 + W5 implementados)

## Tarefas

- [ ] T7.1: Rodar a Spec A inteira contra a fixture do caso W6 (capturada em W0) — coletar quantos pontos do gate W4 disparam dentro dos 4 pontos críticos (AC-A-1)
- [ ] T7.2: Escrever `.claude/spec/2026-05-27-mustard-v4-foundation/review-w7-report.md` com o relatório (pontos disparados, falsos positivos, gap entre disparado e esperado) (AC-A-1)
- [ ] T7.3: Ajustar pesos das 4 camadas em `.claude/vocab/regression.toml` baseado nos disparos reais — sem inventar termos fora dos já presentes (W1)
- [ ] T7.4: Ajustar thresholds verdict (verde/amarelo/vermelho) em `gate_regression_check::run` — sem mudança de signature, apenas constantes/config interna
- [ ] T7.5: Re-rodar a Spec A contra a fixture após os ajustes e confirmar ≥3 dos 4 pontos críticos disparam (AC-A-1)
- [ ] T7.6: Anexar resumo final ao `review-w7-report.md` documentando que o trabalho rodou contra fixture controlada (decisão §16 #2) — sem override pra dado real

## Dependências (waves anteriores)

- W4 (gate run-based completo)
- W5 (span-level integrado)