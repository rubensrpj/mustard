## Verdict: APPROVED (0 blocking)

### Guards (`apps/cli/CLAUDE.md`) — all held
- No `unwrap`/`expect` outside `#[cfg(test)]`: `git diff -U0 … | grep '^\+.*\.\(unwrap\|expect\)('` → 26 hits, **all** inside test modules. `backup_dirs` uses `let-else` + `flatten`/`filter_map`; `settings_dest` uses `parent().unwrap_or(claude_dir)`.
- `mustard.json` still written at `<root>` (`write_project_config`), `.claude` untouched in `copy_dir` skip list, `update --force` path unchanged, `~/.claude` write still gated (`MUSTARD_GLOBAL_PERMISSIONS`).
- New git seam is fail-open by construction (`ExcludeOutcome::unavailable`, `tracked_paths` → empty), never blocking — verified by `a_tree_without_a_repository_reports_instead_of_failing`.

### Mold `cli-options-pattern` — followed
`InitOptions` gained `pub private: bool` with its own `///` line, no field attribute, derive set unchanged; the `#[arg(long)]` lives in `apps/cli/src/cli.rs:44` on `Commands::Init`; `dispatch` builds the literal with field-init shorthand; entry signature still `&Path, &InitOptions`. The new `apps/cli/tests/private_init.rs` is a *tests/* file rather than an in-file `mod tests`, which the mold discourages — justified and accepted: AC-7 names `--test private_init`, and `cfg!(test)` is false in-crate so `probe_rtk`'s `process::exit(1)` would kill the binary (documented at `apps/cli/tests/private_init.rs:5-19`).

### AC verification (commands run, real output)
| AC | Command | Result |
|----|---------|--------|
| 1-4 | `cargo test -p mustard-core --test private_install` | `4 passed` |
| 5 | `… -p mustard-rt --test private_scan` | `1 passed` |
| 6 | `… -p mustard-rt --test private_surface` | `test result: ok. 1 passed` |
| 7 | `… -p mustard-cli --test private_init ac7_…` | `1 passed` |
| 8 | `… --test private_install_leaves_no_trace ac8_…` | `test result: ok. 1 passed` |
| 9 | `cargo build --workspace` | `0 errors` (1 pre-existing warning in `feature.rs:488`, untouched) |

Regression sweep: `cargo test --workspace` → **2929 passed, 6 ignored, exit 0**. `cargo clippy --workspace --all-targets` → **0 errors**.

Each AC test carries a real negative control (shared install seeds `.github/`, shared status *must* show the footprint, shared `run upsert` reports no `private` key), so a green run is not "nothing happened".

### Change requests — both addressed
- **CR (blocking, root-anchored rules):** fixed at `packages/core/src/platform/project_seed.rs:112` with `**/.claude/` + `.claude.backup.*/` covers; AC-8's fixture really carries a subproject `packages/api/.claude/` and a committed CRLF `packages/api/CLAUDE.md`, and the shared control asserts git *sees* `packages/api/.claude/scan-map.md`. Deviation from the CR's literal instruction (per-file `**/` prefix) is documented and justified: `tracked_paths` hands the same strings to `git ls-files`, where a `**/`-prefixed pathspec would break the residue report.
- **CR (backup dir under AC-8):** the fixture seeds `.claude.backup.20260817-101500/` and asserts empty `git status --porcelain -uall`; the shared control asserts `{BACKUP_DIR}/settings.json` is visible. No new criterion added, as instructed.

### Non-blocking findings
1. **minor** — `apps/cli/src/commands/init.rs:374` `backup_dirs()` is now redundant: wave 4 added `CLAUDE_BACKUP_DIRS = ".claude.backup.*/"` to `footprint_paths()`, which already covers every such directory at any depth. The CLI's per-name discovery appends an extra exact-name line per backup on each private init (`missing_rules` compares trimmed literals, so `.claude.backup.X/` never matches `.claude.backup.*/`), slowly growing the exclude file. No test exercises this path — AC-7's init takes the non-interactive merge branch, and AC-8 never calls the CLI.
2. **minor** — `apps/cli/src/commands/init.rs:147` `mustard init` does **not** autodetect the mode, while `run upsert` does (`apps/rt/src/commands/maint/upsert.rs:40`). Since `init.rs:268` calls a re-run "the idempotent replacement for `mustard update`", a later plain `mustard init` on a private project seeds `.claude/settings.json` and re-attempts `.github/`. Not a leak — both are already in the exclude file and `copy_dir` never overwrites — but the two install faces disagree about a decision the spec says is taken once.
3. **minor** — `packages/core/src/platform/project_seed.rs:169` `CLAUDE_MD` as an `ls-files` pathspec is literal, so a host repo that tracks a *subproject* `CLAUDE.md` is never named in `already_tracked`. Harmless today (private mode never writes that file), but the residue report under-covers depth exactly where AC-8's own fixture puts it.
