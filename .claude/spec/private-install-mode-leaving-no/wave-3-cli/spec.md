---
id: wave.private-install-mode-leaving-no.3-cli
---

# wave-3-cli

## Summary

Give the installer face the same one-time choice: mustard init --private, and no .github/ scaffolding in a repository that is not the operator's.

## Network

- Parent: [[spec.private-install-mode-leaving-no]]
- Depends on: [[wave.private-install-mode-leaving-no.1-core]]

## Tasks

- [ ] Add `--private` to the `Init` variant in `apps/cli/src/cli.rs` and carry it through `InitOptions` into `apps/cli/src/commands/init.rs`.
- [ ] `init` passes the mode to the core seeders (wave 1's signature) instead of calling them with today's fixed behaviour.
- [ ] Skip `install_github_templates` entirely when private: the pull-request template is project scaffolding for a repository the operator owns, and it lands OUTSIDE `.claude/` where nothing else covers it.
- [ ] The interactive prompts stay as they are. Do not add a question about the mode — `--private` is the whole surface, and a prompt would put the choice in front of every ordinary install that does not need it.
- [ ] Write `apps/cli/tests/private_init.rs::ac7_init_private_seeds_no_github_template`, named exactly so. Seed a temp project with a GitHub remote — the condition that makes the template copy fire today — and assert the private path writes no `.github/`.

## Files

- `apps/cli/src/cli.rs`
- `apps/cli/src/commands/init.rs`
- `apps/cli/tests/private_init.rs`
