# rt-impl Agent Memory

- [Wave numbering: 0-based](project_wave_numbering_0based.md) — currentWave in resume-bootstrap is 0-based (wave-0-* dirs); event_projections defaults to 1 — must override from completed_waves
- [Workspace: no src-tauri](project_workspace_no_src_tauri.md) — apps/dashboard/src-tauri purged in commit 189a414; Cargo.toml workspace must exclude it or builds fail
