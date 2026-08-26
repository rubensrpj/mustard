---
id: cap.base-do-branch-escolhida-numa
status: active
---

# base do branch escolhida numa

### Requirement: The system SHALL satisfy the acceptance criteria of spec base-do-branch-escolhida-numa.

#### Scenario: AC-1
- when: a unidade é aberta a partir de um branch que existe no remoto mas não está declarado em `git.flow`
- then: o portão de base aceita a abertura em vez de recusar.
- command: `cargo test -p mustard-rt --lib commands::event::base_gate::tests::accepts_any_real_branch_as_base -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`

#### Scenario: AC-2
- when: um branch que NÃO é o padrão do remoto recebe uma escrita direta
- then: a proteção permite; e quando o branch padrão recebe a mesma escrita, then ela é recusada.
- command: `cargo test -p mustard-rt --lib commands::event::work_branch::tests::only_the_remote_default_branch_is_protected -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`

#### Scenario: AC-3
- when: o operador informa um tipo fora da lista sugerida, por exemplo `chore`
- then: o nome do branch é montado com esse prefixo em vez de ser recusado ou coagido para `feature`.
- command: `cargo test -p mustard-rt --lib shared::work_kind::tests::accepts_a_type_outside_the_suggested_list -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`

#### Scenario: AC-4
- when: o fluxo precisa das bases candidatas
- then: `run base-candidates` busca o remoto e devolve os branches reais ordenados por recência do último commit.
- command: `cargo run -p mustard-rt --quiet -- run base-candidates 2>&1 | grep -q '"branches"'`

#### Scenario: AC-5
- when: o `mustard init` roda em um repositório
- then: ele não pergunta mais qual é o branch de produção nem qual é o de desenvolvimento, e o `mustard.json` que ele grava não contém a chave `git.flow`.
- command: `cargo test -p mustard-cli --lib commands::git_flow::tests::init_does_not_ask_for_branches -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`

#### Scenario: AC-6
- when: uma instalação antiga com git.flow preenchido abre uma unidade a partir de um branch que o flow NÃO declara
- then: o portão de base aceita a abertura e a base declarada aparece apenas como pré-seleção
- command: `cargo test -p mustard-rt --lib commands::event::base_gate::tests::a_declared_flow_preselects_without_refusing_others -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`

#### Scenario: AC-7
- when: 
- then: o build do projeto passa verde
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.base-do-branch-escolhida-numa]]

## Related

