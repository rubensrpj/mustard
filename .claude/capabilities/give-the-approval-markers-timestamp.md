---
id: cap.give-the-approval-markers-timestamp
status: active
---

# give the approval markers timestamp

### Requirement: The system SHALL satisfy the acceptance criteria of spec give-the-approval-markers-timestamp.

#### Scenario: AC-1
- when: qualquer uma das três portas grava seu marcador
- then: o corpo sai da mesma função
- command: `cargo test -p mustard-rt marker_body_is_the_single_writer`

#### Scenario: AC-2
- when: um marcador gravado é lido de volta
- then: a proveniência volta em campos tipados,
- command: `cargo test -p mustard-rt read_marker_provenance_round_trips_and_degrades`

#### Scenario: AC-3
- when: o `approve-spec` aprova uma spec
- then: ele ecoa por qual porta e quando a
- command: `cargo test -p mustard-rt approve_spec_echoes_provenance`

#### Scenario: AC-4
- when: o marcador existe mas seu corpo está ilegível
- then: o `approve-spec` continua
- command: `cargo test -p mustard-rt unreadable_marker_body_still_approves`

#### Scenario: AC-5
- when: o `/status` mostra uma spec aprovada
- then: a linha traz a porta e a data
- command: `cargo test -p mustard-rt status_shows_approval_provenance`

## Covers

## Specs
- [[spec.give-the-approval-markers-timestamp]]

## Related

