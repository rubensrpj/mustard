## Verdict — packages/core, round 2: APPROVED (0 critical)

All 9 criteria green, no regressions, Guards and molds clean. Two non-blocking majors.

### Claims verified (command → real output)

| Claim | Command | Result |
|---|---|---|
| AC-1 exclude write + idempotent | `cargo test -p mustard-core --test private_install ac1_…` | `1 passed` |
| AC-2 local-layer settings only | `… ac2_private_upsert_seeds_local_settings` | `1 passed` |
| AC-3 tracked residue reported, not unlinked | `… ac3_already_tracked_paths_are_reported_not_unlinked` | `1 passed` |
| AC-4 shared install byte-identical | `… ac4_shared_install_is_byte_identical_to_today` | `1 passed` |
| AC-5 private scan → `CLAUDE.local.md` | `cargo test -p mustard-rt --test private_scan ac5_…` | `1 passed` |
| AC-6 `--private` + autodetect | `… --test private_surface ac6_…` | `1 passed` |
| AC-7 no `.github/` seed | `cargo test -p mustard-cli --test private_init ac7_…` | `1 passed` |
| AC-8 host repo clean + untouched | `… --test private_install_leaves_no_trace ac8_…` | `1 passed` |
| AC-10 Guards reach the prompt | `… --test private_guards ac10_…` | `1 passed` |
| AC-9 build | `cargo build --workspace` | `0 errors` (1 pre-existing warning) |
| No regression | `cargo test -p mustard-core` / `-p mustard-rt -p mustard-cli` | `647 passed` / `2124 passed`, 0 failed |

**Independent field proof (not an AC).** Real repo, real binary: `run upsert --private` → `run scan --full` → `git status --porcelain -uall` came back EMPTY; the client's CRLF `packages/api/CLAUDE.md` md5-identical; `CLAUDE.local.md` written; a second `run upsert` with NO flag still reported `"private": true`. Then, authoring client files after the install, git still SEES `CLAUDE.md`, `services/billing/CLAUDE.md`, `services/billing/mustard.json`, `packages/api/mustard.json` — the previous round's CRITICAL (over-broad bare-name rules) is genuinely closed by the `FootprintEntry` rule/pathspec/written split. Also confirmed empirically that a linked worktree's `git rev-parse --git-path info/exclude` resolves to the common dir and that exclusions there really apply.

**Guards**: writes go through `io::fs::write_atomic`; `Error::NotFound` matched apart from `Error::Io`; no `unwrap`/`expect` outside tests; `mustard.json` still only via `ProjectConfig`, and the mode is deliberately stored in NO versioned file; `domain/model/` untouched. **Molds**: no skill claims these types; `ExcludeOutcome` follows the file's own `SeedOutcome`/`MigrationOutcome` precedent.

**Change requests**: all four addressed, each verified in substance rather than by claim.

### MAJOR — the dispatch prompt still names the shared file in prose
`apps/rt/src/commands/agent/agent_prompt_template.md:4` renders ``1. Read the `## Guards` section of `{subproject}/CLAUDE.md``` unconditionally. Field render under a private install produced exactly that line while the inlined `## GUARDS` block correctly came from `CLAUDE.local.md` — so the Guards reach the agent (AC-10 is honest), but every dispatched agent is told to open the CLIENT's file. Same "a call site spells the filename itself" shape the change request targeted; it survived because it is a template literal, not a `join`.

### MAJOR — `**/.claude/` also hides files the CLIENT authors
Measured in the field repo: after a private install, `.claude/commands/their-command.md` (written by hand, nothing to do with Mustard) is invisible to `git status -uall`. Already-tracked `.claude/` files stay visible, so committed client work is safe, and the rule is documented and deliberate — but under a `git add -A` law a new client file under `.claude/` is silently never committed.

### MINOR
- `packages/core/src/platform/project_seed.rs:401` — `fn is_false(b: &bool)` adds a new `trivially_copy_pass_by_ref` clippy warning (signature forced by serde's `skip_serializing_if`). CI runs clippy without `-D warnings`.
- `git_exclude` degrades to "nothing was excluded" when the exclude file is unreadable/unwritable in a REAL repository, and the install then lays the footprint down visibly, reporting only `excludeUnavailable`/one printed line. No criterion covers that branch.
