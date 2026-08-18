---
id: cap.fix-linux-install-docs-make
status: active
---

# fix linux install docs make

### Requirement: The system SHALL satisfy the acceptance criteria of spec fix-linux-install-docs-make.

#### Scenario: AC-1
- when: the installer is piped into `sh` with no package beside it
- then: 
- command: `bash -c "tr -d '\r' < packaging/installer/install.sh | sh -s -- --dry-run"`

#### Scenario: AC-2
- when: the four install texts are checked
- then: each one carries the
- command: `bash -c "grep -q 'releases/latest/download/install' packaging/installer/TUTORIAL-LINUX.md && grep -q 'releases/latest/download/install' packaging/installer/RELEASE-BODY.md && grep -q 'releases/latest/download/install' README.md && grep -q 'releases/latest/download/install' README.en.md && grep -q 'plugin marketplace add rubensrpj/mustard' packaging/installer/TUTORIAL-LINUX.md && grep -q 'plugin marketplace add rubensrpj/mustard' README.md && grep -q 'plugin marketplace add rubensrpj/mustard' README.en.md"`

#### Scenario: AC-3
- when: 
- then: the project build passes green
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.fix-linux-install-docs-make]]

## Related

