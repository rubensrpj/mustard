<!-- mustard:generated at:2026-05-19T00-00-00 role:general -->
# Guards: mustard-cli

## Structural rules

| DO | DON'T |
|----|-------|
| Add subcommands with a `<Name>Options` struct + `pub fn <name>` entry point | Put subcommand logic in `cli.rs` — dispatch only |
| Use `init_with_templates` / `update_with_templates` in tests and Tauri | Call `init` / `update` from tests (mutates real env) |
| Re-export only `init`, `update`, `InitOptions`, `UpdateOptions` from `lib.rs` | Expose internal helpers through `lib.rs` — they are `pub(crate)` by design |
| Use `anyhow::Context` to attach file paths to I/O errors | Return bare `io::Error` from fs helpers |

## Templates / payload rules

| DO | DON'T |
|----|-------|
| Treat `templates/` as a released artifact — changes ship in the next CLI release | Treat `templates/` as scratch — edits affect every future `mustard init` |
| Keep `CORE_FOLDERS` as the single list of Mustard-owned dirs in `update.rs` | Hard-code folder names at individual `remove_dir_all` call sites |
| Preserve user files outside `CORE_FOLDERS` on `update` | Delete or overwrite `CLAUDE.md`, `docs/`, `spec/`, `memory/`, `entity-registry.json` |

## Fail-open rules

| DO | DON'T |
|----|-------|
| Use `read_json_object` for all JSON reads — treats absent/malformed as `{}` | `unwrap()` or propagate errors from config file reads |
| Wrap `ensure_global_permissions` / `ensure_rtk` in `unwrap_or_else` — failures are warnings | Block `init`/`update` on RTK install failures |
| Check `MUSTARD_GLOBAL_PERMISSIONS=1` before writing to `~/.claude/settings.json` | Write global settings unconditionally |
| Use `Command::new("git").output().ok()?` pattern for git probes | `bail!` on missing git or non-repo directories |

## Versioning rules

| DO | DON'T |
|----|-------|
| Read `VERSION` from `env!("CARGO_PKG_VERSION")` — compile-time constant | Read version from a file at runtime |
| Let `update` re-stamp only `version` in `.claude/mustard.json` — `runtime` is owned by `init` | Re-write `runtime` block during `update` |
| Write `mustard.json` via `merge_json` to preserve existing keys | `fs::write(json!({...}))` which drops unrelated keys |

## Lint / style rules

| DO | DON'T |
|----|-------|
| Write `#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]` in test modules | Use `unwrap` / `expect` in production paths |
| Keep `#![forbid(unsafe_code)]` on every source file | Add `unsafe` blocks — workspace lint forbids them |
| Write comments and doc-strings in English | Mix PT/EN in code or doc-comments |
