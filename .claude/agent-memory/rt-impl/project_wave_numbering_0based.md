---
name: project_wave_numbering_0based
description: Wave directories are 0-based (wave-0-*, wave-1-*); event_projections defaults currentWave to 1; resume_bootstrap must re-derive from completed_waves
metadata:
  type: project
---

Wave directories in Mustard are 0-based: `wave-0-{role}`, `wave-1-{role}`, etc.

The `event_projections::PipelineStateView.current_wave` defaults to `1` (legacy 1-based) when no `pipeline.wave_complete` events exist.

Fix (2026-05-25, spec `resume-bootstrap-wave-index`): `resume_bootstrap.rs` now re-derives `current_wave` directly from `completed_waves` — `max(completed_waves) + 1`, defaulting to `0` when empty.

**Why:** The FS layout uses 0-based indices but the event store was written with a 1-based convention. Trusting `v.current_wave` skipped wave-0 on fresh specs.

**How to apply:** Any code reading `current_wave` from a `PipelineStateView` for FS path lookups should apply the same `completed_waves.iter().max().map_or(0, |&m| m + 1)` derivation instead of using the field directly.
