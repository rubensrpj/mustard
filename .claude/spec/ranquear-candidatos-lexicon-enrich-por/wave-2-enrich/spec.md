---
id: wave.ranquear-candidatos-lexicon-enrich-por.2-enrich
---

# wave-2-enrich

## Resumo

Re-ranquear candidatos do lexicon-enrich --check por especificidade e adicionar o gate de qualidade do alvo no --apply (rejeitar bridge sobre token ubiquo). Ambas as concerns no mesmo arquivo lexicon_enrich.rs.

## Rede

- Pai: [[ranquear-candidatos-lexicon-enrich-por]]
- Depende de: [[wave-1-metric]]

## Tarefas

- [ ] Em apps/rt/src/commands/lexicon_enrich.rs unbridged_terms: ordenar candidatos por specificity_x1024 desc (tie-break estavel por term) ANTES do cap MAX_UNBRIDGED, em vez da ordem de digest.terms — o cap passa a manter a cabeca de dominio e cortar a cauda de plumbing. Corrigir o doc-comment (linhas 49-52) que afirma falsamente 'discriminative rank'.
- [ ] Em apply_report: apos o gate anti-hallucination (target_not_in_model), rejeitar o bridge cujo termo-alvo tem specificity_x1024 abaixo de um piso (reason 'target_too_generic'), reportado em rejected[] no mesmo formato. Piso como constante nomeada (ou knob em ranking.toml).
- [ ] Testes: --check ordena por especificidade; --apply rejeita alvo ubiquo (df alto) com target_too_generic e segue aceitando alvo de dominio.

## Arquivos

- `apps/rt/src/commands/lexicon_enrich.rs`
