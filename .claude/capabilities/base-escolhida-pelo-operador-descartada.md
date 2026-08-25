---
id: cap.base-escolhida-pelo-operador-descartada
status: active
---

# base escolhida pelo operador descartada

### Requirement: The system SHALL satisfy the acceptance criteria of spec base-escolhida-pelo-operador-descartada.

#### Scenario: AC-1
- when: 
- then: quando o operador escolhe uma base que existe no remoto, entao o branch e cortado DESSA base em QUALQUER projeto — inclusive num que declare exatamente uma base, ou nenhuma; a pergunta houve escolha? e respondida pelo catalogo real e nao pela contagem da lista declarada
- command: `cargo test -p mustard-rt the_recorded_base_survives_to_the_cut_in_any_project`

#### Scenario: AC-2
- when: 
- then: quando a base anotada não existe mais no remoto, então ela é ignorada e a
- command: `cargo test -p mustard-rt a_vanished_recorded_base_is_ignored`

#### Scenario: AC-3
- when: 
- then: quando o projeto declara uma base cujo nome contem barra (release/2026-Q3), entao git delete RECUSA apaga-la, e pr list e git delete funcionam estando sobre ela — o teste roda o comando e observa o efeito, nao procura texto no codigo-fonte
- command: `cargo test -p mustard-rt a_slashed_integration_base_is_never_deleted_and_never_refused`

#### Scenario: AC-4
- when: 
- then: quando o diagnóstico roda num projeto sem lista de configuração, então ele
- command: `cargo test -p mustard-rt doctor_does_not_ask_for_a_flow_that_the_installer_no_longer_writes`

#### Scenario: AC-5
- when: 
- then: quando a referência que `/git` manda ler é lida, então ela ensina o modelo
- command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour the_git_reference_teaches_the_measured_model`

#### Scenario: AC-6
- when: 
- then: a suíte do projeto passa inteira
- command: `cargo test --workspace`

## Covers

## Specs
- [[spec.base-escolhida-pelo-operador-descartada]]

## Related

