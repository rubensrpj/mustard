# apps/cli — `mustard-cli`

Subproject guard for the Mustard CLI crate. Auto-loaded when working under `apps/cli/`.

## What this is

The `mustard-cli` crate — the installer CLI. It generates and updates the `.claude/`
folder in target projects. Entry point `src/main.rs`; subcommands in `src/commands/`
(`init`, `update`, `config`, `add`, `review`, `git_flow`).

## Build & test

```bash
cargo build -p mustard-cli
cargo test  -p mustard-cli
cargo run   -p mustard-cli -- init     # scaffold .claude/ in cwd
cargo run   -p mustard-cli -- update   # regenerate core files
```

## `templates/` is payload, not source

`apps/cli/templates/` is a **verbatim payload** copied into a target project's
`.claude/` by `init`/`update` — it is not compiled. `templates/CLAUDE.md` is the
*orchestrator template* for generated projects, **not** this crate's guard (this
file is). When the agnostic detector relies on `CLAUDE.md` presence to mark
subprojects, this file is what keeps `apps/cli` from being mis-detected as
`templates`.

`resolve_templates_dir()` (`commands/init.rs`) locates the payload at runtime:
`MUSTARD_TEMPLATES_DIR` env → next to the exe → `CARGO_MANIFEST_DIR/templates`.

## Stack

| Item | Value |
|------|-------|
| Language | Rust (edition 2024, MSRV 1.85) |
| Crate | `mustard-cli` v3.1.36 |
| Binary | `mustard` |
| Key deps | `clap`, `anyhow`, `serde_json`, `dialoguer`, `ureq`, `tar`+`flate2`, `mustard-core` |

## Commands

```bash
cargo build -p mustard-cli
cargo test -p mustard-cli
cargo clippy -p mustard-cli
cargo run -p mustard-cli -- init
cargo run -p mustard-cli -- init --yes --dry-run
cargo run -p mustard-cli -- update --force
cargo run -p mustard-cli -- config --yes
cargo run -p mustard-cli -- add template:<name>
cargo run -p mustard-cli -- review --pr <number> [--ci]
```

## Guards

- Editing files under `templates/` changes what every future `mustard init` ships
  — treat them as released artifacts, not scratch code.
- `update` recreates only `CORE_FOLDERS` (`commands/mustard`, `hooks`, `skills`,
  `scripts`, `refs`) and preserves user files — keep that split intact.
- Code, comments and doc-comments in EN; surgical edits only.
- Every new subcommand must follow the `<Name>Options` struct + split entry-point pattern.
- Use `fs_ops::merge_json` for all JSON config writes — never `fs::write(json!({…}))`.
- Never write to `~/.claude/settings.json` without checking `MUSTARD_GLOBAL_PERMISSIONS=1`.
- `#![forbid(unsafe_code)]` is workspace policy — no exceptions.

## Recommended Skills

- `cli-command-pattern` — Options struct + split entry-point pattern for subcommands
- `cli-failopen-pattern` — fail-open JSON reads, shell-out probes, opt-in global mutations

## Scan References

| File | Description |
|------|-------------|
| `.claude/commands/stack.md` | Rust stack, crate manifest, lint policy |
| `.claude/commands/modules.md` | Module map, public exports, subcommand routing |
| `.claude/commands/patterns.md` | 6 recurring patterns with file references |
| `.claude/commands/guards.md` | DO/DON'T rules by category |
| `.claude/commands/recipes.md` | Step-by-step recipes for common tasks |
| `.claude/skills/cli-command-pattern/` | Options struct + split entry-point skill |
| `.claude/skills/cli-failopen-pattern/` | Fail-open JSON/shell-out/opt-in guard skill |
