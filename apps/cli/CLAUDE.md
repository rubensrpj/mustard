@.claude/scan-map.md

# Cli

> Parent: [../../CLAUDE.md](../../CLAUDE.md) | Orchestrator: [../../.claude/mustard/orchestrator.md](../../.claude/mustard/orchestrator.md)



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
