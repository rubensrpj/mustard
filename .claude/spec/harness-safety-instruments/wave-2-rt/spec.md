---
id: wave.harness-safety-instruments.2-rt
---

# wave-2-rt

## Summary

The health check and the reporting command stop validating against sources nothing writes

## Network

- Parent: [[spec.harness-safety-instruments]]

## Tasks

- [ ] In apps/rt/src/commands/doctor/doctor.rs, stop declaring KNOWN_EVENTS by hand. Derive the set from the shipped plugin/hooks/hooks.json manifest, mirroring the known_run_subcommands idiom thirty lines below it. Degrade without panic when the manifest is unreadable.
- [ ] Add a test named known_events_match_shipped_hooks that fails when the doctor's event set and the shipped manifest disagree in either direction, so the drift cannot return.
- [ ] In apps/rt/src/commands/economy/metrics.rs, make collect_specs read the live event log instead of pipeline_states_dir. Reuse the existing event projection rather than writing a second reader. When no source is readable, publish an explicit unknown marker instead of zero.
- [ ] Add a test named metrics_collect_reports_specs_from_events that seeds spec event logs in a tempdir and asserts the collect document counts them, and a second assertion that an unreadable source yields the unknown marker and not zero.
