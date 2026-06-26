---
id: wave.digest-concern-split-por-co.plan
---

# Plano de Waves

## Tabela de Waves

| Wave | Spec | Papel | Depende de | Resumo |
|------|------|-------|------------|--------|
| 1 | [[wave-1-impl]] | impl | — | Motor: QueryResult carrega N sub-digests rotulados; query() agrupa conceitos em concerns por co-ocorrencia e pontua cada cluster com o BM25F existente. |
| 2 | [[wave-2-surface]] | surface | [[wave-1-impl]] | Superficie + contrato: payload() emite os N sub-digests rotulados num payload completo; note() instrui 'analise apenas apos todas as quebras retornarem'. |

## Critérios de Aceitação
- **AC-1** — Build do pipeline verde. Command: `cargo build`
- **AC-2** — Suite completa verde (sem regressao). Command: `cargo test`
- **AC-3** — Consulta de concern unico produz exatamente 1 cluster (sem over-split). Command: `cargo test cooccurrence_single_concern_one_cluster`
- **AC-4** — Conceitos disjuntos quebram em >=2 sub-digests. Command: `cargo test cooccurrence_disjoint_concepts_split`
- **AC-5** — Numa fixture multi-concern, cada concern recupera seu alvo no seu sub-digest. Command: `cargo test concern_split_target_surfaces_per_concern`
- **AC-6** — O note() emitido carrega o contrato 'analise apenas apos todas as quebras retornarem'. Command: `cargo test note_emits_all_breaks_first_contract`
