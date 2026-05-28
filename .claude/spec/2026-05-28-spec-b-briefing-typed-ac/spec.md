# Spec B — Briefing unificado + AC tipado

### Stage: Plan
### Outcome: Active
### Scope: full
### Lang: pt-BR
### Parent: 2026-05-27-mustard-v4-foundation
### Checkpoint: 2026-05-28T00:00:00.000Z

## Contexto

Escopo formalizado como **non-goal** da Spec A v4 (§ Não-Objetivos linhas 27-28). Duas peças que dependem da fundação Spec A já entregue:

1. **Briefing unificado pré-pipeline.** Hoje cada despacho de agente monta seu próprio prompt via `agent_prompt_render` + memória cruzada + skills resolvidas. A Spec A v4 entregou o `agent_prompt_render::run` agnóstico, mas o *formato* do briefing (heading hierarchy, ordem de seções, peso por seção) ainda vive disperso entre `templates/refs/feature/`, `agent_prompt_template.md` embedded, e ad-hoc no orquestrador. A Spec B define um *briefing schema* canônico — pt-BR/en-US via i18n — que substitui essas três fontes por uma só, parametrizada por `{spec, wave, role, mode}`.

2. **AC tipado.** Spec A v4 entregou AC binários com `Command:` shell-ready, mas cada AC é uma linha de prosa onde a *função sob teste* é implícita (descrita no corpo). A Spec B exige que cada AC declare formalmente `Função: <qualifier>` no header — alinhando AC com `## Funções tocadas` (entregue em W0). Isso destrava QA 3-dim (Spec D) e review rubric (Spec D) — eles precisam saber QUE FUNÇÃO cada AC valida pra pontuar cobertura positivo/negativo/regressão.

Dependências satisfeitas pela Spec A v4 (fechada Stage:Close em 2026-05-28):
- W0: `mustard_core::spec::touched_functions::{parse, validate, functions_in_scope_with_fallback}` — parser + validator do formato canônico (consumido pelo header `Função:`).
- W3: `wave_summary::build` + `wave_context::build` — usados pelo briefing pra montar herança/contexto.
- W4: `gate_regression_check::build_vocab_matcher` — usado pelo gate AC tipado pra detectar drift.
- i18n: `mustard_core::i18n::translate` + `project_locale` — narrativa do briefing 100% via catálogo.

## Usuários/Stakeholders

Maintainer único (Rubens). Indireto: futuros contribuidores que rodam `/mustard:feature` ou `/mustard:bugfix` num projeto-alvo e dependem do briefing canônico pra que o agente despachado siga o mesmo formato em qualquer máquina.

## Métrica de sucesso

- **Briefing.** `mustard-rt run agent-prompt-render --spec X --wave N --role R --mode first` emite o briefing canônico com ≤ 6 seções (objetivo / herança / contexto / decisões prévias / tarefas / AC). Todos os headings traduzidos via i18n; nenhum string user-facing literal no template embedded. Diff entre `agent_prompt_template.md` antigo e novo: zero strings em pt-BR/en-US — só `{i18n.key}` placeholders.
- **AC tipado.** Cada AC binário escreve `### Função: <qualifier>` na linha imediatamente abaixo do bullet. `mustard-rt run analyze-validation` rejeita AC sem `Função:` (exit 2). Parser exposto via `mustard_core::spec::ac_typed::parse(spec_md) -> Vec<TypedAc>`.
- **Compatibilidade.** ACs herdados (Spec A v4 e anteriores) continuam parseáveis (downgrade silencioso para `Função: None`) — não quebra QA-run.

## Não-Objetivos

- **QA 3-dim** (positivo + negativo + não-regressão por função) — diferido para Spec D
- **Review rubric** (rubrica fixa para wave de review) — diferido para Spec D
- **Migrar specs antigas para o novo formato** — sem usuários em prod; Specs B+ podem adotar progressivamente. Specs A v4 e anteriores ficam no formato herdado.

## Critérios de Aceitação

A definir durante o PLAN. Esqueleto inicial:

- [ ] AC-B-1: `mustard-rt run agent-prompt-render` produz briefing com 6 seções e zero strings literais (validado por grep no embedded template)
- [ ] AC-B-2: `mustard_core::spec::ac_typed::parse` extrai `Função:` de cada AC quando presente
- [ ] AC-B-3: `analyze-validation` rejeita spec com AC sem `Função:` em Stage:Execute
- [ ] AC-B-4: Briefing renderiza em pt-BR quando `mustard.json#lang == "pt-BR"` e en-US quando `"en-US"`
- [ ] AC-B-5: ACs herdados (sem `Função:`) downgrade silencioso para `Função: None`; QA-run não quebra

## Tarefas

A definir no PLAN. Esqueleto inicial (4-6 waves esperadas):
- W1: `mustard_core::spec::ac_typed` (parser, modelo, fallback)
- W2: `analyze-validation` strict mode para `Função:`
- W3: Briefing schema canônico em `agent_prompt_template.md`
- W4: i18n keys novas em `mustard_core::i18n` para headings do briefing
- W5: QA + CLOSE

## Dependências

- Spec A v4 (`2026-05-27-mustard-v4-foundation`) — **CLOSED 2026-05-28** ✓

## Notas

Esta spec foi scaffoldada em 2026-05-28 como entrega final da Spec A v4 (registro de continuidade). O Plan ainda não foi rodado — `Stage: Plan` significa "aprovação pendente do user", não "wave-plan materializado".