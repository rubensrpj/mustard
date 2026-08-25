---
id: wave.pergunta-abertura-unidade-pergunta-tipo.2-docs
---

# wave-2-docs

## Summary

A pergunta de abertura vira três campos corrigíveis: base primeiro, tipo com hotfix pinado e campo livre, nome apresentado para confirmar ou corrigir — com catracas que prendem cada lei.

## Network

- Parent: [[spec.pergunta-abertura-unidade-pergunta-tipo]]
- Depends on: [[wave.pergunta-abertura-unidade-pergunta-tipo.1-backend]]

## Tasks

- [ ] Em packages/core/templates/mustard/orchestrator.md, reordenar o bloco-modelo para mostrar `sai de` antes de `tipo`, mantendo a marcação prévia de cada campo (um Enter aceita o padrão).
- [ ] Garantir que a linha `tipo:` traga hotfix entre as sugestões e termine no campo livre, e escrever ao lado do bloco o teto de 4 opções da superfície de pergunta com hotfix pinado — um teto que a prosa não nomeia é um teto que o leitor descobre errando na frente do operador.
- [ ] Escrever, com todas as letras, que os campos são INDEPENDENTES: perguntar as duas coisas juntas nunca significa combiná-las em opções pré-pareadas, porque o produto cartesiano esconde a linha de quem quer hotfix saindo da base comum.
- [ ] Apresentar a linha `branch:` como campo corrigível — sugestão + edição — e não como aviso; dizer que editá-la é editar tipo + nome numa string só, e ensinar a linha de despacho a repassar o sinal do portão da onda 1 quando, e somente quando, o operador tiver corrigido o nome.
- [ ] Replicar a semente byte a byte em .claude/mustard/orchestrator.md — a catraca de cópia entregue exige, e editar só o template deixaria este projeto fazendo a pergunta velha.
- [ ] Em apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs, escrever router_asks_the_base_before_the_type (ordem entre as linhas), router_forbids_pairing_and_pins_hotfix (independência, teto, hotfix), router_offers_the_name_for_correction (o nome é oferecido para correção) e delivered_copy_matches_the_seed_at_the_base_row (a cópia entregue coincide com a semente também na linha `sai de:`).

## Files

- `packages/core/templates/mustard/orchestrator.md`
- `.claude/mustard/orchestrator.md`
- `apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs`
