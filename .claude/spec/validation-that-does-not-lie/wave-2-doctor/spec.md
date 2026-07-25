---
id: wave.validation-that-does-not-lie.2-doctor
---

# wave-2-doctor

## Summary

The uncurated-scaffold census reaches a health report and a dispatched agent stops receiving a placeholder as its rules

## Network

- Parent: [[spec.validation-that-does-not-lie]]

## Tasks

- [ ] In apps/rt/src/commands/scan_guards/list.rs, lift the pending-scaffold walk into a shared collector the listing command and a health check both call. One walk, two projections. Do NOT write a second walk with its own ignore list - that would be the third copy of the same traversal in this crate.
- [ ] Add an advisory health check that consults that collector and reports which subprojects still carry an uncurated rules scaffold. Follow the rt-check-pattern skill exactly, and surface it through the existing doctor --check selector rather than publishing a new run subcommand, so the locked command surface does not change. Advisory only, never blocking, silent no-op when there is no census.
- [ ] In apps/rt/src/commands/agent/render/sections.rs, read_guards_block copies the rules block into a dispatched agent's prompt verbatim. When the block still carries the pending sentinel, the agent receives a placeholder as its guidance with nothing marking it. Make the dispatch say so. Follow the rt-inject-pattern contract: fail-open, a missing source yields no injection, never panic.
- [ ] Two top-level integration tests in apps/rt/tests/, named doctor_reports_uncurated_rule_scaffolds and dispatch_warns_on_uncurated_rules. They MUST be top-level #[test] fns, NOT inside a cfg(test) mod, for the same --exact reason. Each two-sided: a pending scaffold is reported AND a curated one is not, so neither can pass by reporting everything.
