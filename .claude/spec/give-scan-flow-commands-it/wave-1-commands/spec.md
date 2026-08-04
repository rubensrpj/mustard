---
id: wave.give-scan-flow-commands-it.1-commands
---

# wave-1-commands

## Summary

Give the two envelope-consuming commands a file face through ONE shared reader, and expose the subproject filter the worklist already computes.

## Network

- Parent: [[spec.give-scan-flow-commands-it]]

## Tasks

- [ ] Add ONE shared envelope reader to apps/rt/src/commands/scan_patterns/mod.rs. It resolves THREE channels: `-` reads stdin, `@<path>` reads that file, anything else is the literal text. `@` counts as a path ONLY when the value carries no newline — a literal envelope always spans lines, so the ambiguity is unreachable.
- [ ] CHANNEL PARITY IS A HARD CONSTRAINT. Only the NEW `@<path>` channel may report an IO failure. The stdin channel keeps its current fail-open behaviour byte for byte: a failed stdin read still degrades to an empty string and the caller still proceeds. stdin is how every dispatch that works today reaches the relay; changing it under cover of an additive feature is the one regression this wave must not ship. Make the reader's return type carry `read this text` vs `this PATH could not be read` — never `this stdin could not be read`.
- [ ] The persisted file has two shapes and the harness owns both, so do not pin one. If the content parses as JSON, harvest every `text` field at ANY depth, in document order, and join them — that covers a bare array of `{type, text}` and any variant that nests the same objects, without inventing a format. If it does not parse as JSON, it is raw text.
- [ ] Close the silent-zero hole. If the content DID parse as JSON but the harvested envelope yields no demarcated block, the report must SAY so rather than print `ok:true, blocks:0` — that is the same silence AC-3 removes, entering by the other door. A blockless RAW-TEXT envelope keeps its existing behaviour (empty report, exit 0); the existing test `a_blockless_envelope_reports_empty_and_never_errors` must stay green.
- [ ] Delete the local `resolve_content` from relay.rs and call the shared reader. On an unreadable `@<path>`, push a `skipped` entry naming the IO error so the report comes back `ok:false`.
- [ ] Delete the local `resolve_content` from apply.rs and call the same shared reader; keep ONLY apply's own trim-to-None on top of it. No facade, no wrapper — the caller calls the core. `resolve_content_blanks_are_none` must stay green.
- [ ] Move `normalize_subproject` out of agent/render/role.rs into scan_patterns/list.rs and make it crate-visible; role.rs imports it from its new home. Normalising a subproject path is the worklist's concern, and role.rs is a consumer of the filter, not its owner. Do NOT touch the prompt text in role.rs — that belongs to wave 2.
- [ ] Add an optional subproject filter to list.rs `run` that applies to BOTH faces — the default worklist and the `--rejected` diagnostic — using the same normalisation. An unknown subproject yields an empty list, never everything.
- [ ] Register `--subproject <dir>` on `ScanPatternsList` in scan_cli.rs and document the `@<path>` form on BOTH `--content` flags (relay and apply). Update the dispatch arms. Remember the four registrations a scan-family change needs.
- [ ] Tests, named exactly as the acceptance criteria name them: relay_reads_an_envelope_from_a_file_path, relay_reads_the_harness_json_array_of_text_blocks, an_unreadable_content_path_is_reported_never_silently_empty, a_json_envelope_with_no_blocks_says_so_instead_of_reporting_zero, apply_reads_the_mold_body_from_a_file_path, list_filters_the_worklist_by_subproject.

## Files

- `apps/rt/src/commands/scan_patterns/mod.rs`
- `apps/rt/src/commands/scan_patterns/relay.rs`
- `apps/rt/src/commands/scan_patterns/apply.rs`
- `apps/rt/src/commands/scan_patterns/list.rs`
- `apps/rt/src/commands/scan_cli.rs`
- `apps/rt/src/commands/agent/render/role.rs`

## Reality Obligations

- **RO-1.1** — The exact on-disk shape of a harness-persisted subagent return is NOT knowable from this repository — it is written by the Claude Code harness, not by this code. The field report measured it as a JSON array of `{type, text}` objects (3 occurrences: 73 KB, 60 KB, 78 KB). Confirm that shape against a real persisted return if one is reachable; if it is not, say so plainly in your report. Either way the parser must harvest by FIELD NAME at any depth rather than match one exact shape, so an unseen variant degrades to a named report entry and never to a silent zero.
