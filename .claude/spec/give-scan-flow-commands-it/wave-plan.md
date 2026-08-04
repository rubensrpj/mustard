---
id: wave.give-scan-flow-commands-it.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.give-scan-flow-commands-it.1-commands]] | commands | — | Give the two envelope-consuming commands a file face through ONE shared reader, and expose the subproject filter the worklist already computes. |
| 2 | [[wave.give-scan-flow-commands-it.2-docs]] | docs | [[wave.give-scan-flow-commands-it.1-commands]] | Re-aim what the flow TEACHES: stop promising the channel is size-safe, stop forbidding the file the harness itself writes, permit N relay calls at block boundaries, and document how a truncated return converges. |

## Acceptance Criteria
- AC-1 — `--content @<path>` with raw text applies exactly as stdin would. Command: `cargo test -p mustard-rt relay_reads_an_envelope_from_a_file_path 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-2 — a file holding the harness JSON array of {type,text} is unwrapped by concatenating text. Command: `cargo test -p mustard-rt relay_reads_the_harness_json_array_of_text_blocks 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-3 — an unreadable path is reported ok:false, never an empty envelope. Command: `cargo test -p mustard-rt an_unreadable_content_path_is_reported_never_silently_empty 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-4 — the apply reads a mold body from a path through the SAME reader. Command: `cargo test -p mustard-rt apply_reads_the_mold_body_from_a_file_path 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-5 — the worklist filters to one subproject on both faces. Command: `cargo test -p mustard-rt list_filters_the_worklist_by_subproject 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-6 — the CLI applies the filter for real. Command: `test "$(cargo run -q -p mustard-rt --bin mustard-rt -- run scan-patterns-list --rejected --subproject apps/rt 2>/dev/null | grep -o '"subproject":"[^"]*"' | sort -u | wc -l)" -eq 1`
- AC-9 — a JSON file that yields no block says so instead of reporting zero. Command: `cargo test -p mustard-rt a_json_envelope_with_no_blocks_says_so_instead_of_reporting_zero 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-10 — the workspace builds green. Command: `cargo build --workspace`
- AC-7 — the flow no longer promises size safety nor forbids the temp file, and teaches the file face plus convergence. Command: `! grep -q 'never worry about its size' apps/rt/src/commands/agent/render/role.rs && ! grep -q 'never via a temp file' plugin/commands/scan.md && grep -q -- '--content @' plugin/commands/scan.md && grep -qi 'converg' plugin/commands/scan.md`
- AC-8 — splitting inside a block stays forbidden while N calls at `=== END ===` boundaries are permitted. Command: `grep -q 'END ===' plugin/commands/scan.md && ! grep -q 'never one per block' plugin/commands/scan.md`
