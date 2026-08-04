---
id: spec.give-scan-flow-commands-it
---

# Give the /scan flow the commands it currently forbids scripts for: a file face on the patterns relay (--content @path, shared with the apply), a --subproject filter on scan-patterns-list, a scan.md rule that permits N relay calls at === END === boundaries while still forbidding splits inside a block, and a patterns prompt that stops promising the channel never truncates plus a documented re-dispatch convergence procedure

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

Give the /scan flow the commands it currently forbids scripts for: a file face on the patterns relay (`--content @path`, shared with the apply), a `--subproject` filter on the mold worklist, a flow rule that permits N relay calls at block boundaries while still forbidding splits inside a block, and a patterns prompt that stops promising the channel never truncates — plus a documented re-dispatch convergence procedure.

Why now: a real enrich over a 12-unit workspace measured four frictions, and three of them are the SAME failure — the flow forbids writing a script but does not offer the command that would replace it. The sharpest case is the relay. It was built for exactly the large-envelope problem and solves it correctly, yet the transport that carries a large return TO the relay does not exist: when a return exceeds the harness's inline limit it is persisted to a JSON file and the orchestrator is handed only a path, and `--content` accepts no path. That happened in 3 of 10 dispatches (73 KB, 60 KB, 78 KB) — the ordinary shape of a mid-sized subproject, not an edge case. The only path that worked was a hand-written script, which the flow text forbids in capitals. When the sole clean route is the forbidden one, the prohibition is aimed at the wrong target.

## Users/Stakeholders

The orchestrator running a `/scan` enrich — it is the one that receives a file path it cannot forward, and the one that must answer "how many clusters are left in this subproject?" three times per run. Downstream: every subproject whose molds are lost when a return truncates, since a mold that never lands blocks its cluster until the next scan cycle.

## Success Metric

A full enrich over this workspace completes with ZERO hand-written scripts: a harness-persisted return is forwarded with `--content @<path>`, a convergence check is one command, and a truncated return is recovered by a documented re-dispatch rather than by transcription.

## Non-Goals

- Slicing the fan-out finer (one agent per slice instead of per subproject) — see the Decisions section: it destroys the whole-subproject view that produced 46 of this run's declines.
- Automating the re-dispatch loop. The flow DOCUMENTS how convergence works; deciding to run another round stays the orchestrator's call.
- Changing the demarcator format, the sweep, the decline ledger, or the miner.
- Raising the harness's inline limit or the Windows argument limit — neither is ours to move.

## Acceptance Criteria

- **AC-1** — when `--content @<path>` names a file holding the envelope as raw text, then the relay applies its blocks exactly as if they had arrived on stdin
  Command: `cargo test -p mustard-rt relay_reads_an_envelope_from_a_file_path 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt a_backticked_path_is_read_the_same_as_a_bare_one 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-2** — when the file at `@<path>` holds the harness's own shape (a JSON array of `{type, text}` objects) instead of raw text, then the envelope is recovered by concatenating the text fields, so no script is needed to unwrap it
  Command: `cargo test -p mustard-rt relay_reads_the_harness_json_array_of_text_blocks 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt an_envelope_of_many_blocks_is_split_by_its_demarcators 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-3** — when `@<path>` cannot be read, then the report names the IO failure and comes back `ok:false`, instead of degrading to an empty envelope that prints `ok:true, blocks:0`
  Command: `cargo test -p mustard-rt an_unreadable_content_path_is_reported_never_silently_empty 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt a_blockless_envelope_reports_empty_and_never_errors 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-4** — when `scan-patterns-apply` is given `--content @<path>`, then it reads the mold body from that file through the SAME reader the relay uses, so the two commands cannot drift apart
  Command: `cargo test -p mustard-rt apply_reads_the_mold_body_from_a_file_path 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt resolve_content_blanks_are_none 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-5** — when the worklist is asked for one subproject, then it yields that subproject's entries ONLY, on both the default face and the `--rejected` diagnostic, and an unknown subproject yields an empty list rather than everything
  Command: `cargo test -p mustard-rt list_filters_the_worklist_by_subproject 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt collect_proposes_mold_for_a_real_cluster 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-6** — when `scan-patterns-list --rejected --subproject apps/rt` is run against this workspace, then exactly one distinct subproject appears in the output, so the convergence check is one command instead of a grouping script
  Command: `test "$(cargo run -q -p mustard-rt --bin mustard-rt -- run scan-patterns-list --rejected --subproject apps/rt 2>/dev/null | grep -o '"subproject":"[^"]*"' | sort -u | wc -l)" -eq 1`
  Control: `test "$(cargo run -q -p mustard-rt --bin mustard-rt -- run scan-patterns-list --rejected 2>/dev/null | grep -o '"subproject":"[^"]*"' | sort -u | wc -l)" -ge 2`
- **AC-7** — when the flow text is read, then it no longer promises the agent that return size is safe and no longer forbids the temp file, and it teaches the file face plus re-dispatch convergence
  Command: `! grep -q 'never worry about its size' apps/rt/src/commands/agent/render/role.rs && ! grep -q 'never via a temp file' plugin/commands/scan.md && grep -q -- '--content @' plugin/commands/scan.md && grep -qi 'converg' plugin/commands/scan.md`
  Control: `grep -q 'NEVER write a script to work around a rough edge' plugin/commands/scan.md`
- **AC-8** — when the flow text is read, then splitting text INSIDE a block is still forbidden while N relay calls at `=== END ===` boundaries are explicitly permitted
  Command: `grep -q 'END ===' plugin/commands/scan.md && ! grep -q 'never one per block' plugin/commands/scan.md`
  Control: `grep -q 'one call per agent' plugin/commands/scan.md`
- **AC-9** — when the file at `@<path>` parses as JSON but yields no demarcated block, then the report SAYS so, instead of printing `ok:true, blocks:0` — the same silence AC-3 removes, entering by the other door
  Command: `cargo test -p mustard-rt a_json_envelope_with_no_blocks_says_so_instead_of_reporting_zero 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt the_report_is_byte_stable 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-10** — the project build and tests pass green
  Command: `cargo build --workspace`

<!-- PLAN -->

## Files

- `apps/rt/src/commands/scan_patterns/mod.rs` — home of the ONE shared envelope reader (`-` = stdin, `@<path>` = file, anything else = literal), including the harness JSON-array unwrap
- `apps/rt/src/commands/scan_patterns/relay.rs` — drop the local `resolve_content`, call the shared reader, surface an IO failure in the report; tests for AC-1/AC-2/AC-3
- `apps/rt/src/commands/scan_patterns/apply.rs` — drop the local `resolve_content`, call the shared reader, keep only its own trim-to-`None`; test for AC-4
- `apps/rt/src/commands/scan_patterns/list.rs` — take `normalize_subproject` in, add the subproject filter to `run`
- `apps/rt/src/commands/scan_cli.rs` — `--subproject` on `ScanPatternsList`, the `@<path>` form documented on both `--content` flags, dispatch arms updated
- `apps/rt/src/commands/agent/render/role.rs` — correct the size promise in the patterns prompt; import `normalize_subproject` from its new home
- `plugin/commands/scan.md` — step 4 rewritten (file face, N calls at block boundaries), a re-dispatch convergence paragraph, the script ban re-aimed
- `MUSTARD-COMMANDS.md` — the published `scan-patterns-*` surface follows the new flags

## Boundaries

IN: the file face on `--content` (relay + apply, one shared reader); the IO-failure report; `--subproject` on `scan-patterns-list`; the corrected prompt sentence; the three `scan.md` text changes (file face, boundary splitting, convergence); the command reference.
OUT: slicing the fan-out per cluster; automating the re-dispatch loop; the demarcator format; `scan-patterns-sweep`/`-decline`; the miner and `grain.model.json`; the harness inline limit and the Windows argument limit.

## Definitions

- **envelope** — One patterns agent's WHOLE return: prose plus the `=== FILE: <moldPath> ===` / `=== DECLINE: <slug> ===` blocks the relay splits on. The unit the relay was built to own.
- **file face** — The third form `--content` accepts, alongside `-` (stdin) and a literal string: `@<path>`, meaning read the envelope from this file. It is the form the harness forces when a return exceeds the inline limit and is persisted to disk.
- **harness-persisted return** — What the orchestrator receives instead of the agent's text when the return exceeds the inline limit: a path to a JSON file. Measured on the run that motivated this: 3 of 10 dispatches (73 KB, 60 KB, 78 KB) — the normal case for a mid-sized subproject, not a rare one.
- **re-dispatch convergence** — How a truncated return is recovered: apply the intact blocks, record the declines, re-render. Created molds and declined slugs leave the worklist, so the next round is strictly smaller. Measured: 59 clusters -> save 5 molds + 5 declines -> 49 clusters -> the second round came back persisted and intact.
- **block boundary** — An `=== END ===` line. Splitting an envelope THERE is safe by construction (the relay is idempotent per block and its report is additive); splitting anywhere INSIDE a block is the transcription error the flow forbids.

## Decisions

- `--content` gains the `@<path>` form rather than a sibling `--content-file` flag
  Reason: One surface stays one surface: relay and apply both resolve `--content` through the same reader, so symmetry is automatic instead of a second flag to keep in step on two commands. `--content` already carries `allow_hyphen_values`, so the flag's parsing contract does not change.
- `@` means a path only when the value carries no newline; a value with a newline is the literal envelope
  Reason: A literal envelope ALWAYS spans lines — the demarcators occupy whole lines by construction — so the ambiguity is unreachable in practice, and the rule costs one condition instead of a second flag.
- An unreadable `@<path>` is reported, never silently fail-open to an empty envelope
  Reason: The relay is fail-open by contract (exit 0), but a missing file degrading to an empty envelope prints ok:true, blocks:0 — which reads as `the agent returned nothing to apply` when in truth nothing was read. The IO error lands in the report so ok:false names it.
- ONLY the new `@<path>` channel reports an IO error; the stdin channel (`-`) keeps its current fail-open behaviour byte for byte
  Reason: stdin is how every dispatch that works TODAY reaches the relay. Making the new reporting rule apply to it as well would change working behaviour under cover of an additive feature. The change must only ADD a third channel, never alter the two that exist.
- The JSON branch harvests every `text` field at any depth, rather than matching one exact shape
  Reason: the persisted file is written by the harness, not by this code, so its exact shape is not ours to pin. Matching one shape and falling back to raw text on anything else re-opens the very silence AC-3 closes: an unmatched variant would be read as prose, yield no block, and print ok:true, blocks:0. Harvesting by field name covers the variants without inventing any.
- Rewriting flow step 4 must PRESERVE the paragraph that names the single-block commands
  Reason: `template_parity` runs a REVERSE ratchet — it fails any registered command that no prose calls. `scan-patterns-apply` and `scan-patterns-decline` are named only in that paragraph, so dropping it in the rewrite orphans both and turns a text edit into a red test.
- Both commands resolve the envelope through ONE shared reader instead of each keeping its own
  Reason: resolve_content is already duplicated with divergent contracts — relay.rs:196 returns String, apply.rs:307 returns Option<String> after a trim — and adding the file face to both copies would triple the drift surface. The project law forbids a facade: callers call the core directly and keep only their own trimming.
- `--subproject` on scan-patterns-list reuses the existing filter; normalize_subproject moves from role.rs into list.rs
  Reason: The render already filters the same worklist by normalised subproject (role.rs:199), so the capability exists and is not being written twice. Normalising a subproject path is the worklist's own concern; the render is a consumer of that filter, not its owner.
- The fan-out is NOT sliced finer per subproject
  Reason: relay.rs:16-21 already argues it: an agent that sees the whole subproject declines coherently (`this role is already covered by mold X`), and slice agents lose that view. Measured on this run: 46 of the declines were of that kind — nearly half the useful work. Re-dispatch convergence preserves the whole view; slicing destroys it.
- The forbidden-script rule in scan.md is narrowed, not lifted
  Reason: Three of the four frictions are the same failure — the flow forbids scripts without offering the command that replaces them — so the ban was aimed at the wrong target. Splitting text INSIDE a block stays forbidden; N relay calls at `=== END ===` boundaries become explicitly allowed, because the relay is idempotent per block and its report is additive.
- This unit is cut from dev
  Reason: The `*` base in mustard.json is dev and the checkout was already on it; the user confirmed dev at the base gate.
- The re-mined census (grain.model.json + grain.dictionary.json) travels inside this unit
  Reason: The base gate re-mined it on a clean dev, and the project's /git law is `add -A`, so it cannot be committed apart once the unit branch is checked out.

## Evidence

- resolve_content treats any value other than `-` as the literal envelope text — there is no file path form, so a harness-persisted return has no legitimate way into the relay
  Evidence: `apps/rt/src/commands/scan_patterns/relay.rs:196`
- The apply keeps its OWN copy of resolve_content with a different contract (Option<String>, trimmed) — the duplication a shared reader removes
  Evidence: `apps/rt/src/commands/scan_patterns/apply.rs:307`
- The patterns prompt tells the agent to deliver every mold in one message and never worry about its size — true of the relay, false of the channel, which truncated a ~120 KB return from the front and lost ~48 already-authored molds
  Evidence: `apps/rt/src/commands/agent/render/role.rs:154`
- The flow forbids the only clean path the large case has: one call per agent, never one per block, and never via a temp file
  Evidence: `plugin/commands/scan.md:44`
- The flow forbids scripts in capitals — NEVER write a script to work around a rough edge in this flow — while the file face that would remove the script does not exist
  Evidence: `plugin/commands/scan.md:53`
- ScanPatternsList exposes only --root and --rejected; there is no way to ask how many clusters remain in ONE subproject
  Evidence: `apps/rt/src/commands/scan_cli.rs:229`
- The per-subproject filter already exists in-process: patterns_task_block calls list::collect and filters on the normalised subproject — the CLI simply does not expose it
  Evidence: `apps/rt/src/commands/agent/render/role.rs:199`
- normalize_subproject is private to the render module, so exposing the same filter on the CLI requires moving it rather than copying it
  Evidence: `apps/rt/src/commands/agent/render/role.rs:268`
- The prose ratchet extracts only the command NAME after `mustard-rt run `, stopping at the first byte outside [a-z0-9-] — so new FLAGS documented in scan.md cannot break it
  Evidence: `apps/rt/tests/run_command_surface.rs:175`
- MUSTARD-COMMANDS.md publishes the scan-patterns-* family and its backend list, so the root command reference must follow the new flags
  Evidence: `MUSTARD-COMMANDS.md:127`