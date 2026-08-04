---
id: cap.give-scan-flow-commands-it
status: active
---

# give scan flow commands it

### Requirement: The system SHALL satisfy the acceptance criteria of spec give-scan-flow-commands-it.

#### Scenario: AC-1
- when: `--content @<path>` names a file holding the envelope as raw text
- then: the relay applies its blocks exactly as if they had arrived on stdin
- command: `cargo test -p mustard-rt relay_reads_an_envelope_from_a_file_path 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-2
- when: the file at `@<path>` holds the harness's own shape (a JSON array of `{type, text}` objects) instead of raw text
- then: the envelope is recovered by concatenating the text fields, so no script is needed to unwrap it
- command: `cargo test -p mustard-rt relay_reads_the_harness_json_array_of_text_blocks 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-3
- when: `@<path>` cannot be read
- then: the report names the IO failure and comes back `ok:false`, instead of degrading to an empty envelope that prints `ok:true, blocks:0`
- command: `cargo test -p mustard-rt an_unreadable_content_path_is_reported_never_silently_empty 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-4
- when: `scan-patterns-apply` is given `--content @<path>`
- then: it reads the mold body from that file through the SAME reader the relay uses, so the two commands cannot drift apart
- command: `cargo test -p mustard-rt apply_reads_the_mold_body_from_a_file_path 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-5
- when: the worklist is asked for one subproject
- then: it yields that subproject's entries ONLY, on both the default face and the `--rejected` diagnostic, and an unknown subproject yields an empty list rather than everything
- command: `cargo test -p mustard-rt list_filters_the_worklist_by_subproject 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-6
- when: `scan-patterns-list --rejected --subproject apps/rt` is run against this workspace
- then: exactly one distinct subproject appears in the output, so the convergence check is one command instead of a grouping script
- command: `test "$(cargo run -q -p mustard-rt --bin mustard-rt -- run scan-patterns-list --rejected --subproject apps/rt 2>/dev/null | grep -o '"subproject":"[^"]*"' | sort -u | wc -l)" -eq 1`

#### Scenario: AC-7
- when: the flow text is read
- then: it no longer promises the agent that return size is safe and no longer forbids the temp file, and it teaches the file face plus re-dispatch convergence
- command: `! grep -q 'never worry about its size' apps/rt/src/commands/agent/render/role.rs && ! grep -q 'never via a temp file' plugin/commands/scan.md && grep -q -- '--content @' plugin/commands/scan.md && grep -qi 'converg' plugin/commands/scan.md`

#### Scenario: AC-8
- when: the flow text is read
- then: splitting text INSIDE a block is still forbidden while N relay calls at `=== END ===` boundaries are explicitly permitted
- command: `grep -q 'END ===' plugin/commands/scan.md && ! grep -q 'never one per block' plugin/commands/scan.md`

#### Scenario: AC-9
- when: the file at `@<path>` parses as JSON but yields no demarcated block
- then: the report SAYS so, instead of printing `ok:true, blocks:0` — the same silence AC-3 removes, entering by the other door
- command: `cargo test -p mustard-rt a_json_envelope_with_no_blocks_says_so_instead_of_reporting_zero 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-10
- when: 
- then: the project build and tests pass green
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.give-scan-flow-commands-it]]

## Related

