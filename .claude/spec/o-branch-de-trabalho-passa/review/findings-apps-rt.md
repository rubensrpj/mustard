## Verdict — APPROVED (0 critical) — `o-branch-de-trabalho-passa` @ `apps/rt`

**Guards — PASS.** No new `run` subcommand (only a new `--type` flag on `event emit-pipeline` + `git work-unit-open`), both `cli.rs` families got variant field AND dispatch arm. `cargo clippy --workspace --all-targets` → 0 errors, zero `unwrap_used`/`expect_used` outside `#[cfg(test)]`. The new `BaseUnknown` state returns `Verdict::Warn` (or `Deny` only to keep an edit off a bare base) — the hook never blocks on its own error.

**Molds — PASS.** `CutOutcome` compliant with rt-outcome-pattern (payload enum, no serde, `AlreadyThere` idempotent variant kept, explicit `project: &Path`). `WorkBranchGate` compliant with rt-gate-pattern. `work_kind.rs` matches no mold path glob.

**Acceptance Criteria — all proven by the reviewer, not taken on trust.** AC-1 `1 passed` (control `compute_work_branch` green) · AC-2 `1 passed` · AC-4 `1 passed` · AC-5 `2 passed` (includes the survives-the-cut sibling driving a real 3-tier repo) · AC-3 `cargo build --workspace` 0 errors, 2 warnings both proven pre-existing by diffing `6bb40e8d` · `cargo test --workspace` 4847 passed, 0 failed, 6 ignored (70 suites).

**Independent end-to-end with the real binary** (temp repo, flow `{*:dev, dev:qas, qas:main}`):
- `emit-pipeline --type hotfix --base qas` → `"branch":"hotfix/corrigir-emergencia-login"`; marker carries two lines.
- `spec-draft` → HEAD == `qas` (not `main`, the pre-marked default), **`spec.md` written**, `meta.json` holds `"base": "qas"`, `.cut-base` gone.

The previously-rejected critical (the cut wrote `meta.json` and locked `spec-draft` out, leaving the unit spec-less) is genuinely fixed, not merely described.

### Non-blocking findings

- **minor** — `packages/core/templates/.gitignore` gained 13 rules but NOT `spec/*/.cut-base`, while `work_kind.rs:78` claims the file "never reaches the merge". It is retired by `write_meta_json` in the same command, so the window is small — but the claim is unenforced.
- **minor** — `pr_door.rs:136` and `session_start_inject.rs:453` build the ROOTLESS `BaseFlow::of`, so a recorded hotfix base is invisible there; in a ≥3-base project the `/pr` refusal hint falls back to `primary_base()` and the session surfacing shows an empty base. Display/classification only — every decision that consumes a base uses the rooted `of_at`.
- **minor** — `mode_decision.rs:174` compares `slug_of_work_branch(current) == spec` raw, where it previously compared against the sanitised `compute_work_branch`. Harmless for canonical slugs, fragile for a slug needing sanitisation.

<VERDICT>{"verdict":"approved","critical":0,"findings":[{"severity":"minor","location":"apps/rt/src/shared/work_kind.rs:80","summary":".cut-base is not in the seeded .gitignore, so the doc claim that it never reaches the merge is unenforced"},{"severity":"minor","location":"apps/rt/src/commands/review/pr_door.rs:136","summary":"rootless BaseFlow::of hides a recorded hotfix base from the /pr refusal hint and the session surfacing in >=3-base projects"},{"severity":"minor","location":"apps/rt/src/commands/pipeline/resume_bootstrap/mode_decision.rs:174","summary":"branch-slug comparison no longer passes through sanitize_git_ref, fragile for slugs needing sanitisation"}]}</VERDICT>
