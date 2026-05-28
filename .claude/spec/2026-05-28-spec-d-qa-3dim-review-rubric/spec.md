# Spec D — QA 3-dim + Review rubric

### Stage: Plan
### Outcome: Active
### Scope: full
### Lang: pt-BR
### Parent: 2026-05-27-mustard-v4-foundation
### Checkpoint: 2026-05-28T00:00:00.000Z

## Contexto

Escopo formalizado como **non-goal** da Spec A v4 (§ Não-Objetivos linhas 29-30). Depende da Spec B (briefing + AC tipado) — sem `Função:` em cada AC, QA-run não consegue separar dimensões nem review-rubric pode pontuar por função.

Duas peças:

1. **QA 3-dim por função.** Hoje `mustard-rt run qa-run` roda cada `Command:` literal e reporta `pass/fail`. A Spec D divide a validação de cada AC em **3 dimensões** referenciadas à função declarada (via `Função:` da Spec B):
   - **Positivo**: input válido → output esperado (caminho feliz).
   - **Negativo**: input inválido → erro tipado (não panic, não fail-open).
   - **Não-regressão**: snapshot antes/depois da função (consome `mustard_core::regression_check::compare_snapshots` da Spec A v4 W2).

2. **Review rubric.** `/mustard:review` hoje despacha um agente genérico que escreve `review.md` em prosa. A Spec D fixa uma **rubrica de 5 eixos** (correctness, idiomaticity, performance, security, alignment-to-spec) com escala 1-5 por eixo. Cada eixo cita evidência (linha de código ou commit). O verdict final é a média ponderada.

Dependências satisfeitas pela Spec A v4:
- W2: `mustard_core::regression_check::compare_snapshots` — usado pela dimensão Não-regressão.
- W4: gate `Verdict::{Amber, Red}` — usado pelo verdict de QA 3-dim quando a dimensão Negativo falha.
- W7: caso W6 fixture — usada pelo bench da review rubric pra calibrar peso de cada eixo.

Dependências pendentes (Spec B):
- `mustard_core::spec::ac_typed::parse` — usado pra extrair `Função:` de cada AC.
- Briefing canônico — usado pelo review agent.

## Usuários/Stakeholders

Maintainer único (Rubens). Indireto: contribuidores em projetos-alvo que rodam `/mustard:qa` ou `/mustard:review` e dependem de uma rubrica consistente entre máquinas.

## Métrica de sucesso

- **QA 3-dim.** `mustard-rt run qa-run --spec X --format json` emite, por AC tipado, 3 entries (`positive`, `negative`, `non_regression`). Overall pass exige `positive == pass` E `negative == pass` E (`non_regression == pass` OU `non_regression == skip` quando snapshot ausente).
- **Review rubric.** `/mustard:review` emite `review.md` com header tabular `Eixo | Score | Evidência`. 5 eixos canônicos, escala 1-5, cada score acompanhado de citação `<arquivo>:<linha>` ou `<commit-sha>`. Verdict numérico = média ponderada (pesos definidos no PLAN).

## Não-Objetivos

- **Substituir gate de regressão** (Spec A v4 W4). QA 3-dim consome `regression_check::compare_snapshots`; não duplica.
- **Briefing nem AC tipado** — Spec B.
- **Auto-correção de specs antigas** — ACs sem `Função:` (downgrade da Spec B) caem em `non_regression: skip` automaticamente.

## Critérios de Aceitação

A definir no PLAN. Esqueleto inicial:

- [ ] AC-D-1: `qa-run --format json` emite array de `{ac_id, function, positive, negative, non_regression}` por AC tipado
- [ ] AC-D-2: AC sem `Função:` (legacy) reporta `non_regression: "skip"` sem erro
- [ ] AC-D-3: `mustard-rt run review-rubric --spec X` emite tabela com 5 eixos × score × evidência
- [ ] AC-D-4: `correctness` falhar (score < 3) bloqueia merge — exit 2 do subcomando
- [ ] AC-D-5: Pesos da rubrica configuráveis via `.claude/review.toml` (fallback hardcoded em sigência da Spec A v4 W7#2)

## Tarefas

A definir no PLAN. Esqueleto inicial (5-7 waves esperadas):
- W1: `mustard_core::qa::dimensions` (struct `QaResult3D`, parser)
- W2: `qa-run --format json` emite 3-dim per AC tipado
- W3: `review-rubric` subcomando + 5-axis schema
- W4: pesos via TOML em `.claude/review.toml`
- W5: integração com `/mustard:review` SKILL
- W6: QA + CLOSE

## Dependências

- Spec A v4 (`2026-05-27-mustard-v4-foundation`) — **CLOSED 2026-05-28** ✓
- Spec B (`2026-05-28-spec-b-briefing-typed-ac`) — em PLAN; QA 3-dim depende de AC tipado

## Notas

Scaffoldada em 2026-05-28 como entrega final da Spec A v4. O PLAN aguarda a aprovação user + conclusão da Spec B (dependência forte em `Função:` por AC).