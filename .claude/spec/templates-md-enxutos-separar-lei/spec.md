---
id: spec.templates-md-enxutos-separar-lei
---

# templates md enxutos: separar lei, manual e porquê; orçamento de palavras e ênfase imposto por teste

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

A auditoria de 2026-07-07 mediu a camada de templates `.md` (56 arquivos, 42.223 palavras): um negrito a cada ~52 palavras, uma palavra em CAIXA-ALTA a cada ~39, 75 "NEVER", e os 5 maiores arquivos (10,6 mil palavras — ~25% do corpus) misturam três coisas em parágrafos-muralha: LEI (o que pode/não pode), MANUAL (como chamar as ferramentas) e PORQUÊ (justificativas). O peso é pago em toda sessão (CLAUDE.md) e em todo pipeline (SKILL + refs), e a própria medição de economia do projeto aponta a reinjeção do harness como o maior custo em runs pequenos. A história do projeto também mostra que obediência vem dos gates, não do drama da prosa — então a prosa pode emagrecer sem perder função.

## Usuários/Stakeholders

O orquestrador e os subagentes (pagam menos tokens por turno e obedecem melhor a checklists curtos); quem mantém o mustard (o porquê preservado em doc não-injetado); todo projeto consumidor (recebe templates mais baratos via `mustard update`).

## Métrica de sucesso

Os 5 maiores arquivos caem para ≤ metade do peso somado (10,6 mil → ≤ 5,3 mil palavras) sem perder nenhuma regra operacional; o teto vira lei executável: um teste de orçamento que FALHA quando qualquer template estoura.

## Não-Objetivos

Não reescrever os outros 51 arquivos agora (eles já cabem no teto global e seguem o molde depois); não mudar comportamento de nenhum fluxo ou gate; não tocar código Rust além do teste novo; não traduzir templates.

## Critérios de Aceitação

- **AC-1** — Teste de orçamento dos templates passa (tetos por arquivo dietado + teto global 1500 + orçamento de ênfase)
  Command: `cargo test -p mustard-cli template_budget`
- **AC-2** — Todos os testes do cli passam
  Command: `cargo test -p mustard-cli`
- **AC-3** — Suíte completa do rt permanece verde (nenhum fluxo/gate regrediu)
  Command: `cargo test -p mustard-rt`

## Checklist

- [x] T1 — teste `template_budget` em `apps/cli/tests/` (tetos: CLAUDE.md ≤1000; feature/SKILL ≤1200; pipeline-config ≤1200; pipeline-execution/SKILL ≤1200; spec-language ≤700; global ≤1500; negrito ≤1/200 palavras nos dietados) — VERMELHO antes da dieta.
- [x] T2 — dieta dos 5 arquivos em `apps/cli/templates/` (LEI = checklist no topo; MANUAL = tabelas; PORQUÊ = movido para `docs/TEMPLATE-RATIONALE.md`), preservando toda regra operacional.
- [x] T3 — `docs/TEMPLATE-RATIONALE.md` criado com o racional movido (não injetado em sessão alguma).
- [x] T4 — cópias locais em `.claude/` sincronizadas com os templates dietados.
- [x] T5 — AC-1..AC-3 verdes.