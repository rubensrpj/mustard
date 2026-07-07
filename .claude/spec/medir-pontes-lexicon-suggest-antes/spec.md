---
id: spec.medir-pontes-lexicon-suggest-antes
---

# medir as pontes do lexicon-suggest antes de qualquer volta da semântica

<!-- drafter:tone=didactic — didactic tone; expand abbreviations on first use. -->

<!-- PRD -->

## Contexto

A lacuna estrutural do digest é vocabulário: o pedido cru em português devolve `weak` (na sonda do painel de contratos, todos os termos PT — `vencimento`, `atraso`, `pagamentos` — deram tier `none`), e o valor só destrava quando o orquestrador formula a query em inglês do código. A ferramenta desenhada para encurtar isso já existe e é determinística: `lexicon-suggest` correlaciona as rodadas de `feature.query` da telemetria e propõe pontes termo-que-falhou → termo-do-código, gravadas só com aceite (`--accept missed=bridged`). O que NÃO existe é medição: não sabemos se as pontes estão sendo alimentadas, quantas foram aceitas, nem quanto reduzem re-consultas. Sem essa medição, qualquer conversa sobre reabrir a camada semântica (12,5 mil linhas cortadas em 2026-07-07 por recall ocioso e sumários fracos) é chute. Regra da casa: medir antes de construir.

## Usuários/Stakeholders

O orquestrador (menos rodadas de re-consulta); quem decide o roadmap (dado real para a decisão semântica-sim-ou-não).

## Métrica de sucesso

Relatório com números: pontes propostas × aceitas por projeto; taxa de `weak`/`none` que uma re-consulta com ponte resolveria; veredito fundamentado — lexicon basta, lexicon precisa de ajuste, ou (só com evidência) abrir spec de semântica começando pelos sumários.

## Não-Objetivos

Não implementar embeddings/semântica; não mudar o ranking do digest; não criar juiz LLM sobre o localizador.

## Critérios de Aceitação

- **AC-1** — Testes do lexicon-suggest cobrem o ciclo propor→aceitar→usar na query seguinte
  Command: `cargo test -p mustard-rt lexicon`
- **AC-2** — Suíte do rt permanece verde
  Command: `cargo test -p mustard-rt`
- **AC-3** — Lint limpo
  Command: `cargo clippy -p mustard-rt`

## Checklist

- [ ] T1 — instrumentar/extrair a medição: das telemetrias `feature.query` existentes (mustard + sialia), contar rodadas `weak`/`none`, re-consultas e pontes candidatas que o `lexicon-suggest` propõe hoje.
- [ ] T2 — verificar o elo fraco do ciclo: a sugestão pós-re-consulta está sendo oferecida/aceita na prática? Se o fluxo nunca sugere, consertar o ponto de sugestão (prosa do fluxo ou comando).
- [ ] T3 — relatório com o veredito (fica como está / ajustar lexicon / abrir spec de semântica) registrado nesta spec.