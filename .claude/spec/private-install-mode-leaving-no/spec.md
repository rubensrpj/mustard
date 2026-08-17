---
id: spec.private-install-mode-leaving-no
---

# Mustard must be installable into a client repository without leaving any versioned trace: nothing it writes — at the root or in any subproject — may appear in that repo's git

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

Mustard must be installable into a client repository without leaving any versioned trace: nothing it writes — at the root or in any subproject — may appear in that repo's git.

Why now: the harness is being used on a consulting engagement, inside a repository the operator does not own. Every install today plants five versioned paths in that repository, and every full scan writes one more per subproject — a Guards file next to the subproject's code. That footprint is versioned deliberately: Mustard assumes the repository is the operator's own, where a spec is the unit's record and Guards are the team's shared knowledge. On a client engagement the assumption inverts, and today there is no way to say so.

The obvious workaround fails on the one file that matters. An ignore entry in the project's own ignore file is itself a versioned line that announces the hidden tool, and a clone-local exclude rule only acts on paths git does not already track — so a client repository that already versions its own Guards file sees it reported as modified after every scan, no matter how many rules exist. This project has already recorded that exact limit once, in the spec `harness-obstructs-its-own-work`: the ignore list does not affect already-tracked files. What the mode needs is not a better rule but a different destination: write beside the client's file rather than into it, on the untracked layer the editor already reads.

## Users/Stakeholders

The operator installing Mustard into a repository they do not own — a consultant, a contractor, anyone working inside a client's codebase — plus that client, who receives commits containing only their own code. Nobody else's workflow changes: a shared install stays byte-identical to what it is today.

## Success Metric

In a host repository that already versions its own `CLAUDE.md`, a private install followed by a full scan and a complete spec cycle leaves `git status --porcelain --untracked-files=all` EMPTY, and the host's own `CLAUDE.md` byte-identical to what it was before.

## Non-Goals

- Moving the footprint out of the project directory. The harness reads `.claude/settings.json` and the per-directory instruction files from disk at fixed locations; relocating them would break the hooks. Private means invisible to git, not absent from the working tree.
- Unlinking paths a host repository already tracks. `git rm --cached` rewrites the client's index — the report names the residue and the command that clears it; the operator decides.
- Rewriting history. A footprint already committed in an earlier session stays in the log; this changes what happens from the install forward.
- Changing branch names. `feature/…` and `fix/…` still travel on push — they are not files, and no file-level mechanism hides them.
- Any change to the shared (non-private) install. Same bytes, same paths, same report.

## Acceptance Criteria

- **AC-1** — when a private install runs in a repository, then the footprint rules land in the clone-local exclude file resolved through git (never the literal `.git/info/exclude`), appended idempotently so a second run adds nothing.
  Command: `cargo test -p mustard-core --test private_install ac1_private_upsert_writes_clone_local_exclude` Expect: `[1-9][0-9]* passed`
- **AC-2** — when a private install seeds the harness settings, then they land in `.claude/settings.local.json` and `.claude/settings.json` is never created.
  Command: `cargo test -p mustard-core --test private_install ac2_private_upsert_seeds_local_settings` Expect: `[1-9][0-9]* passed`
- **AC-3** — when a footprint path is ALREADY tracked in the host repository, then the report names it as residue and the install unlinks nothing.
  Command: `cargo test -p mustard-core --test private_install ac3_already_tracked_paths_are_reported_not_unlinked` Expect: `[1-9][0-9]* passed`
- **AC-4** — when a shared (non-private) install runs, then every path and byte it writes is identical to today's — the mode changes nothing for an ordinary project.
  Command: `cargo test -p mustard-core --test private_install ac4_shared_install_is_byte_identical_to_today` Expect: `[1-9][0-9]* passed`
- **AC-5** — when `scan --full` runs under a private install, then each subproject's Guards are written to `<sub>/CLAUDE.local.md` and an existing `<sub>/CLAUDE.md` is left byte-identical.
  Command: `cargo test -p mustard-rt --test private_scan ac5_private_scan_writes_local_guards_and_never_touches_claude_md` Expect: `[1-9][0-9]* passed`
- **AC-6** — when `mustard-rt run upsert --private` is invoked, then the flag is accepted and the emitted report carries the private outcome; the mode is thereafter autodetected with no flag and no versioned setting.
  Command: `cargo test -p mustard-rt --test private_surface ac6_upsert_accepts_private_flag_and_mode_is_autodetected` Expect: `[1-9][0-9]* passed`
- **AC-7** — when `mustard init --private` is invoked, then the install is private and the `.github/` pull-request template is not seeded into the host repository.
  Command: `cargo test -p mustard-cli --test private_init ac7_init_private_seeds_no_github_template` Expect: `[1-9][0-9]* passed`
- **AC-8** — when a host repository that ALREADY versions its own `CLAUDE.md` receives a private install plus a full scan plus a spec directory, then `git status --porcelain --untracked-files=all` comes back EMPTY against real git and that `CLAUDE.md` is byte-identical to before.
  Command: `cargo test -p mustard-core --test private_install_leaves_no_trace ac8_host_repo_stays_clean_and_untouched` Expect: `[1-9][0-9]* passed`
- **AC-9** — the project build passes green
  Command: `cargo build --workspace`

<!-- PLAN -->

## Files

- `packages/core/src/platform/git_exclude.rs` (create) — Resolves the clone-local exclude path through git and appends missing rule lines idempotently; also answers "is this path already tracked here?".
- `packages/core/src/platform/project_seed.rs` — `upsert_project` takes the mode; `seed_settings` targets the local settings file when private; `UpsertReport` grows the private outcome.
- `packages/core/src/io/claude_paths.rs` — the local-settings path joins its shared twin, so no call site composes it by hand.
- `packages/core/src/platform/mod.rs` — registers the new module.
- `packages/core/src/lib.rs` — re-exports what `apps/` consumes.
- `packages/core/tests/private_install.rs` (create) — AC-1 to AC-4.
- `packages/core/tests/private_install_leaves_no_trace.rs` (create) — AC-8, the field proof against real git.
- `apps/rt/src/commands/scan_claude.rs` — `run_full` picks the local instruction file when the install is private.
- `apps/rt/src/commands/maint/cli.rs` — `--private` on the `Upsert` variant plus its dispatch arm.
- `apps/rt/src/commands/maint/upsert.rs` — passes the mode through and reports it.
- `apps/rt/tests/run_command_surface.rs` — the locked command surface admits the new flag.
- `apps/rt/tests/private_scan.rs` (create) — AC-5.
- `apps/rt/tests/private_surface.rs` (create) — AC-6.
- `apps/cli/src/cli.rs` — `--private` on `Init`.
- `apps/cli/src/commands/init.rs` — carries the mode into the seeders and skips the `.github/` copy when private.
- `apps/cli/tests/private_init.rs` (create) — AC-7.

## Boundaries

IN: how the install DECIDES between shared and private; where the four seeds and the per-subproject Guards are written under each; the clone-local exclude write and its already-tracked report; the two flags (`mustard init --private`, `mustard-rt run upsert --private`) and the autodetection that makes them one-time.

OUT: the location of the footprint (it stays in the working tree — see Non-Goals); unlinking or rewriting anything the host repository already tracks or already committed; branch naming; any behaviour change to a shared install; the `.claude/.gitignore` template's contents, which keep covering runtime scratch exactly as today.

## Definitions

- **private install mode** — an install of Mustard whose files exist on the project's disk — the harness needs them there — but are never visible to that clone's git: nothing to stage, nothing to diff, nothing to push
- **local layer** — the untracked counterpart Claude Code already defines for each shared instruction file: CLAUDE.local.md beside CLAUDE.md, settings.local.json beside settings.json. Both are read the same way as their shared twin
- **ensure-excluded** — idempotently appending the missing rule lines to the clone-local exclude file, whose path is resolved with `git rev-parse --git-path info/exclude` rather than spelled literally
- **host repo** — the repository Mustard is installed INTO — a consulting client's codebase here — as opposed to Mustard's own repository

## Decisions

- the mode switch lives in no versioned file: it is chosen once through a flag and thereafter autodetected from the clone-local exclude file
  Reason: a knob in mustard.json would itself be the versioned trace the mode exists to remove — the setting would announce the tool it hides. Reading the exclude file makes the state self-evident and removes a knob nobody has to remember
- in private mode the per-subproject Guards are written to <sub>/CLAUDE.local.md instead of <sub>/CLAUDE.md
  Reason: an ignore rule only acts on an UNTRACKED path. A host repo that already versions its own CLAUDE.md would show ' M CLAUDE.md' on every scan no matter how many exclude lines exist. Writing beside the file instead of into it is the only way the client's own file is never touched
- in private mode the harness settings are seeded into .claude/settings.local.json instead of .claude/settings.json
  Reason: it is the local layer Claude Code documents for exactly this, the template .gitignore already covers it, and this repository already writes machine-local statusLine there — the pattern is precedent, not invention
- the exclude file is reached through `git rev-parse --git-path info/exclude`, never as the literal path .git/info/exclude
  Reason: in a submodule or a linked worktree `.git` is a FILE, not a directory, so the literal path does not exist and the write silently lands nowhere. The project already states this rule for its own git flows
- already-tracked paths are reported, not silently unlinked
  Reason: `git rm --cached` rewrites the host repo's index — a consequence the operator must choose. Reporting names the residue and the one command that clears it; doing it unasked would mutate a client repository on a flag that reads as install-time cosmetics

## Evidence

- upsert_project writes exactly four seeds into the host repo: .claude/settings.json, .claude/mustard/orchestrator.md, .claude/.gitignore and the project-root mustard.json
  Evidence: `packages/core/src/platform/project_seed.rs:136`
- the seeded .claude/.gitignore covers only regenerable runtime output; a spec's own spec.md and qa/report.md stay versioned on purpose
  Evidence: `packages/core/templates/.gitignore:45`
- seed_gitignore is the one seed that merges by LINE, so a rule added to the template later still reaches an already-installed project
  Evidence: `packages/core/src/platform/project_seed.rs:335`
- scan --full writes each subproject's Guards into <sub>/CLAUDE.md, and only rewrites when the content actually changed
  Evidence: `apps/rt/src/commands/scan_claude.rs:562`
- the workspace root's own CLAUDE.md is already never touched by the scan — only subprojects get one, so the private-mode change is scoped to subprojects
  Evidence: `apps/rt/src/commands/scan_claude.rs:537`
- `run upsert` accepts no flags at all today: MaintCmd::Upsert is an empty variant
  Evidence: `apps/rt/src/commands/maint/cli.rs:166`
- `mustard init` accepts only --force, --yes and --dry-run — there is no private/local option anywhere in the CLI surface
  Evidence: `apps/cli/src/cli.rs:33`
- CLAUDE.local.md appears nowhere in the codebase — the local instruction layer is net-new here (verified by a repository-wide grep for the literal)
  Evidence: `apps/rt/src/commands/scan_claude.rs:470`
- settings.local.json is NOT net-new: the statusline heal observer already writes machine-local statusLine into it, so the local-settings layer has working precedent in this codebase
  Evidence: `apps/rt/src/hooks/session/statusline_heal_observer.rs:78`
- the template .gitignore already carries settings.local.json, so a private-mode settings file is covered the moment it is written
  Evidence: `packages/core/templates/.gitignore:16`
- the project already documents the submodule-safe exclude protocol — resolve via rev-parse, append grep-guarded, and unlink already-tracked paths with `git rm --cached` — but only applies it to five ephemeral runtime paths, never to the install footprint
  Evidence: `plugin/refs/git/submodule-rules.md:62`
- the CLI seeds .github/pull_request_template.md into the host repo when it detects a GitHub remote — a fifth versioned path, outside .claude/ entirely
  Evidence: `apps/cli/src/commands/init.rs:434`
- ClaudePaths is the single owner of .claude/ path composition, so a private-mode settings path belongs there and not in ad-hoc joins at each call site
  Evidence: `packages/core/src/io/claude_paths.rs:354`
- the seeded-ignore test proves coverage against real git rather than by reading the template, and derives its path list from the writers in the code plus this repository's own field record — the same proof shape a private-mode test must take
  Evidence: `packages/core/tests/seeded_ignore.rs:93`