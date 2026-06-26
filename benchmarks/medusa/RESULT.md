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
