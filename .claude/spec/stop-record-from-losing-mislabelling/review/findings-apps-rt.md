## Verdict — APPROVED (0 critical)

Everything below was run, not read.

**Build & suites**
- AC-7 `cargo build --workspace` -> green, 10.91s
- `cargo test -p mustard-rt` -> 1772 passed + 1777 passed + 31 integration binaries, 0 failed anywhere
- `cargo clippy -p mustard-rt --all-targets` -> no unwrap_used/expect_used denial, no errors

**Per criterion, each filter run individually with a non-zero count**
- AC-1 `every_wave_keeps_its_own_memory` -> ok. 3 passed. The "route production populates" claim was NOT taken on trust: the official hooks schema documents `agent_transcript_path` for SubagentStop; both keys are unmodelled in `contract.rs` so `#[serde(flatten)] raw` really receives them; a real subagent transcript on this machine has line 1 = the EXPANDED prompt verbatim, which is both the seam the stamp rides and independent proof the PreToolUse rewrite reaches the child; `wave_advance.rs` emits ref stubs, so the stamping path is the one production walks. The end-to-end test drives stub -> hook -> transcript -> SubagentStop -> materialize with no hand-seeded wave. The prior CRITICAL (inert MUSTARD_ACTIVE_WAVE) is genuinely closed.
- "none dropped" -> a real two-sided diagnosis: the verbatim lost sentence fails `lesson_qualifies`, and the same sentence plus a consequence clause qualifies and is written. Names the failing clause instead of asserting one.
- AC-2 `obligation_match_is_by_id_not_substring` -> ok. 1 passed. Byte-indexed on both sides; covers RO-3.10/RO-3.1, prefix, suffix, empty id, empty haystack, later real hit, multi-byte neighbour.
- AC-3 `close_report_spells_the_two_unproven_cases_apart` -> ok. 1 passed. Five distinct wordings plus a dedup assertion.
- AC-4 `retry_context_is_scoped_to_its_subproject` -> ok. 1 passed. Reader AND writer scoped; the test writes through the real `record_review`.
- AC-5 / AC-6 / AC-8 -> ok. 1 passed each. Not spell-checks: each asserts prose AND mechanism (`recommended_subagent_type` for reserved roles; `project_seed.rs`/`config.rs` for the seed; plus a no-drift assertion between the template and the seeded copy, diffed by hand: identical).

**Change requests** — the last one produced AC-6 and AC-8 through `ac-add`, both proven RED first (ledger `additions`). All four registrations present for `ac-add`.

**Non-blocking findings**
1. MAJOR — `review_result.rs:178` `write_review_verdict_md` still writes one unscoped `review/verdict.md`, so a later spec-wide review overwrites a subproject's. Live evidence in-tree: a commit dropped `- Subproject: apps/rt` from this spec's own verdict.md. Verbatim the leak T4 closed for `findings.md`, left open in the neighbouring function a human reads.
2. MAJOR — the new `--spec` paragraph listed `ac-negative-check` among PATH-only commands, but `resolve_spec_file` accepts slug or path.
   RESOLVED after this review: verified at `ac_negative_check.rs:657-668` (file, dir, then slug through the same locator qa-run uses), corrected in the template AND the seeded copy, and the parity assertion in AC-6 re-run green. `ac-amend` named alongside it for the same reason.
3. MINOR — `render/retry.rs`: a root render (`subproject "."`) loses both the spec-wide findings fallback and any subproject-named verdict once one scoped findings file exists. Fine for plan-driven dispatch, but it silently narrows a root-scoped retry.

None violates a `## Guards` rule or an `rt-*-pattern` mold, and none is a correctness defect in the shipped criteria — so none blocks.
