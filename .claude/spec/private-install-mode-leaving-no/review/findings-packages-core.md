All 9 acceptance criteria and the full workspace suite are green; one blocking defect the criteria structurally cannot see.

## Verified claims

| Claim | Command | Result |
|---|---|---|
| AC-1 exclude write, idempotent | `cargo test -p mustard-core --test private_install ac1_…` | `1 passed` |
| AC-2 local settings only | `… ac2_private_upsert_seeds_local_settings` | `1 passed` |
| AC-3 tracked residue reported | `… ac3_already_tracked_paths_are_reported_not_unlinked` | `1 passed` |
| AC-4 shared byte-identical | `… ac4_shared_install_is_byte_identical_to_today` | `1 passed` |
| AC-5 private scan → `CLAUDE.local.md` | `cargo test -p mustard-rt --test private_scan ac5_…` | `1 passed` |
| AC-6 `--private` + autodetect | `cargo test -p mustard-rt --test private_surface ac6_…` | `1 passed` |
| AC-7 no `.github/` | `cargo test -p mustard-cli --test private_init ac7_…` | `1 passed` |
| AC-8 host repo clean | `cargo test -p mustard-core --test private_install_leaves_no_trace ac8_…` | `1 passed` |
| AC-9 build | `cargo build --workspace` | `0 errors` (1 pre-existing warning) |
| No regression | `cargo test --workspace` | `2929 passed, 0 failed` |
| Lints | `cargo clippy --workspace --all-targets` | `0 errors`, 172 pre-existing warnings |

Guards: writes go through `io::fs::write_atomic`, `Error::NotFound` is distinguished from `Error::Io`, no `unwrap`/`expect` outside `#[cfg(test)]`, `mustard.json` still only through `ProjectConfig`, `domain/model/` untouched. No `{role}-pattern` mold claims `InstallMode`/`ExcludeOutcome`.

## CRITICAL — the exclude rules hide files Mustard never writes

`footprint_paths()` feeds `ensure_excluded` verbatim, and two of its entries are bare filenames, which gitignore matches **at every depth**. In private mode Mustard writes `CLAUDE.local.md` (never a subproject `CLAUDE.md`) and only the root `mustard.json`. So those two rules can only ever hide the **client's own** files. Reproduced against real git on a fresh repo, after `mustard-rt run upsert --private`:

```
# .git/info/exclude
mustard.json
CLAUDE.md
CLAUDE.local.md
...
$ printf '# billing\n' > services/billing/CLAUDE.md
$ printf '# root\n'    > CLAUDE.md
$ printf '{}\n'        > services/billing/mustard.json
$ git status --porcelain --untracked-files=all
(empty)
```

Three files the operator just authored are invisible to the client's git. Under this project's own `/git` law (`add -A` stages everything), a `CLAUDE.md` written *for* the client is silently never committed. The spec scopes the mode to "nothing **it writes** … may appear in that repo's git" (`spec.md:13`); it does not authorize hiding third-party files. The code even names the tension — `packages/core/src/platform/project_seed.rs:154-158` says the per-file entries stay only because `tracked_paths` needs them as `ls-files` pathspecs — but resolves it by handing the same list to both consumers. AC-8 cannot catch this: an over-broad rule makes its "status is EMPTY" assertion *more* likely to pass.

Confirmed empirically that the two consumers genuinely need different lists: `git ls-files -- CLAUDE.md` matches only the root file (not `packages/api/CLAUDE.md`), and `git ls-files -- '**/.claude/'` matches nothing — so the ls-files half already gains nothing from depth-matching.

## MAJOR — the residue advice targets the client's own file

`packages/core/tests/private_install.rs:130` seeds "the host's OWN instruction file, versioned before Mustard ever arrived" and AC-3 asserts it MUST be named residue. `apps/cli/src/commands/init.rs:352-357` then prints `git rm --cached CLAUDE.md`. Mustard never writes that file in private mode; following the printed command untracks the client's own file, and their next commit deletes it. Same root cause as above.

## MINOR

`apps/cli/src/commands/init.rs:377` (`backup_dirs`) is now dead weight: wave 4 added `.claude.backup.*/` to `footprint_paths()`, which already covers every discovered name — the discovery only appends redundant timestamped lines. Also `mustard init` honours `--private` but does not autodetect the way `run upsert` does (`apps/rt/src/commands/maint/upsert.rs:43`), so a re-run without the flag seeds `.claude/settings.json` beside the local layer; the `**/.claude/` cover keeps it invisible, so no trace escapes.

Change requests: both mid-pipeline instructions were honoured — the AC-8 fixture carries `.claude.backup.20260817-101500/` and a subproject with its own `.claude/`, and `footprint_paths()` gained the depth-reaching covers.
