---
id: wave.build-test-cycle-is-too.3-profile
---

# wave-3-profile

## Summary

Bound the incremental cache by a policy declared in the repository, so 52 GB cannot accumulate again while everyone waits for someone to remember a command.

## Network

- Parent: [[spec.build-test-cycle-is-too]]
- Depends on: [[wave.build-test-cycle-is-too.1-measure]]

## Tasks

- [ ] Read wave 1's incremental on/off comparison. It decides the policy: if the cache buys less time than it costs on this workspace, the dev profile turns it off; if it buys real time, the profile keeps it and the bound comes from elsewhere. Write the chosen policy into Cargo.toml with a comment stating the measured reason, in the style of the profile comments already there.
- [ ] Whichever way the number falls, the policy must be DECLARED in the repository rather than inherited from a default — the defect is that nothing in the tree has an opinion about a directory that grew to 52 GB.
- [ ] Leave `[profile.dev.package."*"] opt-level = 1` alone unless wave 1's numbers say it costs more than it saves on this workspace. Its comment claims it cuts the cold-build tail; if the measurement refutes that claim, change the setting AND the comment together, and never the comment alone.
- [ ] Do not touch the release profile's lto or codegen-units — the spec puts them out of bounds.

## Files

- `Cargo.toml`
