---
id: cap.guardas-afirmam-mais-que-medem
status: active
---

# guardas afirmam mais que medem

### Requirement: The system SHALL satisfy the acceptance criteria of spec guardas-afirmam-mais-que-medem.

#### Scenario: AC-1
- when: o `Cargo.lock` da raiz não andou mas uma dependência de terceiros está no número alvo
- then: a guarda da terceira perna reprova
- command: `cargo test -p mustard-core --test version_line bump_guard_rejects_a_lock_whose_local_crates_did_not_move 2>&1 | grep -q '1 passed'`

#### Scenario: AC-2
- when: um lock fixa mais de um crate nosso
- then: a guarda confere TODOS eles, e não um nome escolhido à mão
- command: `cargo test -p mustard-core --test version_line bump_guard_checks_every_local_crate_of_each_lock 2>&1 | grep -q '1 passed'`

#### Scenario: AC-3
- when: um crate nosso SOME de um lock
- then: a guarda reprova nomeando qual sumiu, em vez de aprovar o conjunto reduzido que sobrou
- command: `cargo test -p mustard-core --test version_line bump_guard_rejects_a_lock_that_lost_one_of_our_crates 2>&1 | grep -q '1 passed'`

#### Scenario: AC-4
- when: a perna do dev decide pular a propagação
- then: ela consulta as mesmas pernas que o bloco de trabalho conserta
- command: `cargo test -p mustard-core --test version_line dev_leg_decision_consults_what_the_work_block_repairs 2>&1 | grep -q '1 passed'`

#### Scenario: AC-5
- when: um agente declara `model` ou `effort` com comentário ou entre aspas
- then: a catraca lê o valor e o aceita; e continua reprovando valor com sobra depois do id
- command: `cargo test -p mustard-rt --test plugin_agents scalar_ 2>&1 | grep -q '2 passed'`

#### Scenario: AC-6
- when: 
- then: o build do projeto passa verde
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.guardas-afirmam-mais-que-medem]]

## Related

