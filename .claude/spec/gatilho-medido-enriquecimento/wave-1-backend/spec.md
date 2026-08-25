---
id: wave.gatilho-medido-enriquecimento.1-backend
---

# wave-1-backend

## Summary

o portao de base passa a medir a lacuna do enriquecimento e a reportar em stderr, e o comentario deixa de citar a porta selada

## Network

- Parent: [[spec.gatilho-medido-enriquecimento]]

## Tasks

- [ ] Criar `apps/rt/src/commands/event/enrichment_gap.rs`: o tipo `EnrichmentGap { pending_guards: Vec<String>, missing_molds: Vec<String> }` com `is_empty()`, a funcao PURA `measure(project: &Path) -> EnrichmentGap` e, separada dela, o reporter que produz o efeito. A separacao entre decidir e imprimir espelha `census_refresh_due` vs `refresh_census_if_stale` no modulo irmao `base_gate.rs`, e e o que torna a decisao testavel sem efeito.
- [ ] A medida NAO abre travessia nova: os subprojetos com `## Guards` ainda em esqueleto vem de `crate::commands::scan_guards::list::collect_pending` (a mesma travessia unica que o doctor ja reusa), e os moldes sem autor vem de `crate::commands::scan_patterns::list::collect`, que ja exclui molde presente no disco e slug declinado. Uma terceira copia da travessia divergiria em silencio das outras duas.
- [ ] Exportar o literal do sinal como constante do crate (`ENRICHMENT_STALE_TAG`, valor `base-gate: enrichment stale`), para que a prosa semeada e o codigo possam ser travados no MESMO texto pelo teste da onda 2. Um literal digitado duas vezes e o que permite as duas metades divergirem.
- [ ] O reporter imprime UMA linha em stderr quando a lacuna nao e vazia, nomeando a contagem, alguns slugs e o fato de que fechar isso e unidade PROPRIA em arvore limpa, a ser aberta depois que a corrente fechar. stderr, jamais stdout: a unica linha JSON do `emit-pipeline` e comparada byte a byte por gates, e o aviso de refresh do censo ja usa stderr exatamente por esse motivo.
- [ ] Registrar o modulo em `apps/rt/src/commands/event/mod.rs`.
- [ ] Chamar o reporter em `apps/rt/src/commands/event/emit_pipeline.rs`, dentro do braco `BaseVerdict::Open`, logo depois de `refresh_census_if_stale`. Nao tocar em `Abstain` (portao nao mediu, entao nao tem o que reportar) nem em `Refuse` (nada e escrito antes da recusa).
- [ ] Corrigir o comentario de `apps/rt/src/commands/event/base_gate.rs` que ainda diz que o passe completo `stays with the explicit /scan, which is where a human reviews that much rewriting`: essa porta foi selada em 03/08/2026. O texto passa a dizer que o passe completo fica com o FLUXO, despachado como unidade propria quando este portao reporta a lacuna.
- [ ] Testes `#[cfg(test)]` no modulo novo: `counts_molds_with_no_author` (molde candidato sem autor entra na contagem), `names_a_subproject_whose_guards_are_still_a_scaffold` (subprojeto com Guards em esqueleto e nomeado) e `no_census_means_an_empty_gap` (sem `grain.model.json` a lacuna volta vazia, em silencio, sem panico). Fail-open em todo passo: diretorio ilegivel e lacuna vazia, nunca erro.

## Files

- `apps/rt/src/commands/event/enrichment_gap.rs`
- `apps/rt/src/commands/event/mod.rs`
- `apps/rt/src/commands/event/emit_pipeline.rs`
- `apps/rt/src/commands/event/base_gate.rs`
