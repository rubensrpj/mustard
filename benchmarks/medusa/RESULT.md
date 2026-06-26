# Medusa order module — benchmark de recall por intenção (cross-lingual PT→EN)

Repo OSS público (medusajs/medusa, módulo `packages/modules/order`, 294 métodos de lógica), código 100% inglês, queries em pt-BR. Prova o fosso: achar código EN pela INTENÇÃO em outra língua, onde o match-por-nome dá zero.

## Resultado (2026-06-25)

| retrieval | recall |
|---|---|
| name-match (digest) | **0/12** |
| purpose-search (IDF) | **9/12 @1 · 10/12 @5 · 11/12 @12** |

## Como reproduzir
1. `git clone --depth 1 https://github.com/medusajs/medusa` ; `mkdir packages/modules/order/.claude`
2. `mustard-rt run scan --root packages/modules/order`
3. Enriquecer purposes em pt-BR (Sonnet lendo o corpo, BLIND às queries — render→dispatch→apply). ~$1.
4. `mustard-rt run recall-bench --labels labels.ndjson --model packages/modules/order/.claude/grain.model.json`

## Ressalvas honestas
- Os 12 GT estão concentrados em `src/services/order-module-service.ts` (god-service da Medusa) — spread fraco; v2 deve usar arquivos por-ação distintos. name-match ainda é limpo 0/12.
- 1 miss (`estornar pagamento`→`deleteOrderTransactions`): resíduo de sinônimo puro (sumário não usou "estornar") — fecha com ponte de léxico.
- Sumarizador SEMPRE cego às queries (sem lista de verbos) — invariante de validade.

## v2 — spread RESOLVIDO (arquivos por-ação distintos), 2026-06-25

`labels-v2-distinct-files.ndjson`: 11 casos, cada GT num arquivo DISTINTO (`src/services/actions/{cancel-return,register-fulfillment,...}.ts`) — resolve a ressalva do god-service.

| retrieval | @1 | @5 |
|---|---|---|
| name-match | 0.09 | **0.36** |
| purpose-search | 0.55 | **0.91** |

Enquadramento honesto: aqui o name-match NÃO é 0 porque os arquivos por-ação têm o VERBO no nome (`cancel-return`) e `cancelar`≈cancel é COGNATO. Mas o name não desambigua o SUBSTANTIVO (devolução/reclamação/troca, todos em PT) → ranqueia errado (cancelar-devolução cai em name#4 entre os cancel-*). O purpose, com verbo+substantivo PT, acha e desambigua (@5 0.91). **O recall-hole real é o substantivo cross-lingual; o ganho do fosso = 0.36→0.91 @5 num benchmark com spread justo.** (1 miss: "separar para expedição" — sinônimo que o sumário não usou.)

## v3 — "tudo em inglês" (sem ponte de léxico), 2026-06-25

Mudança de arquitetura (spec `english-canonical-retrieval`): o tier-4 lexicon (ponte PT→EN) foi **removido**; o motor de busca é inglês-intra-língua e a **query chega traduzida para inglês** (o chamador/LLM traduz). `labels-v2-en.ndjson` = as 11 queries da v2 traduzidas para a intenção em inglês natural (mapeando o significado, não o nome do arquivo).

| caminho | retrieval | @1 | @5 | combinado@5 (name OU purpose) |
|---|---|---|---|---|
| PT antigo (com ponte tier-4) | name / purpose | 0.09 / 0.55 | 0.36 / 0.91 | ~0.91 |
| **EN novo (sem ponte)** | name / purpose | 0.64 / 0.27 | **0.82 / 0.73** | **1.0 (11/11)** |

Leitura honesta:
- **name-match quase dobra (0.36→0.82 @5)**: a query inglesa bate direto no nome inglês do método — o cognato deixa de ser sorte e o substantivo deixa de estar em outra língua.
- **purpose vira a rede secundária**: pega os 2 casos onde o nome diverge (`marcar entregue`→`registerDelivery` name#None→purpose#1; `abrir reclamação`→`createClaim` name#6→purpose#3). Os dois sinais são **complementares** (desenho recall/precisão).
- **combinado@5 = 1.0**: todo alvo sobrevive ao corte no top-5 de name OU purpose — que é o que o juiz Sonnet vê no sistema real. A deleção da ponte é um **ganho** de recall, não regressão.
- purpose@5 isolado (0.73 < 0.91 PT) é parcialmente artefato de só 80/294 purposes terem sido traduzidos para inglês neste teste; um re-enrich inglês completo deve elevá-lo. Mas o nome já resolve a maioria, então o purpose-search opera como safety-net, seu papel correto.
- recall-neutralidade mesma-língua **provada**: PT-query+PT-purpose após a deleção reproduz o baseline byte-a-byte (0.36/0.91) — o tier-4 só somava casamento cross-língua, nunca o intra-língua.

`labels-v2-distinct-files.ndjson` (PT) fica como prova-do-fosso histórica; `labels-v2-en.ndjson` é o contrato do caminho novo.
