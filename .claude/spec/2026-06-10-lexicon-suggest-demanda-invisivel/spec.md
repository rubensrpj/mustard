# Tactical Fix: lexicon-suggest não enxerga a demanda real (filtro de sessão + tradução prévia do orquestrador)

## Contexto

Verificado em produção (sialia, 2026-06-10): `mustard-rt run lexicon-suggest --root C:\Atiz\sialia` retorna `{"queries": 0, "candidates": []}` apesar de ≥5 eventos `feature.query` reais do dia na telemetria (.claude/.session/*/.events e .claude/spec/*/.events). As 7 pontes PT verificadas pela auditoria (previsao/provisao/natureza/financeiro/financeira/observacao/descricao/lancamento+launch) tiveram que ser aplicadas à mão no overlay — o loop por demanda não teria proposto nenhuma. Duas causas:

1. **Filtro de sessão estrito**: o correlator só considera `feature.query` da "sessão/spec ativa"; invocado de fora da sessão emissora (caso típico: o usuário roda a sugestão depois, em outra sessão), vê zero. Era a Preocupação "filtro de sessão tolerante" registrada na review da spec `lexico-supervisionado-promover-pontes-confirmadas` — agora confirmada.
2. **Demanda lavada pela tradução prévia**: a prosa do /feature instrui o orquestrador a consultar com vocabulário do repo (EN), então o intent PT vira termos EN antes do binário — os misses PT nunca entram no `feature.query` (a query real de hoje chegou como `financial account provision...`). O único miss orgânico foi `observation` (EN), e nem o par observation→notes foi correlacionado (causa 1). Agravante de auditabilidade: o payload do `feature.query` não grava o texto bruto do `--intent`, só os queryTerms tokenizados.

Fix: (a) afrouxar o filtro do correlator — considerar eventos de toda a telemetria do workspace dentro de uma janela (ex.: por spec, ou N dias), não só a sessão corrente; (b) gravar o intent bruto (ou os termos pré-tradução) no payload do `feature.query` para a demanda PT ficar visível; (c) prosa do /feature: a PRIMEIRA consulta vai com o vocabulário do usuário (a escada+léxico existem para isso) e a re-query EN curada fica para miss/weak — assim o miss PT é registrado antes da tradução.

## Critérios de Aceitação

- **AC-1** — Correlator: fixture com feature.query de sessões distintas do mesmo workspace gera candidatos (miss→bridge); janela/escopo coberto por teste.
  Command: `cargo test -p mustard-rt lexicon_suggest`
- **AC-2** — Workspace verde.
  Command: `cargo test --workspace`
- **AC-3** — Prosa: primeira consulta com vocabulário do usuário; re-query EN só em miss/weak.
  Command: `rg -n "vocabulário do usuário|user's vocabulary|first query" apps/cli/templates/commands/mustard/feature/SKILL.md`

## Arquivos

- apps/rt/src/commands/ (lexicon_suggest) — filtro de sessão → escopo workspace/spec com janela
- apps/rt/src/commands/feature.rs — payload do feature.query grava intent bruto/termos pré-tradução
- apps/cli/templates/commands/mustard/feature/SKILL.md — ordem das consultas (PT primeiro, EN na re-query)
- testes em apps/rt