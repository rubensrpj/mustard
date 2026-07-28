## Verdict: APPROVED — 0 critical

**Build & tests (run, not taken on trust)**
- AC-9 `cargo build --workspace` -> exit 0.
- AC-1...AC-8, AC-10...AC-13: each named test run individually -> `test result: ok. 1 passed; 0 failed` (satisfies `[1-9][0-9]*`). Verified two-sided by reading, not by name: AC-5 has an *accounted* control duty plus a sibling-wave test; AC-7 has a TS control that must NOT carry the skip; AC-8 asserts `dropped:1 / marked:0` events and terminality; AC-13 asserts both the residue removal AND that a command-only amendment leaves prose alone AND that an `Expect:` line inside the block is never eaten; AC-1 asserts the ledger file on disk moved, not just the report.
- `cargo test --workspace` -> one failure only: `mustard_core io::atomic_md::store::tests::bench_scan_200_files_under_100ms` (`store.rs:277`, a wall-clock <100ms assert). Isolated rerun -> `ok. 1 passed`. `store.rs` is untouched by this branch. Pre-existing flake, not this spec's.
- `cargo clippy --workspace --all-targets` -> 0 errors (deny `unwrap_used`/`expect_used` holds).

**Guards + molds** — no violation. No new `run` subcommand (`--confirm`, `--drop/--reason` are flags on existing ones), so the four-registration rule does not apply; `run_command_surface`/`template_parity` ratchets green. `WaveStatus::Dropped` is serde-additive and pinned by a test asserting the four existing words are byte-identical (`packages/core` model-contract guard). No `std::fs` write bypass introduced.

**The three prior criticals are genuinely closed, and I checked the mechanism, not just the prose**
1. `close_pipeline.rs:153` calls `ac_negative_check::confirm_in_process` — a real production caller, before the terminal event; `resume-loop.md` + `pipeline-config.md` now teach all three readings (`taken:false`/`unproven`/advisory).
2. `resume-loop.md:111` no longer contradicts `ac_amend.rs:589`; the inexecutable-predecessor exception is documented, and `confirm_one` (`ac_negative_check.rs:604`) refuses to confirm anything whose `proof != Red`, so `evidenced()`'s second half cannot be used to smuggle a vacuous criterion past `approve-spec`.
3. `neverDispatched` and the `Onde` legend now have readers, and `AC-12`'s test enforces *proximity* to `currentWave`, not mere presence.

**Non-blocking**
- MAJOR — `plugin/refs/spec/resume-loop.md:64` still says `{ok:true}`/absent -> dispatch. AC-7's `skipped` marker now rides the trim (`wave_advance.rs:390`) but the documented reader is still told to treat `ok:true` as dispatch-green. Same shape as CR-1/CR-3, one file away; AC-7's literal wording ("the caller surfaces") is met, so it does not block.
- MAJOR — AC-13 ships `verdict:unproven / proof:green` (`ac-proof.json:44-53`). `approve_spec.rs:437` is fail-closed on exactly that, so this spec would not clear its own approval gate if re-run. The red half *was* takeable (the CR landed 30 min before the commit; writing the criterion first would have yielded red) — the honest ledger is the right call, but "unprovable" is not accurate. CLOSE does not gate on the ledger, so it does not block the close.
- MINOR — AC-13's ledger `reason` is the canned `REASON_GREEN`: *"this criterion cannot tell done from not-done — rewrite the command"*. That is false here (the test genuinely fails without the `consumed` logic), and contradicts the spec's own Decision. A reader of the ledger alone is pointed at the wrong action.
- MINOR — `ac-proof.json` carries no `confirmation` key on any entry, including the round-2 ones: the ledger was written by an installed binary predating wave 1. Harmless (`#[serde(default)]`), but the ledger you are reading was not produced by the code you are shipping.
