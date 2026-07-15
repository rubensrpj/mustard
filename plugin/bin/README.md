# bin/

Per-platform native binaries for the Mustard harness. **Populated by the release
workflow, not committed** — this directory ships only `.gitkeep` + this README in
source control.

When the plugin is enabled, `bin/` is prepended to the Bash tool `PATH`, and
Claude Code auto-resolves the correct binary for the host OS. The release stamps:

- `mustard-rt` / `mustard-rt.exe` — the enforcement runtime (hooks call it as
  `"${CLAUDE_PLUGIN_ROOT}/bin/mustard-rt" on <Event>`; the MCP server is
  `mustard-rt mcp`).
- `scan` / `scan.exe` — the deterministic grain miner.

## Version stamping

At author time `plugin.json` `version` mirrors `CARGO_PKG_VERSION` from
`apps/rt/Cargo.toml` (currently `0.1.0`, a dev placeholder). The cross-platform
release workflow that stamps the real release version into the binaries MUST stamp
the same value into `plugin.json` `version`, so plugin update semantics track the
shipped binary.
