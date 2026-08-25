---
id: wave.pergunta-abertura-unidade-pergunta-tipo.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.pergunta-abertura-unidade-pergunta-tipo.1-backend]] | backend | — | O portão passa a aceitar um nome escolhido pelo operador, por um sinal explícito que ganha da derivação — sem afrouxar a lei de um nome só. |
| 2 | [[wave.pergunta-abertura-unidade-pergunta-tipo.2-docs]] | docs | [[wave.pergunta-abertura-unidade-pergunta-tipo.1-backend]] | A pergunta de abertura vira três campos corrigíveis: base primeiro, tipo com hotfix pinado e campo livre, nome apresentado para confirmar ou corrigir — com catracas que prendem cada lei. |

## Acceptance Criteria
- AC-4 — when o portão recebe um nome escolhido pelo operador pelo sinal explícito de renomeação, then é esse nome que nomeia a branch, os eventos e o diretório da spec, e o relatório registra de onde ele veio; um --spec comum continua perdendo para a derivação.
Command: `cargo test -p mustard-rt operator_name_wins_over_the_derivation`
Expect: `1 passed`
- AC-1 — when o bloco-modelo do roteador é lido, then a linha `sai de:` aparece antes da linha `tipo:` e a linha `tipo:` contém hotfix.
Command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour router_asks_the_base_before_the_type`
Expect: `1 passed`
- AC-2 — when a regra ao lado do bloco é lida, then ela nomeia o teto de opções da superfície, proíbe parear os campos e prende hotfix na lista.
Command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour router_forbids_pairing_and_pins_hotfix`
Expect: `1 passed`
- AC-3 — when a cópia entregue é comparada com a semente compilada, then as duas coincidem também na linha `sai de:`.
Command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour delivered_copy_matches_the_seed_at_the_base_row`
Expect: `1 passed`
- AC-5 — when o bloco-modelo é lido, then a linha `branch:` se apresenta como campo corrigível (sugestão + edição) e não como aviso.
Command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour router_offers_the_name_for_correction`
Expect: `1 passed`
- AC-6 — quando os injetaveis do roteador sao medidos, entao cada arquivo cabe embutido no contexto sem virar arquivo com previa — a secao ## Dispatch mora num injetavel proprio, pendurado em sessionStart, e o restante do roteador segue em userPromptSubmit
Command: `cargo test -p mustard-cli --test template_budget` Expect: `2 passed`
