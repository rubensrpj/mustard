---
id: cap.harness-ve-toda-branch-trabalho
status: active
---

# harness ve toda branch trabalho

### Requirement: The system SHALL satisfy the acceptance criteria of spec harness-ve-toda-branch-trabalho.

#### Scenario: AC-1
- when: o enumerador é consultado num repositório com branches locais e branches que existem só no remoto
- then: ele devolve as duas famílias, filtradas por prefixo de base, e uma base sem underscore nunca entra no resultado
- command: `cargo test -p mustard-rt branch_enumerator_sees_local_and_remote_refs`

#### Scenario: AC-2
- when: o ritual de saída e o inventário de specs precisam saber quais branches existem
- then: ambos consultam o mesmo enumerador, e nenhuma das duas varreduras anteriores sobrevive
- command: `cargo test -p mustard-rt settle_and_active_specs_share_one_enumerator`

#### Scenario: AC-3
- when: uma unidade foi mergeada mas cortada in-place, sem worktree
- then: ela aparece na lista de pendentes de poda, que hoje responde vazio nesse caso
- command: `cargo test -p mustard-rt in_place_merged_unit_is_reported_pending`

#### Scenario: AC-4
- when: o classificador recebe uma branch cuja remota desapareceu mas cujo merge não foi verificado
- then: ele a marca como perigo e nunca como pendente de poda
- command: `cargo test -p mustard-rt gone_alone_never_authorises_deletion`

#### Scenario: AC-5
- when: o CLI do provedor está ausente ou não autenticado
- then: a coluna de PR responde desconhecido com o motivo, jamais sem-PR
- command: `cargo test -p mustard-rt absent_provider_answers_unknown_never_absent`

#### Scenario: AC-6
- when: o módulo de relatório é compilado
- then: ele não alcança nenhuma função de exclusão de branch — a segurança da fase é estrutural, não disciplinar
- command: `cargo test -p mustard-rt report_module_cannot_reach_deletion`

#### Scenario: AC-7
- when: uma spec existe apenas numa branch remota
- then: o inventário de specs a lista e nomeia onde ela vive
- command: `cargo test -p mustard-rt active_specs_sees_a_spec_on_a_remote_only_branch`

#### Scenario: AC-8
- when: há unidades devendo poda
- then: a statusline informa a contagem, na língua configurada do projeto e sem nome de base literal no código
- command: `cargo test -p mustard-rt statusline_names_units_awaiting_prune`

#### Scenario: AC-9
- when: o ritual de saída documenta por que consulta o provedor
- then: a prosa não afirma um método de merge que ninguém mediu
- command: `cargo test -p mustard-rt settle_doc_states_no_unmeasured_merge_method`

#### Scenario: AC-11
- when: uma branch de trabalho nao tem nenhum commit a frente da sua base
- then: ela nunca e classificada como pendente de poda — cortar a branch nao e entregar trabalho, e o portao de branch corta toda unidade nova exatamente nessa forma
- command: `cargo test -p mustard-rt a_branch_with_no_commits_ahead_is_never_awaiting_prune`

#### Scenario: AC-12
- when: uma unidade cujo merge foi verificado perdeu a ref local mas a remota segue viva
- then: ela entra na lista de pendentes de poda em vez de sair apenas como so-no-remoto
- command: `cargo test -p mustard-rt merged_unit_alive_only_on_the_remote_is_awaiting_prune`

#### Scenario: AC-10
- when: 
- then: o build e os testes do projeto passam verdes
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.harness-ve-toda-branch-trabalho]]

## Related

