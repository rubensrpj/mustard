---
id: cap.comandos-pr-passam-por-uma
status: active
---

# comandos pr passam por uma

### Requirement: The system SHALL satisfy the acceptance criteria of spec comandos-pr-passam-por-uma.

#### Scenario: AC-1
- when: 
- then: quando o adaptador recebe uma ref completa do Azure e um nome curto do GitHub, então a porta responde o MESMO nome curto para os dois.
- command: `cargo test -p mustard-rt --lib shared::pr_provider::tests::a_full_ref_and_a_short_name_answer_the_same_branch -- --exact`

#### Scenario: AC-2
- when: 
- then: quando o Azure responde active/completed/abandoned/notSet, então a porta traduz para OPEN/MERGED/CLOSED/OPEN e carrega o mergeStatus verbatim.
- command: `cargo test -p mustard-rt --lib shared::pr_provider::tests::azure_states_map_to_the_canonical_vocabulary -- --exact`

#### Scenario: AC-3
- when: 
- then: quando o provedor em vigor não tem adaptador implementado, então toda operação responde provider-unsupported, nunca um sucesso fingido nem uma ausência medida.
- command: `cargo test -p mustard-rt --lib shared::pr_provider::tests::a_provider_without_an_adapter_refuses_honestly -- --exact`

#### Scenario: AC-4
- when: 
- then: quando pr-open roda, então o relatório nomeia o provedor em vigor e a URL veio do adaptador, não de um gh cru no comando.
- command: `cargo test -p mustard-rt --lib commands::review::pr_publish::tests::pr_open_reports_through_the_port -- --exact`

#### Scenario: AC-5
- when: 
- then: quando a prosa da porta de PR é lida, então nenhuma linha manda rodar gh pr create/edit/ready direto: o caminho é o comando da porta, e uma catraca em teste guarda a regra
- command: `cargo test -p mustard-rt --test pr_prose_door -- --exact`

#### Scenario: AC-6
- when: 
- then: quando a unidade termina, então o workspace inteiro compila.
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.comandos-pr-passam-por-uma]]

## Related

