---
name: cli-command-pattern
description: "Options struct + split entry-point pattern used by every mustard-cli subcommand. Use when adding a new subcommand, refactoring an existing command, wiring up the Tauri backend to a command, or testing a command in isolation. Even if the user just says 'add command', 'new subcommand', or 'port this command'."
source: scan
---
<!-- mustard:generated at:2026-05-19T00-00-00 role:general -->

## Convention

- Every subcommand lives in `src/commands/<name>.rs` and exposes exactly two public symbols: a `<Name>Options` struct and a `pub fn <name>` entry function.
- `<Name>Options` is `Debug + Default + Clone` — no `clap` derives, no env reads.
- The entry function signature is `pub fn <name>(project_path: &Path, opts: &<Name>Options) -> Result<()>`.
- `cli.rs` builds the struct from `clap` args and passes it to the function — zero logic in `cli.rs`.
- Each command that needs the bundled `templates/` directory exposes a second `pub fn <name>_with_templates(project_path, templates_dir, opts)` variant. The plain variant resolves templates via `resolve_templates_dir()` and delegates; the `_with_templates` variant is what tests and Tauri call.
- `lib.rs` re-exports only `init`, `update`, `InitOptions`, `UpdateOptions`, and `VERSION`. All other symbols stay `pub(crate)`.

## Real examples in this codebase

- `apps/cli/src/commands/init.rs` — `InitOptions` struct (lines 37-46), `init()` (line 65), `init_with_templates()` (line 76)
- `apps/cli/src/commands/update.rs` — `UpdateOptions` struct (lines 43-48), `update()` (line 60), `update_with_templates()` (line 69)
- `apps/cli/src/commands/config.rs` — simplest variant: `ConfigOptions` + thin wrapper over `git_flow`
- `apps/cli/src/cli.rs` — dispatch table: constructs options from clap args, calls entry functions (lines 100-125)

## References

See `references/examples.md` for verbatim code excerpts.
