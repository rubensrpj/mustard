---
id: wave.digest-concern-split-por-co.2-surface
---

# wave-2-surface

## Resumo

Superficie + contrato: payload() emite os N sub-digests rotulados num payload completo; note() instrui 'analise apenas apos todas as quebras retornarem'.

## Rede

- Pai: [[digest-concern-split-por-co]]
- Depende de: [[wave-1-impl]]

## Tarefas

- [ ] apps/rt/src/commands/feature.rs payload() (128-224): serializar os N sub-digests rotulados num payload COMPLETO (cada concern com seus anchors/anchorsDetail/reason). Manter compatibilidade para concern unico (1 grupo = comportamento atual). Depende do tipo de view de concern do QueryResult (onda 1).
- [ ] apps/rt/src/commands/feature.rs note() (365-394): adicionar a instrucao do contrato 'analise apenas apos TODAS as quebras retornarem' (a IA nao deve processar o concern #1 antes de todos voltarem) nos reasons relevantes.
- [ ] Teste do contrato: note_emits_all_breaks_first_contract (a note emitida contem a instrucao quando ha >=2 sub-digests).

## Arquivos

- `apps/rt/src/commands/feature.rs`
