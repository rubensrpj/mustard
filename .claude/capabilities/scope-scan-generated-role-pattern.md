---
id: cap.scope-scan-generated-role-pattern
status: active
---

# scope scan generated role pattern

### Requirement: The system SHALL satisfy the acceptance criteria of spec scope-scan-generated-role-pattern.

#### Scenario: AC-1
- when: o censo registra os diretórios de um cluster de papel
- then: a worklist de moldes
- command: `cargo test -p mustard-rt globs_for`

#### Scenario: AC-2
- when: o prompt do papel `patterns` é renderizado
- then: o contrato do molde exige a chave
- command: `cargo test -p mustard-rt patterns_contract_requires_paths`

#### Scenario: AC-3
- when: um molde traz `paths:` no frontmatter
- then: o parser o reconhece como campo tipado
- command: `cargo test -p mustard-core paths_parses_as_a_typed_field`

#### Scenario: AC-4
- when: `scan-patterns-apply` grava um molde cujo corpo autorado traz `paths:`
- then: a
- command: `cargo test -p mustard-rt run_preserves_the_paths_key`

#### Scenario: AC-5
- when: o frontmatter dos comandos do plugin é auditado
- then: os cinco utilitários carregam
- command: `cargo test -p mustard-rt command_frontmatter_`

#### Scenario: AC-6
- when: o prompt do papel `patterns` é renderizado para um subprojeto real deste
- then: 
- command: `mustard-rt run agent-prompt-render --role patterns --subproject rt`

#### Scenario: AC-7
- when: `dependency-precheck` não consegue ler a spec que lhe indicaram
- then: ele responde
- command: `cargo test -p mustard-rt unreadable_spec_is_not_a_pass`

## Covers

## Specs
- [[spec.scope-scan-generated-role-pattern]]

## Related

