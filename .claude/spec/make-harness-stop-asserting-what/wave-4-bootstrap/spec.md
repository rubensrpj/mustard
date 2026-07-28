---
id: wave.make-harness-stop-asserting-what.4-bootstrap
---

# wave-4-bootstrap

## Summary

Resume bootstrap and the dependency precheck stop implying what they did not check — one about progress, the other about a stack it cannot parse.

## Network

- Parent: [[spec.make-harness-stop-asserting-what]]
- Depends on: [[wave.make-harness-stop-asserting-what.1-proof]]

## Tasks

- [ ] Distinguish a plan that was scaffolded and never dispatched from a plan waiting on its first wave: directory count alone must not read as progress. The dispatch record, not the filesystem, decides which of the two it is.
- [ ] Fix the module doc that states wave directories are 0-based while the code twenty lines below implements 1-based — one of the two is read by whoever edits next.
- [ ] Surface the precheck's own skip marker at the caller: an unsupported stack already reports that it declined to judge, and that sentence must reach whoever is about to dispatch instead of being read as a clean pass.

## Files

- `apps/rt/src/commands/pipeline/resume_bootstrap/wave_progress.rs`
- `apps/rt/src/commands/pipeline/resume_bootstrap/mod.rs`
- `apps/rt/src/commands/review/dependency_precheck.rs`
