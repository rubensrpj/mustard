## Verdict — APPROVED (0 critical)

Guards: root CLAUDE.md#Guards is an empty seed; apps/rt and packages/core guards checked one by one. Four-registration rule N/A (cli.rs diffs are doc-text only). `cargo clippy --workspace --all-targets`: zero error lines. `.cut-base` uses write_atomic. Hooks still fail-open.

### Acceptance Criteria — each run, real output
AC-1 1 passed · AC-2 1 passed · AC-3 1 passed · AC-4 1 passed · AC-5 1 passed · AC-6 all green, 0 failed (2026 rt lib tests + 60 integration binaries). Re-run with --test-threads=1 also green, so no memo/order dependence.

### Discrimination checks (would these fail on the old code?)
AC-1 asserts HEAD == rev(release/2026-Q3) and != rev(dev) — the old membership filter yielded dev, so it fails. AC-2's first half obeys an undeclared-but-existing base, which the old filter dropped. AC-3 drives delete_at/list_at and asserts branch survival, not source text. AC-4/AC-5 both invert prior assertions. None is a rubber stamp.

### The six refusal points
`preselected_bases()` now has NO refusing caller. Remaining sites: the picker's pre-selection flag (git_branches.rs:227), the refresh set (work_branch.rs:332), hint ordering (work_kind.rs:498) and two test assertions. The deprecated `integration_bases()` forward is deleted. Freshly built binary in a throwaway repo: `doctor --check branch-protection` returns status: ok, protected: dev, pre-selected bases: none declared, with no "declare the flow" prescription.

### Non-blocking observations
1. work_kind.rs:1219 and :1261 — both tests `return` silently when seed_remote_refs fails, so they would go vacuously green on a machine without git. Non-vacuous here; the skip is invisible.
2. work_branch.rs:331 — the forget_remote_names comment claims it protects "the reader that consults it then drops the operator's recorded base", but both cut doors resolve the recorded base BEFORE calling refresh. The call is still correct (it protects the later record_cut_base/base_of reads); the rationale overstates.
3. work_kind.rs:873 — has_unit_record's ref leg hardcodes ".claude/spec/{slug}" while the on-disk leg goes through ClaudePaths::spec_dir(). Equivalent today; a ClaudePaths change desyncs the two halves of one predicate silently.
4. work_branch.rs:403 and statusline/segment.rs:379 still build a rootless BaseFlow::of, so a legacy {base}_{slug} unit whose base the flow does not declare resolves to None. Read-only paths, no refusal, but the last places the closed list still decides an answer.
5. Operator-facing: the INSTALLED hook binary still carries the $(mktemp -d) splitting bug; reproduced live this session. The fix (env_token_end, rtk_rewrite.rs:330) is in this wave and its test passes; it only needs the release to land.
