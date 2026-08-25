---
id: cap.aprovacao-moldes-padrao
status: active
---

# aprovacao moldes padrao

### Requirement: The system SHALL satisfy the acceptance criteria of spec aprovacao-moldes-padrao.

#### Scenario: AC-1
- when: 
- then: quando o palpite mais recente de .pipeline-states/ nomeia uma spec que NAO esta na janela full+Plan e existe um unico plano full+Plan sem aprovar, a resolucao entrega o plano pendente e a aprovacao e cunhada.
- command: `cargo test -p mustard-rt a_stale_hint_never_shadows_the_pending_full_plan 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-2
- when: 
- then: quando o operador seleciona uma opcao de aprovacao oferecida e o fato 1 e que recusa, o observador nomeia em stderr qual condicao falhou, em vez de sair calado.
- command: `cargo test -p mustard-rt a_fact_one_decline_names_its_reason 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-3
- when: 
- then: quando o prompt inteiro e /mustard:spec r e o checkout E o branch da unidade cujo plano full esta em Plan sem aprovar, o marcador .approved-by-user e cunhado; fora desse branch, nada e cunhado.
- command: `cargo test -p mustard-rt a_bare_r_inside_the_units_branch_mints_the_marker 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-4
- when: 
- then: o worklist entregue ao autor mostra paths como o bloco YAML que o molde deve carregar, e o valor copiado ao pe da letra dele passa no validador.
- command: `cargo test -p mustard-rt the_worklist_prints_paths_as_the_yaml_the_mold_must_carry 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-5
- when: 
- then: um molde que declara paths na forma inline e aceito, e o arquivo gravado carrega paths em lista em bloco.
- command: `cargo test -p mustard-rt an_inline_paths_value_is_accepted_and_written_as_a_list 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-6
- when: 
- then: um molde cujos quatro titulos faltem, dupliquem ou estejam fora de ordem e recusado sem ser escrito, e um molde com os quatro na ordem certa e aceito.
- command: `cargo test -p mustard-rt a_mold_whose_headings_are_wrong_is_refused 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-7
- when: 
- then: um arquivo de envelope que foi lido, nao e JSON e nao demarca nenhum bloco volta ok:false nomeando o arquivo, nunca ok:true blocks:0.
- command: `cargo test -p mustard-rt a_read_file_that_demarcates_nothing_is_never_a_silent_ok 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-8
- when: 
- then: resume-loop secao A deixa de prometer que a resposta ao modal cunha o marcador sem dizer o que conta, e commands/spec.md registra a forma /mustard:spec r.
- command: `! grep -q 'the answer mints the same marker' plugin/refs/spec/resume-loop.md && grep -q '/mustard:spec r' plugin/commands/spec.md`

#### Scenario: AC-9
- when: 
- then: a recusa do approve-spec por marcador ausente nomeia os tres gestos que cunham, inclusive a forma digitada.
- command: `cargo test -p mustard-rt the_refusal_names_the_gestures_that_actually_mint 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-10
- when: 
- then: a arvore compila inteira depois das tres ondas.
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.aprovacao-moldes-padrao]]

## Related

