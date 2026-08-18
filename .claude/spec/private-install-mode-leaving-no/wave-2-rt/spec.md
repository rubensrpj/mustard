---
id: wave.private-install-mode-leaving-no.2-rt
---

# wave-2-rt

## Summary

Make the runtime honour the mode: Guards to the local instruction file, the --private flag on the bootstrap door, and autodetection so the flag is needed once.

## Network

- Parent: [[spec.private-install-mode-leaving-no]]
- Depends on: [[wave.private-install-mode-leaving-no.1-core]]

## Tasks

- [ ] In `apps/rt/src/commands/scan_claude.rs`, `run_full` writes each subproject's Guards to `<sub>/CLAUDE.local.md` when the install is private, and must NOT read, rewrite or otherwise touch `<sub>/CLAUDE.md`. Claude Code discovers `CLAUDE.local.md` in a subdirectory exactly as it discovers `CLAUDE.md` there — on demand when a file in that directory is read — and appends it AFTER the shared file in the same directory, so a host repository's own Guards survive and ours are additive. The workspace root is already skipped entirely (line 537) and stays skipped.
- [ ] The `@.claude/scan-map.md` import line keeps working unchanged: an import resolves relative to the file that contains it, and the local file sits in the same directory. Do not rewrite the import.
- [ ] Implement autodetection in ONE place the whole runtime reads: the install is private when the clone-local exclude file carries the footprint rules wave 1 declared. No entry in `mustard.json` and no environment variable — a versioned knob would be the very trace this unit removes, and an env var is state the operator has to remember. Cache it per process the way `project_config_cached` already caches config.
- [ ] Add `--private` to `MaintCmd::Upsert` in `apps/rt/src/commands/maint/cli.rs` and to its dispatch arm — the crate's guard is FOUR registrations, so also update the locked list in `apps/rt/tests/run_command_surface.rs` and satisfy the reverse ratchet in `tests/template_parity.rs` with a real caller or a justified whitelist line.
- [ ] `apps/rt/src/commands/maint/upsert.rs` passes the mode to `upsert_project` and prints the grown report. Keep the `run` face contract: deterministic byte-stable JSON, fail-open, exit 0 always.
- [ ] Write `apps/rt/tests/private_scan.rs::ac5_private_scan_writes_local_guards_and_never_touches_claude_md` and `apps/rt/tests/private_surface.rs::ac6_upsert_accepts_private_flag_and_mode_is_autodetected`, named exactly so — the criteria filter on those tokens. AC-6 must prove BOTH halves: the flag is accepted on the first run, and a SECOND run with no flag still behaves privately.

## Files

- `apps/rt/src/commands/scan_claude.rs`
- `apps/rt/src/commands/maint/cli.rs`
- `apps/rt/src/commands/maint/upsert.rs`
- `apps/rt/tests/run_command_surface.rs`
- `apps/rt/tests/private_scan.rs`
- `apps/rt/tests/private_surface.rs`

## Reality Obligations

- **RO-2.1** — Confirm against the official Claude Code memory documentation that a `CLAUDE.local.md` in a SUBDIRECTORY is discovered and loaded the same way a subdirectory `CLAUDE.md` is, and that within one directory the local file is appended after the shared one. The whole wave rests on it: if the local file were root-only, per-subproject Guards would silently stop loading and nothing in this repository's tests would notice.
