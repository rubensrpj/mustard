All nine ACs run green, the full CI suite is green — and the wave still ships one blocking seam defect.

## Verified claims (each run, real output)

| AC | Command | Result |
|---|---|---|
| AC-1..4 | `cargo test -p mustard-core --test private_install <name>` | 4× `1 passed` |
| AC-5 | `cargo test -p mustard-rt --test private_scan ac5_…` | `1 passed` |
| AC-6 | `cargo test -p mustard-rt --test private_surface ac6_…` | `1 passed` |
| AC-7 | `cargo test -p mustard-cli --test private_init ac7_…` | `1 passed` |
| AC-8 | `cargo test -p mustard-core --test private_install_leaves_no_trace ac8_…` | `1 passed` |
| AC-9 | `cargo build --workspace` | `0 errors, 1 warning` (pre-existing, `feature.rs:488`, untouched by this wave) |
| regression | `cargo test --locked -p mustard-core -p mustard-cli -p mustard-rt -p scan` | `2916 passed, 4 ignored (72 suites)` |
| lints | `cargo clippy --workspace --all-targets` | exit 0 (pedantic warnings only) |

The tests are honest: each carries a negative control (shared install visible to git, shared scan writing into `CLAUDE.md`, shared `run upsert` producing no `private` key), each asks real `git` instead of reading a constant, and `git_status` returns a failing sentinel when the measurement did not happen. Both mid-pipeline change requests are really in the code: `packages/core/tests/private_install_leaves_no_trace.rs:57,65` puts a `packages/api/.claude/` subproject and a `.claude.backup.<stamp>/` under the empty-status assertion, and the shared control at line 118-130 proves both are otherwise visible.

Guards + mold: clean. No new `run` subcommand, so the four-registration rule does not bite (the surface test locks names, not flags); `MaintCmd::Upsert` keeps `display_order = 44`, the flag has help prose, the dispatch arm is wired (`apps/rt/src/commands/maint/cli.rs:236`) — `rt-cmd-pattern` respected. No `unwrap`/`expect` outside tests. The report stays byte-stable: the four new fields skip when empty and the `unavailable` reasons are deliberately path-free.

## BLOCKING — the writer moved, every reader stayed

`apps/rt/src/commands/scan_claude.rs:487` now writes the subproject Guards to `CLAUDE.local.md` in private mode. Nothing that CONSUMES those Guards was taught the new name — a repo-wide grep for `CLAUDE.local.md` returns only `context.rs`, `scan_claude.rs` and the tests:

- `apps/rt/src/commands/agent/render/sections.rs:34` — `read_guards_block` reads `subproject_dir.join("CLAUDE.md")` unconditionally. This is the `## GUARDS` block inlined into every dispatched agent prompt. Under a private install it returns the *client's* file — or `""`, which `collapse_empty_sections` then deletes. Agents get no Guards.
- `apps/rt/src/commands/agent/render/role.rs:271` — `read_guards_facts` loses the `<!-- facts: kind=…; frameworks=… -->` grounding the enrich pass needs.
- `apps/rt/src/hooks/write/post_edit.rs:629` — `governing_subproject` locates the owning unit by finding a `CLAUDE.md`; in a client repo without one it returns `None` and the guard-reminder gate goes silent.
- `apps/rt/src/commands/scan_guards/list.rs:124` — the guards census only recognises `CLAUDE.md`, so the curation loop never sees a private install's pending blocks.

AC-5 only asserts the file *lands*; code presence is not effectiveness. The spec Decision claims "their Guards survive and ours are additive" — true for Claude Code's memory loader, false for Mustard's own injection, which is the mechanism the Guards exist for. A private install therefore ships a harness whose central artifact is inert. Fix shape: one resolver honouring `install_mode` (prefer `CLAUDE.local.md`, fall back to `CLAUDE.md`), used by those four readers, plus a criterion that dispatches a prompt under a private install and asserts the GUARDS text is present.

## Non-blocking

- MAJOR `apps/cli/src/commands/init.rs:147` — `init` picks the mode from the flag ALONE; `run upsert` autodetects (`upsert.rs`), `init` does not. A plain `mustard init` re-run over a private project re-seeds `.claude/settings.json` and `.github/`; both stay git-invisible, but Claude Code then merges two settings layers (hooks registered twice). The spec's "chosen once, thereafter autodetected" is half-implemented.
- MINOR `apps/cli/src/commands/init.rs:374` — `backup_dirs()` discovery is redundant since wave 4 added the `.claude.backup.*/` wildcard to `footprint_paths()`; no test exercises it, and every init appends one more per-timestamp line to the exclude file.
- MINOR `packages/core/src/platform/project_seed.rs:169` — the bare `CLAUDE.md` rule (there for the `ls-files` residue report) also lands in the exclude file, hiding the client's own future untracked `CLAUDE.md` at any depth from the operator's clone.
