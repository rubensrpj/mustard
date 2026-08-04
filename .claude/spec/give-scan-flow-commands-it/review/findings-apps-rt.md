# Review — apps/rt — give-scan-flow-commands-it

Verdict: approved · critical: 0

## Build / suite
- `cargo build --workspace` → 0 errors, 2 warnings (both pre-existing: `feature.rs:488`, `branch_state.rs:308`).
- `cargo test --workspace` → 4707 passed, 0 failed, 6 ignored (70 suites, 294s) — `run_command_surface` and `template_parity` (the reverse ratchet) included.
- `cargo clippy --workspace --all-targets` → 0 errors (the `unwrap_used`/`expect_used` deny guard holds; new code uses `ok()?`, `map_or`, let-else, `unwrap_or_default`).

## AC-by-AC (each test name matched exactly 1 test, `1 passed`)
| AC | Command | Control |
|---|---|---|
| 1 | `relay_reads_an_envelope_from_a_file_path` PASS | `a_backticked_path…` PASS |
| 2 | `relay_reads_the_harness_json_array_of_text_blocks` PASS | `an_envelope_of_many_blocks…` PASS |
| 3 | `an_unreadable_content_path_is_reported…` PASS | `a_blockless_envelope_reports_empty…` PASS |
| 4 | `apply_reads_the_mold_body_from_a_file_path` PASS | `resolve_content_blanks_are_none` PASS |
| 5 | `list_filters_the_worklist_by_subproject` PASS | `collect_proposes_mold_for_a_real_cluster` PASS |
| 6 | shell gate PASS (1 distinct subproject) | PASS (>=2) |
| 7 | grep gate PASS | PASS |
| 8 | grep gate PASS | PASS |
| 9 | `a_json_envelope_with_no_blocks_says_so…` PASS | `the_report_is_byte_stable` PASS |
| 10 | build green | — |

## Independent proof (not the implementer's tests)
Found a real persisted return under `~/.claude/projects/C--Atiz-mustard/<session>/tool-results/toolu_*.json` — its shape is exactly the claimed pretty-printed `[{type,text},…]` closed by the `agentId`/`<usage>` trailer. Ran the built binary against it into a throwaway root:
`mustard-rt run scan-patterns-relay --root <tmp> --content @<that file>` → `"blocks": 13`, `declined:["rt-file"]`, 12 mold blocks routed. The file's own prose says "Twelve molds below, one decline" — the unwrap is correct and the usage trailer added no block. Also verified `--subproject apps/nope` → `[]` (not everything) and no-flag output unchanged.

## Mold contract
Only `rt-cmd-pattern` (`paths: apps/rt/src/commands/**`) covers a touched file. `scan_cli.rs` conforms: named struct field with `#[arg(long)]` + doc-comment help, `Option<String>` for the optional input, dispatch arm threading `subproject.as_deref()`. No new `run` subcommand, so the four-registration guard is not triggered. `rt-entry-pattern` (same glob) is for `*Entry` rows — none created. `rt-report/result/verdict/outcome/item` all scope to `doctor|maint|pipeline|review|event` — `scan_patterns/**` is outside them, so the new `Envelope` enum falls under no mold.

## Change requests
All five are Plan/Execute-phase steering. The first ("melhor análise sem atrapalhar o que já existe") is answered structurally — stdin byte-for-byte untouched, default list output unchanged — and is carried by AC-1's/AC-9's controls; the second ("fale sem gíria") by the spec's Definitions section. None silently dropped.

## Findings (all minor, none blocking)

1. `apps/rt/src/commands/scan_patterns/apply.rs:129` — an unreadable `@path` prints the IO error, then `unwrap_or_default()` sends `""` into `apply_one`, so a second line `empty mold body — nothing to write` follows. Verified live. The failure IS named first, so AC-4 and the decision hold; the trailing line is misleading noise.
2. `apps/rt/src/commands/scan_patterns/relay.rs:51` — a `@path` that reads as RAW (non-JSON) prose and demarcates nothing still prints `ok:true, blocks:0`. Deliberate and documented; AC-9 scopes only the JSON door. Residual, not a defect against the spec.
3. `apps/rt/src/commands/scan_patterns/mod.rs:84` — the IO error folded into the report is the OS string, which is locale-dependent, so the report is stable per machine, not across machines. Pre-existing precedent at `apply.rs:112` (`cannot write …: {e}`), unchanged by this wave.
4. `mustard.json:23` — version stamp `0.1.30 → 0.1.33` rode along in commit `6a57df41`, outside the spec's declared Boundaries — but it corrects a stale stamp against `Cargo.toml` 0.1.33 and is unavoidable under the `add -A` law.
5. `apps/rt/src/commands/scan_cli.rs:251` adds a 4th `semicolon_if_nothing_returned` warning to a file where 3 identical ones already sit (247/254/257) — sibling-consistent, warning level.

No Guards violation, no mold violation, no correctness defect found.
