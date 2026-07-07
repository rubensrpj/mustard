---
id: spec.dieta-templates-por-catraca-teto
---

# dieta dos templates por catraca de teto global

<!-- drafter:tone=didactic — didactic tone; expand abbreviations on first use. -->

<!-- PRD -->

## Contexto

A spec `templates-md-enxutos-separar-lei` (2026-07-07, concluída) dietou os 5 maiores templates (−50%, 10.575 → 5.233 palavras) e instalou o teto executável: `apps/cli/tests/template_budget.rs` falha o build quando qualquer template estoura (tetos estritos nos dietados + teto global de 1.500 palavras). Os outros ~51 arquivos estão sob o teto global, mas ainda carregam o estilo antigo — parágrafos-muralha, ênfase inflacionada, porquês inline (maiores: task/SKILL 1.455, git-flow 1.352, wave-decomposition 1.313, submodule-rules 1.256, resume-loop 1.197). A catraca completa o trabalho sem big-bang: abaixa-se o teto global em degraus e dieta-se apenas quem estourar — o teste aponta o alvo de cada degrau sozinho.

## Usuários/Stakeholders

Toda sessão e todo pipeline (pagam a reinjeção); quem mantém (o molde LEI/MANUAL/PORQUÊ já está estabelecido e documentado em `docs/TEMPLATE-RATIONALE.md`).

## Métrica de sucesso

Teto global em 1.100 palavras com o corpus inteiro conforme; nenhuma regra operacional perdida (mesmo critério da dieta original: reestruturar, não resumir).

## Não-Objetivos

Não re-dietar os 5 já dietados; não mudar comportamento de fluxo; não traduzir.

## Critérios de Aceitação

- **AC-1** — Teste de orçamento passa com o teto do degrau corrente
  Command: `cargo test -p mustard-cli template_budget`
- **AC-2** — Todos os testes do cli passam
  Command: `cargo test -p mustard-cli`
- **AC-3** — Suíte do rt permanece verde
  Command: `cargo test -p mustard-rt`

## Checklist

- [ ] T1 — degrau 1: `GLOBAL_WORD_CAP` 1500 → 1300; dietar os que estourarem (task, git-flow, wave-decomposition — LEI em checklist, MANUAL em tabela, PORQUÊ para `docs/TEMPLATE-RATIONALE.md`).
- [ ] T2 — degrau 2: 1300 → 1100; dietar os que estourarem (submodule-rules, resume-loop, diagnose, bugfix…).
- [ ] T3 — estender o orçamento de ênfase (negrito ≤1/200 palavras) do conjunto dietado para o corpus inteiro.
- [ ] T4 — sincronizar cópias locais `.claude/` dos arquivos tocados.