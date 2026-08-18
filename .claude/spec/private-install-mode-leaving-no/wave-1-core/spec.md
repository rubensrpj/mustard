---
id: wave.private-install-mode-leaving-no.1-core
---

# wave-1-core

## Summary

Teach the install engine a private mode: a clone-local exclude writer, local-layer settings, and a report that names already-tracked residue.

## Network

- Parent: [[spec.private-install-mode-leaving-no]]

## Tasks

- [ ] Add `packages/core/src/platform/git_exclude.rs`. It resolves the exclude file by running `git rev-parse --git-path info/exclude` from the project root — NEVER the literal `.git/info/exclude`, which does not exist in a submodule or a linked worktree where `.git` is a file. It appends only the rule lines the file lacks (compare on the trimmed line, like `missing_ignore_patterns` in project_seed.rs already does), under an attributing header. It also answers whether a given path is already tracked, via `git ls-files`. Fail-open throughout: no git, no repository, or an unreadable exclude degrades to 'nothing excluded' and is REPORTED, never an error and never a panic (`unwrap`/`expect` are deny outside tests).
- [ ] Add the mode as an enum in `platform/` — two variants, shared (today's behaviour) and private. Thread it through `upsert_project(root, version, mode)`. Do not add a knob to `mustard.json`: the mode is a caller argument here, and the persistence decision lives in wave 2.
- [ ] In `seed_settings`, target `.claude/settings.local.json` when the mode is private. Compose that path in `ClaudePaths` beside `settings_json_path()` — it is the single owner of `.claude/` path composition, so no call site joins it by hand.
- [ ] Grow `UpsertReport` with the private outcome: whether the run was private, the exclude rules it appended, and the footprint paths the host repository ALREADY tracks. Keep the report deterministic and byte-stable — fixed field order, no timestamps, project-root-relative names only; new fields skip serialization when empty so a shared install's JSON is unchanged.
- [ ] The footprint list the private mode excludes is derived, not hand-typed: the four paths `upsert_project` writes, plus `CLAUDE.md`/`CLAUDE.local.md` and `.github/pull_request_template.md`. Declare it in ONE place that the wave-4 proof can read back.
- [ ] Write `packages/core/tests/private_install.rs` with the four test functions named EXACTLY as the acceptance criteria name them (`ac1_private_upsert_writes_clone_local_exclude`, `ac2_private_upsert_seeds_local_settings`, `ac3_already_tracked_paths_are_reported_not_unlinked`, `ac4_shared_install_is_byte_identical_to_today`) — the criteria filter on those tokens, and a filter that matches nothing reports 0 passed and reads as a pass. Use a real git repository via tempfile, like `packages/core/tests/seeded_ignore.rs` does; ask git, never the template.
- [ ] AC-4 is the regression guard and must be written as one: run a SHARED upsert and assert the created path list and file bytes are what they are today — the mode must be invisible to every ordinary project.

## Files

- `packages/core/src/platform/git_exclude.rs`
- `packages/core/src/platform/project_seed.rs`
- `packages/core/src/platform/mod.rs`
- `packages/core/src/io/claude_paths.rs`
- `packages/core/src/lib.rs`
- `packages/core/tests/private_install.rs`

## Reality Obligations

- **RO-1.1** — Run `git rev-parse --git-path info/exclude` for real in three shapes — an ordinary repository, a submodule, and a linked worktree — and record what each returns. The project's own `plugin/refs/git/submodule-rules.md` asserts the literal path fails in a submodule, but the resolved value is git's semantics, not this repository's, and the whole mechanism rests on it.
