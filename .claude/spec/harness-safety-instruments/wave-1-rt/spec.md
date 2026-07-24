---
id: wave.harness-safety-instruments.1-rt
---

# wave-1-rt

## Summary

Kill-switch keeps the safety net and actually stops the hooks; the shipped allowlist drops the grant the platform refuses

## Network

- Parent: [[spec.harness-safety-instruments]]

## Tasks

- [ ] In apps/rt/src/commands/maint/unhook.rs, replace the whole-file rename in disable_one with a surgical write: read settings.json, set "disableAllHooks": true, write it back atomically via mustard_core::io::fs::write_atomic. Keep the volatile-state wipe and every DisabledEntry state string; add a `hooks_disabled` boolean rather than repurposing `moved_to`.
- [ ] In apps/rt/src/commands/maint/rehook.rs, mirror the inverse in restore_one: remove the disableAllHooks key when present. Keep the legacy settings.json.disabled-* restore path so a project unhooked by the old build can still be recovered, and keep every existing state string.
- [ ] Remove the two protected-path entries from packages/core/templates/settings.json permissions.allow and from this project's .claude/settings.json. Change nothing else in either file.
- [ ] Add a test named unhook_disables_hooks_without_dropping_permissions that seeds a tempdir settings.json carrying a permissions.deny list and a statusLine, runs the disable path, and asserts the file still exists with deny and statusLine intact AND disableAllHooks true. Add its rehook counterpart asserting the key is gone and the rest survives.
