# Robustez do ranking de âncoras do digest — cobertura por termo + raridade + par morfológico

## Contexto

Auditoria 2026-06-10 da run `/feature` payables no sialia (binário g4327b44): a agregação de âncoras do digest é soma pura `tier×BM25` por arquivo, sem IDF, sem cobertura por termo e sem diversificação — termos genéricos/frequentes da query (financial=363, account=254, code=368) afogam o domínio-alvo (payables=103, nature=18) e entregam 8/12 âncoras do vizinho financial-accounts com 0 payables, enquanto o relatório por termo (caminho separado, não capado, estratificado) acerta 17/18. Agravantes: a guarda anti-truncamento bloqueia plural↔singular genuíno (a query `payables` nunca alcança o token `payable`, 254 ocorrências — as próprias páginas-alvo ficam invisíveis), e o drafter ignora `report.reason=weak`. Ablações E1-E8 confirmaram; MMR/estratificação/cap-25 inocentados (só atuam nos samples por termo). Detalhe completo na memória do projeto (`mustard-sialia-payables-audit`).

Fase 1 (crate scan): seleção de âncoras cobertura-primeiro + IDF ponto-fixo no preenchimento + `files_detail` auditável + `slices_omitted` + relaxamento do guard quando os stems coincidem.
Fase 2 (crate rt, após fechar a pipeline concorrente `checklist-progresso-por-onda`): payload do feature expõe score/termos por âncora; spec-draft rotula âncoras `weak`/`none` como baixa-confiança e não as semeia no checklist.

## Critérios de Aceitação

- **AC-1** — Crate scan verde com os novos testes: cobertura por termo nas âncoras (regressão do caso sialia em fixture: termos frequentes do vizinho não expulsam o domínio raro), IDF ponto-fixo no fill, par de truncamento com stem igual casa, slices_omitted exposto.
  Command: `cargo test -p scan`
- **AC-2** — Workspace verde (goldens de rt atualizados deliberadamente se a forma/conteúdo do digest mudar).
  Command: `cargo test --workspace`

## Arquivos

- apps/scan/src/digest.rs — seleção de âncoras: fase cobertura (round-robin pelos matched_terms já ordenados tier→raridade, 1 top-file por termo) + fase fill por soma ponderada por IDF ponto-fixo; campo aditivo files_detail (score + termos casados por âncora); slices_omitted espelhando terms_omitted
- apps/scan/src/rank.rs — idf ponto-fixo (constantes como dado em ranking.toml); corrigir o comentário "IDF cancels" (premissa falsa para a soma cross-termo das âncoras)
- apps/scan/ranking.toml — constantes do IDF como dado
- apps/scan/src/matching.rs — aceitar par de truncamento quando os stems da mesma língua coincidem (payables↔payable, morfologia genuína); manter a recusa para prefixo sem respaldo morfológico
- apps/scan/tests/ — regressões (anchor_ranking, match_tiers, fixture sialia-like)
- (fase 2) apps/rt/src/commands/feature.rs + apps/rt/src/commands/spec/spec_draft.rs