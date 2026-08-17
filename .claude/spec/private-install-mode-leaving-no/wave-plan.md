---
id: wave.private-install-mode-leaving-no.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.private-install-mode-leaving-no.1-core]] | core | — | Teach the install engine a private mode: a clone-local exclude writer, local-layer settings, and a report that names already-tracked residue. |
| 2 | [[wave.private-install-mode-leaving-no.2-rt]] | rt | [[wave.private-install-mode-leaving-no.1-core]] | Make the runtime honour the mode: Guards to the local instruction file, the --private flag on the bootstrap door, and autodetection so the flag is needed once. |
| 3 | [[wave.private-install-mode-leaving-no.3-cli]] | cli | [[wave.private-install-mode-leaving-no.1-core]] | Give the installer face the same one-time choice: mustard init --private, and no .github/ scaffolding in a repository that is not the operator's. |
| 4 | [[wave.private-install-mode-leaving-no.4-proof]] | proof | [[wave.private-install-mode-leaving-no.1-core]], [[wave.private-install-mode-leaving-no.2-rt]], [[wave.private-install-mode-leaving-no.3-cli]] | Ask real git the whole question: a host repo that already versions its own CLAUDE.md must stay byte-identical and report nothing after a private install, a scan and a spec. |

## Acceptance Criteria
- AC-1 — when a private install runs in a repository, then the footprint rules land in the clone-local exclude file resolved through git (never the literal `.git/info/exclude`), appended idempotently so a second run adds nothing. Command: `cargo test -p mustard-core --test private_install ac1_private_upsert_writes_clone_local_exclude` Expect: `[1-9][0-9]* passed`
- AC-2 — when a private install seeds the harness settings, then they land in `.claude/settings.local.json` and `.claude/settings.json` is never created. Command: `cargo test -p mustard-core --test private_install ac2_private_upsert_seeds_local_settings` Expect: `[1-9][0-9]* passed`
- AC-3 — when a footprint path is ALREADY tracked in the host repository, then the report names it as residue and the install unlinks nothing. Command: `cargo test -p mustard-core --test private_install ac3_already_tracked_paths_are_reported_not_unlinked` Expect: `[1-9][0-9]* passed`
- AC-4 — when a shared (non-private) install runs, then every path and byte it writes is identical to today's — the mode changes nothing for an ordinary project. Command: `cargo test -p mustard-core --test private_install ac4_shared_install_is_byte_identical_to_today` Expect: `[1-9][0-9]* passed`
- AC-5 — when `scan --full` runs under a private install, then each subproject's Guards are written to `<sub>/CLAUDE.local.md` and an existing `<sub>/CLAUDE.md` is left byte-identical. Command: `cargo test -p mustard-rt --test private_scan ac5_private_scan_writes_local_guards_and_never_touches_claude_md` Expect: `[1-9][0-9]* passed`
- AC-6 — when `mustard-rt run upsert --private` is invoked, then the flag is accepted and the emitted report carries the private outcome; the mode is thereafter autodetected with no flag and no versioned setting. Command: `cargo test -p mustard-rt --test private_surface ac6_upsert_accepts_private_flag_and_mode_is_autodetected` Expect: `[1-9][0-9]* passed`
- AC-7 — when `mustard init --private` is invoked, then the install is private and the `.github/` pull-request template is not seeded into the host repository. Command: `cargo test -p mustard-cli --test private_init ac7_init_private_seeds_no_github_template` Expect: `[1-9][0-9]* passed`
- AC-8 — when a host repository that ALREADY versions its own `CLAUDE.md` receives a private install plus a full scan plus a spec directory, then `git status --porcelain --untracked-files=all` comes back EMPTY against real git and that `CLAUDE.md` is byte-identical to before. Command: `cargo test -p mustard-core --test private_install_leaves_no_trace ac8_host_repo_stays_clean_and_untouched` Expect: `[1-9][0-9]* passed`
- AC-9 — the project build passes green. Command: `cargo build --workspace`
