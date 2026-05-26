# rt-impl Agent Memory

- [Wave numbering: 0-based](project_wave_numbering_0based.md) — currentWave in resume-bootstrap is 0-based (wave-0-* dirs); event_projections defaults to 1 — must override from completed_waves
- [Workspace: no src-tauri](project_workspace_no_src_tauri.md) — apps/dashboard/src-tauri purged in commit 189a414; Cargo.toml workspace must exclude it or builds fail
- [W2 left test break in enforce_registry](project_w2_enforce_registry_test_break.md) — hooks/enforce_registry.rs tests still need `use std::path::Path` after W2 sweep; check passes, test compile fails
- [W5 test leak root cause](project_w5_test_leak_root_cause.md) — hook tests with ctx.project_dir="." or empty input.cwd leaked apps/rt/.claude/; fix = treat "." as no-cwd, gate emits
