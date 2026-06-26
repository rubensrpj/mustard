---
id: wave.ranquear-candidatos-lexicon-enrich-por.3-pt-mining
---

# wave-3-pt-mining

## Resumo

Minerar o vocabulario PT do projeto (specs, commits, comentarios) e alinhar a termos de codigo por co-ocorrencia, surfando pares PT->code rankeados pela especificidade — fecha a direcao que falta (a cobertura alem dos 3).

## Rede

- Pai: [[ranquear-candidatos-lexicon-enrich-por]]
- Depende de: [[wave-1-metric]]

## Tarefas

- [ ] Minerar vocabulario PT de fontes comparaveis, deterministico/offline: textos de specs (.claude/spec/*/spec.md), mensagens de commit (git log) e comentarios/strings PT no codigo. Tokenizar e filtrar stopwords PT (reusar stoplist vendorizada).
- [ ] Alinhar cada termo PT candidato a um termo de codigo por co-ocorrencia/posicao (mesmo modulo/entidade/rota/coluna), sem embeddings — set arithmetic sobre os postings existentes. Ranquear os pares PT->code pela especificidade do alvo (reusa domain_specificity_x1024 da W1).
- [ ] Surfar os pares PT->code (rankeados) como saida nova do enrich (ex.: --check-pt ou campo aditivo) para o orquestrador propor bridges que o fluxo unidirecional nao alcancava. Byte-estavel.

## Arquivos

- `apps/scan/src/digest.rs`
- `apps/rt/src/commands/lexicon_enrich.rs`
