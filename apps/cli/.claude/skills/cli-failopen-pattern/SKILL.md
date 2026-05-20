---
name: cli-failopen-pattern
description: "Fail-open patterns used across mustard-cli: JSON reads that never error, shell-out helpers returning Option, and opt-in guards for global mutations. Use when reading config files, probing external tools, writing to user home directories, or adding a new external-tool probe. Even if the user just says 'read json', 'probe git', 'check if installed', or 'don't block on missing tool'."
source: scan
---
<!-- mustard:generated at:2026-05-19T00-00-00 role:general -->

## Convention

- `fs_ops::read_json_object(path)` never errors. Absent file, I/O failure, malformed JSON, and non-object root all return an empty `Map`. Use it for every config file read.
- `fs_ops::merge_json(path, &[("key", value)])` reads via the above, applies patches, and writes back. Unmodified keys are preserved verbatim.
- External-tool probes return `Option<String>`, never `Result`. Shape: `Command::new(tool).args([…]).output().ok()? → None` on any failure.
- The global `~/.claude/settings.json` is never written unless `MUSTARD_GLOBAL_PERMISSIONS=1`. Always check the opt-in env var before mutating user home.
- RTK installation failures and `ensure_global_permissions` failures are warnings, never errors. Use `unwrap_or_else(|err| eprintln!("[mustard] warning: …"))`.
- Interactive prompts check `std::io::stdin().is_terminal()` and fall back to the safe default when stdin is not a TTY.

## Real examples in this codebase

- `apps/cli/src/fs_ops.rs` — `read_json_object` (lines 111-120) and `merge_json` (lines 87-105)
- `apps/cli/src/commands/git_flow.rs` — `git()` helper (lines 84-90): fail-open git probe
- `apps/cli/src/commands/init.rs` — `has_github_remote()` (lines 297-306): fail-open remote check
- `apps/cli/src/commands/init.rs` — `ensure_rtk()` (lines 451-471): warn-only RTK install
- `apps/cli/src/commands/init.rs` — `ensure_global_permissions()` (lines 350-421): opt-in guard
- `apps/cli/src/commands/init.rs` — TTY guard (lines 200-204)

## References

See `references/examples.md` for verbatim code excerpts.
