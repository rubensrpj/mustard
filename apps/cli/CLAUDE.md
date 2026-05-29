# Cli

> Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)

<!-- mustard:generated at:2026-05-29T00:00:00Z role:general -->

## Stack

Rust (edition 2024, MSRV 1.85), crate `mustard-cli` v3.1.36 — binary `mustard` + library `mustard_cli` (linked natively by the Tauri dashboard). Rust port of the original TypeScript/Bun CLI (epic B5). Deps: `clap`, `serde`/`serde_json`, `anyhow`, `dialoguer`, `ureq`, `tar`+`flate2`, `zip`. Scaffolds and updates the `.claude/` folder. `#![forbid(unsafe_code)]`; workspace lints (`unwrap_used` = deny). `templates/` and `templates-extras/` are data payloads, never compiled.

Subcommands: `init` · `update` · `config` · `add` · `review` · `install-nerd-font` · `install-grammars`.

## Commands

| Task | Command |
|------|---------|
| Build | `rtk cargo build -p mustard-cli` |
| Check | `rtk cargo check -p mustard-cli` |
| Test | `rtk cargo test -p mustard-cli` |
| Lint | `rtk cargo clippy -p mustard-cli` |
| Docs/type-check | `rtk cargo doc -p mustard-cli --no-deps` |
| Run | `rtk cargo run -p mustard-cli -- <subcommand>` |

No migration/codegen step. See `.claude/commands/recipes.md` for env vars and full run recipes.

## Guards

- Write the single config to `<root>/mustard.json` (the workspace anchor) — never `.claude/mustard.json`. `mustard_core::ProjectConfig` owns the schema.
- Keep `.claude` in the `copy_dir` `skip_top_level` list — avoid the `.claude/.claude/` nesting bug (I1 rule).
- Probe external tools fail-open (`--version` + `is_ok_and(status.success())`) and degrade with install hints. RTK is the only hard gate (`probe_rtk` exits 1). Never make rg/gh/brew/scoop blocking.
- `update --force` skips the prompt, never the backup.
- Read JSON fail-open (`read_json_object`); merge with `entry().or_insert_with()`; write via `mfs::write_atomic` + trailing newline. Don't clobber user keys.
- Don't mutate `~/.claude/settings.json` unless `MUSTARD_GLOBAL_PERMISSIONS=1`.
- No language/framework identifier in `.rs` source — language data lives in `templates/grammars-suggestions.json`; commands come from `detect_commands`.
- Validate template/skill names (`[A-Za-z0-9_-]`, reject `..`) before FS/network use.
- No `unwrap`/`expect` outside `#[cfg(test)]`. Keep `main.rs` thin; all logic in the library.

Full DO/DON'T list: `.claude/commands/guards.md`.

## Scan References

- [.claude/commands/stack.md](.claude/commands/stack.md) — language, edition, deps, binary/lib layout
- [.claude/commands/modules.md](.claude/commands/modules.md) — per-module map (entry flow, command table)
- [.claude/commands/patterns.md](.claude/commands/patterns.md) — 8 recurring conventions with refs
- [.claude/commands/guards.md](.claude/commands/guards.md) — full DO/DON'T rules
- [.claude/commands/recipes.md](.claude/commands/recipes.md) — build/test/lint + run recipes + env vars

## Recommended Skills

- `cli-subcommand-module` — Options struct + entry fn dispatched from `cli.rs`; use when adding a subcommand.
- `cli-failopen-tool-probe` — fail-open `--version` probe + best-effort install of external binaries.
- `cli-surgical-json-merge` — fail-open read, `entry().or_insert_with()` merge, atomic write; recursive copy.
