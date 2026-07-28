## Verdict: APPROVED — 0 critical, 1 major, 3 minor

### Independently re-run
- AC-1 `guard_allows_when_build_target_is_not_the_running_binary` — ok. 1 passed (x2 targets)
- AC-2 `guard_refuses_when_build_target_is_the_running_binary` — ok. 1 passed (x2 targets)
- AC-3 `cargo build --workspace` — exit 0, 11.08s
- `cargo test -p mustard-rt` — 3580 passed, 0 failed (33 suites)
- `cargo clippy -p mustard-rt --all-targets` — 0 errors

### T1/T2 genuinely path-based, not renamed text matching
`runner.rs:126-157` — `package_shadowing_running` compares `running.parent()` against `target_root/<profile>` through `comparable()` (canonicalize + Windows case/separator flattening), then requires the named `-p` package to equal the running file's stem. `SELF_CRATES` is gone (`git grep targets_running_crate` -> no hits). The `--workspace` salvage asks the same question and excludes only the shadowed package.

### T3 measured, not asserted — the previous CRITICAL is closed
- `~/.cargo/bin/mustard-rt.exe` contains `this command overwrites` (2 hits) and ZERO hits of the old wording: the binary that took the run carries the fix.
- The previous spec's `qa/report.md` is 13/13 PASS with FRESH timings (AC-9 9.6s->4.7s, AC-1 31.6s->7.8s), so it is a new run.
- Its `meta.json` reads `"stage":"Close","outcome":"Completed"`.

### Previous review's findings — both resolved
- MAJOR absolute path: `runner.rs:171-178` falls back to `file_name()`, then `UNNAMEABLE_BINARY`; proven by `running_binary_label_never_leaks_an_absolute_path`.
- MINOR wrong branch: `runner.rs:827-841` tempdir now writes `Cargo.toml` and asserts `!overwrites_running_binary(...)` directly.
- Root files: `git diff origin/dev...HEAD -- MUSTARD-COMMANDS.md install-retrieval.ps1` -> empty, byte-identical to dev.

### MAJOR — the fix pass added behaviour no criterion runs
`running_binary_label_never_leaks_an_absolute_path` is the only guard against the machine-path leak and no criterion named it, so QA would stay green if the fallback regressed.

RESOLVED after this review: AC-4 was added naming that exact test. Its ledger entry is honest about arriving late — `verdict: unproven, proof: green`, because the work it describes had already landed when the criterion was written. Fabricating a red, or dropping the criterion to keep the ledger tidy, would each be the habit these specs remove. The structural cause (no door to ADD a criterion, only to replace one) is the subject of the next spec.

### MINOR
- This spec's own `qa/report.md` came from an EXTERNAL `run qa-run` (`self_invoked=false`), so it proves the criteria, not the self-invoked branch. That branch is proven by the previous spec's close-pipeline, verified above.
- `runner.rs:136` derives the package name from the running file's stem, so a crate whose `[[bin]] name` differs from its package name would not match. Harmless today; the doc says "package".
- `runner.rs:367-370` `.unwrap_or_default()` is unreachable — the branch is only entered when `paths.is_some()`.

### Not a finding (checked, pre-existing)
`ac-proof.json:12` carries an absolute path inside `stderr_excerpt`, but that is cargo's own output through the untouched `excerpt()` path; four other specs on dev already carry it.

No `## Guards` rule violated: no new `run` subcommand, no hook touched, no `unwrap`/`expect` outside `#[cfg(test)]`, refusal reason now machine-independent.
