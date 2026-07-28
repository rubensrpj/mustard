## Verdict: REJECTED — 1 critical (the critical is now RESOLVED by measurement; see the note at the end)

### Verified green (independently re-run by the reviewer)

- AC-1 `guard_allows_when_build_target_is_not_the_running_binary` — ok. 1 passed (x2 targets)
- AC-2 `guard_refuses_when_build_target_is_the_running_binary` — ok. 1 passed (x2 targets)
- AC-3 `cargo build --workspace` — exit 0
- `cargo test -p mustard-rt` — 3578 passed, 0 failed
- `cargo clippy -p mustard-rt --all-targets` — 0 errors
- root-file restore is byte-exact

T1 and T2 genuinely implemented: `overwrites_running_binary` compares `running.parent()` against `target_root/<profile>`; the refusal names the file; `named_packages` keeps token boundaries.

### CRITICAL — T3 was asserted, not measured (RESOLVED after this review, see note)

- HEAD carried `Overall: PASS` 13/13, but that content was committed BEFORE the guard landed and came from an EXTERNAL `qa-run` where `self_invoked = false` already made the old guard inert. It measured nothing about this fix.
- The working tree held the same file modified back to `Overall: SKIP`, 12 rows carrying the OLD wording `self-invocation: cannot rebuild the running binary`, written AFTER both guard commits.
- The installed binary at `~/.cargo/bin/mustard-rt.exe` did not contain the fix, so the claimed run could not have happened.

### MAJOR — absolute machine path can reach versioned files

`apps/rt/src/commands/review/qa_run/runner.rs:158-160` — `running.strip_prefix(cwd).unwrap_or(running)`. With `CARGO_TARGET_DIR` set to an absolute path outside the project (a refusal is then still reachable), the raw absolute path is interpolated into `stderr_excerpt` -> `qa/report.md` and into `reason` -> `ac-proof.json`, both committed files. Violates the crate Guard "saida determinista, sem caminhos volateis" and the project law against machine paths in versioned files.

### MAJOR — dirty deliverable that reverses the record

The PASS->SKIP modification was uncommitted. Committed as-is it destroys the previous spec's pass; discarded, nothing measured T3.

### MINOR — a test names a mechanism it does not take

`runner.rs:799-824` — `qa_self_invoked_runs_the_command_when_the_paths_differ` claims "this process runs from somewhere else, so nothing is refused", but its tempdir has no `Cargo.toml`, so `cargo_target_root` returns `None` and the guard short-circuits on the UNANSWERABLE branch, never the path-differ branch.

### MINOR — commit labelling

`a1cd5d33` carries the entire guard implementation under a `chore(spec):` subject, mixed with two spec scaffolds this spec's Non-Goals place out of scope.

---

### NOTE recorded by the orchestrator after this review

The critical was taken seriously and RESOLVED by doing what the reviewer asked, in this order:
1. `cargo install --path apps/rt --force` — the fixed binary now lives at `~/.cargo/bin/mustard-rt.exe` (29.2M, the old one parked as `.old-5a230680`).
2. `mustard-rt run close-pipeline --spec make-harness-stop-asserting-what` — the SELF-INVOKED path, not the external one.
3. Result: `completed: true`, `overall: "pass"`, all 13 criteria `status: pass`. The previous spec is now genuinely closed, and `qa/report.md` on disk records it.

What remains for the fix loop: both MAJORs (the absolute-path fallback, which is real and still in the code) and both MINORs.

### Separate defect found while committing, NOT part of this spec

`MUSTARD-COMMANDS.md` (716 lines) and `install-retrieval.ps1` (49 lines) were removed from the project root TWICE during this session's commits — once after the implementer returned, once during `close-pipeline`. Both files exist on `dev` (verified with `git cat-file -e dev:MUSTARD-COMMANDS.md`), and `apps/rt/tests/template_parity.rs:221` still requires `MUSTARD-COMMANDS.md` to exist. Restored byte-exact both times. The cause was not located: `remove_file` callers in `hooks/session` and `hooks/task` all operate inside `.claude/`.
