---
id: wave.digest-concern-split-por-co.1-impl
---

# wave-1-impl

## Resumo

Motor: QueryResult carrega N sub-digests rotulados; query() agrupa conceitos em concerns por co-ocorrencia e pontua cada cluster com o BM25F existente.

## Rede

- Pai: [[digest-concern-split-por-co]]

## Tarefas

- [ ] packages/core/src/domain/scan.rs: estender QueryResult (hoje files/files_detail planos, digest.rs:154-200) com um tipo de view de agrupamento por concern — uma lista de sub-digests rotulados; cada concern carrega seus proprios files/files_detail/reason. Tipo puro serde, sem logica.
- [ ] apps/scan/src/rank.rs: helper novo de componentes conexos (union-find) sobre os conceitos da query; nao existe util de connected-components hoje. Entrada: indices dos conceitos + pares que co-ocorrem; saida: vetor de componentes (cada componente = lista de indices de conceito).
- [ ] apps/scan/src/digest.rs query() (255-462): apos montar os QConcept (382-417), construir o grafo de co-ocorrencia dos conceitos (aresta c1-c2 sse a intersecao dos conjuntos de modulos em Corpus::postings (696) for nao-vazia; ponte opcional via Corpus::imports (708)); componentes conexos = concerns; para cada cluster rodar a pontuacao BM25F existente (bm25f_contribution_x1024 + stratified_order) restrita aos seus conceitos; montar os N sub-digests rotulados. INVARIANTE: query de concern unico -> exatamente 1 cluster, ranking identico ao de hoje (zero regressao em queries strong).
- [ ] Testes unitarios no scan: cooccurrence_single_concern_one_cluster (1 cluster), cooccurrence_disjoint_concepts_split (>=2 clusters), concern_split_target_surfaces_per_concern (fixture multi-concern: cada concern recupera seu alvo no seu sub-digest).

## Arquivos

- `packages/core/src/domain/scan.rs`
- `apps/scan/src/rank.rs`
- `apps/scan/src/digest.rs`
