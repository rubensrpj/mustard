## Verdict — REJECTED (2 critical as filed; CRITICAL 2 is VOID, see the note)

Ran: `cargo test -p mustard-rt` -> 3615 passed, 0 failed (33 suites, 298s); `cargo clippy -p mustard-rt --all-targets` -> warnings only, no `unwrap_used`/`expect_used` denials; workspace compiles (AC-7 green). Guards: no panic path added (`accounts_for` bounds-guards both sides), observer still returns `()`, no new `run` subcommand in this spec's scope.

**PASS — T2/AC-2** `obligation_match_is_by_id_not_substring` -> ok. 1 passed. `wave_done.rs:140` `accounts_for` requires non-id chars on both sides; test covers `RO-3.10`/`RO-3.1`, prefix, suffix, empty and multi-byte neighbours.

**PASS — T3/AC-3** -> 2 passed. `close_pipeline.rs:180` `unproven_wording` gives every `Confirmation` arm a distinct sentence, plus a dedup assertion so three shapes cannot collapse into two.

**PASS — T4/AC-4** -> 1 passed. `render/retry.rs:127` scopes both the `review.result` lookup and the findings read; `read_scoped_findings` refuses the spec-wide file once any scoped file exists. Test drives the real writer `record_review`, not a fixture layout.

**PASS — T5/AC-5** -> 1 passed. `resume-loop.md:64` teaches `{ok:true, skipped:"..."}` -> DECLINED, and `plugin_prose_matches_shipped_behaviour.rs:176` pins the mechanism so the prose cannot outlive it.

**CRITICAL 1 — T1/AC-1 is inert in production. STANDS.**
`subagent_inject.rs:462` sources the emitting wave from `MUSTARD_ACTIVE_WAVE`. Nothing in this repository sets that variable — the crate says so at `wave_advance.rs:191` — and `shared/events/route.rs:177-181` already applied the identical env fallback, so the edit adds no information the row did not have. Confirmed empirically: every real `decision` row under `.claude/spec/*/.events/*.ndjson` carries `"wave":null`. So `e.wave > 0` is false for every production lesson, every memory file becomes `*-waveunknown.md`, and AC-1 goes green only because the test hand-seeds `wave: 3/4/5` — a state no emitter can produce. The Success Metric ("five memory files with five correct headers") is not met.

**CRITICAL 2 — VOID. The premise was false, and the fix loop must NOT act on it.**
As filed, it said AC-6 was green by construction and that the real removers were `work_branch_gate.rs:164` and `git_settle.rs:469` (`git checkout` dropping working-tree files). The reasoning was sound and the finding is still withdrawn, because the cause is now known and is not mechanical: **the operator deleted `MUSTARD-COMMANDS.md` and `install-retrieval.ps1` by hand, by mistake.** No code removed them. AC-6 and T6 have been dropped from the spec — T6 is marked as a DROPPED decision with this reason, not left as an unchecked item. Both restorations were correct; all three root files are tracked and byte-exact. Do not chase `work_branch_gate` or `git_settle`.

**MAJOR 3 — the "none is dropped" half of T1 is undiagnosed.** `wave_done.rs:285-305`: nothing on the drop path changed; `free_memory_path` already disambiguated collisions with suffixes. The measured 5-emitted/4-written loss is never explained, and AC-1 asserts "none dropped" over three lessons with distinct slugs that the old code would also have written.

**MINOR 4 — `review_result.rs:114`** still overwrites the spec-wide `review/findings.md` with the last reviewer's content on every review. `read_scoped_findings` guards the retry, but any other reader keeps the cross-subproject leak.

Unrelated flake (not this wave): `mustard-core io::atomic_md::store::tests::bench_scan_200_files_under_100ms` at 104ms vs a 100ms wall-clock limit (`store.rs:277`).

### What the fix loop must do
1. CRITICAL 1 — make wave attribution work from a source that production actually populates, or state honestly that it cannot and make AC-1 assert the reachable behaviour instead of a hand-seeded one.
2. MAJOR 3 — diagnose the 5-emitted/4-written loss for real, or record why it cannot be reproduced.
3. MINOR 4 — the spec-wide findings file.
4. Do NOTHING about CRITICAL 2.
