# Re-review — make-spec-authoring-carry-conversation (post-fix)

## Verdict: approved — 0 critical

## Verified live, with the real binary in a temp project — not the unit tests
- A hollow `.clarified` plus a real user approval made `approve-spec` print "recorded NOTHING", name the remedy, and EXIT 1. The gate that could not fail now fails, and no event is emitted on refusal.
- `grill-capture --finalize` with neither term nor reason wrote no marker: the hollow marker can no longer be minted by its own door.
- `spec-draft --material` with a mistyped key returned `unknown field` naming the valid ones, and wrote NO spec — the fail-closed claim holds.
- Valid material produced the Evidence section with the file:line intact, and `analyze-validation` on that same file returned zero issues: the prose rule really does accept what it used to reject.

## Suite and criteria
- `cargo build --workspace` green; `cargo test --workspace` exit 0 across 61 suites, zero FAILED; clippy exit 0 with nothing pointing at a touched file.
- AC-1 through AC-11 each run individually: every one `test result: ok. 1 passed`. AC-11 confirmed unfiltered.

## Guards, molds, scope
- Guards: none violated. The fail-open invariant is respected; the fail-closed clarify check is a command, not a hook, and says so. `run` output stayed byte-stable. The four-registration rule was satisfied. The CLAUDE.md Guards edit is a factual sharpening of an existing ratchet, not a loosening.
- Molds: the gate pattern and the inject pattern both intact; only constants moved, and the strict block still fires.
- Scope: the files flagged as leakage are declared in the tactical spec — verified, not assumed.
- All seven change requests addressed. The load-bearing one is honoured literally: the guard locates rows by shape, asserts the set both ways, and the prose lost its stated count entirely.

## Non-blocking findings
1. minor — a test comment still describes the median cut after it moved to the upper quartile. No assertion depends on it, but it teaches the wrong rule.
2. minor — `plugin/refs/agent-prompt/agent-prompt.md` was rewritten for AC-11 and never added to the spec's Files/Boundaries. The per-wave cut keys off exactly that list, so an undeclared file is invisible to it.
3. minor — the value filter's doc calls itself "deliberately conservative" while its marker lists carry very common words, so the bar is looser than claimed. AC-10 is genuinely met; this is calibration, not a defect.
