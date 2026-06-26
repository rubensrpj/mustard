---
id: wave.ranquear-candidatos-lexicon-enrich-por.1-metric
---

# wave-1-metric

## Resumo

Metrica de especificidade de dominio (TF-IDF count x idf) em core, exposta como campo aditivo no indice de termos do digest — a fundacao que W2 e W3 consomem.

## Rede

- Pai: [[ranquear-candidatos-lexicon-enrich-por]]

## Tarefas

- [ ] Em packages/core/src/domain/ranking.rs, adicionar fn pura domain_specificity_x1024(count, df, n_docs) = TF-IDF fixed-point: count (saturado) x idf_x1024(df, n_docs), reusando idf_x1024 existente. Byte-estavel, sem float. Documentar que o pico fica no meio da frequencia (demove ubiquo de df alto E hapax de count baixo).
- [ ] Em apps/scan/src/digest.rs build_terms, computar por termo df = per_module.len() e n_docs = total de modulos indexados (c.doc_len.len()), e adicionar campo ADITIVO specificity_x1024 ao TermD via ranking::domain_specificity_x1024. NAO mudar a ordenacao publicada (sort por rank_key permanece) nem o cap MAX_TERMS.
- [ ] Cobrir com testes: termo de df alto (type/response style) recebe specificity baixa; termo de df medio com count razoavel (tenant/category style) recebe alta; hapax (df=1, count baixo) recebe baixa.

## Arquivos

- `packages/core/src/domain/ranking.rs`
- `apps/scan/src/digest.rs`
