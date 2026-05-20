<!-- mustard:generated at:2026-05-19T00-00-00 role:general -->
# Modules: mustard-cli

## Public module map

| Module | Path | Responsibility |
|--------|------|----------------|
| `cli` | `src/cli.rs` | `clap` parser + subcommand dispatch (`run()`) |
| `commands` | `src/commands/mod.rs` | One module per subcommand |
| `commands::init` | `src/commands/init.rs` | `mustard init` — scaffold `.claude/` |
| `commands::update` | `src/commands/update.rs` | `mustard update` — refresh core files |
| `commands::config` | `src/commands/config.rs` | `mustard config` — thin wrapper over `git_flow` |
| `commands::git_flow` | `src/commands/git_flow.rs` | Git-flow detection + `mustard.json` generation |
| `commands::add` | `src/commands/add.rs` | `mustard add` — fetch + install community template |
| `commands::auto_update` | `src/commands/auto_update.rs` | `mustard auto-update` — npm registry check + install |
| `commands::review` | `src/commands/review.rs` | `mustard review` — Claude API PR review |
| `fs_ops` | `src/fs_ops.rs` | `copy_dir`, `merge_json`, `read_json_object` |
| `npm` | `src/npm.rs` | `get_latest_version`, `update_global`, `compare_versions` |
| `runtime` | `src/runtime.rs` | `RuntimeInfo` — host OS/arch stamped into `mustard.json` |

## Public exports from `lib.rs`

| Symbol | Re-exported from |
|--------|-----------------|
| `init` | `commands::init::init` |
| `update` | `commands::update::update` |
| `InitOptions` | `commands::init::InitOptions` |
| `UpdateOptions` | `commands::update::UpdateOptions` |
| `VERSION` | `env!("CARGO_PKG_VERSION")` — compile-time constant |

## Subcommand → module mapping

| CLI command | Module | Entry fn |
|------------|--------|----------|
| `mustard init` | `commands::init` | `init(project_path, &InitOptions)` |
| `mustard update` | `commands::update` | `update(project_path, &UpdateOptions)` |
| `mustard config` | `commands::config` → `commands::git_flow` | `config(project_path, &ConfigOptions)` |
| `mustard auto-update` | `commands::auto_update` | `auto_update(&AutoUpdateOptions)` |
| `mustard add <template>` | `commands::add` | `add(cwd, template_spec, &AddOptions)` |
| `mustard review` | `commands::review` | `review(cwd, &ReviewOptions)` |

Ref: `apps/cli/src/lib.rs`, `apps/cli/src/cli.rs`, `apps/cli/src/commands/mod.rs`
