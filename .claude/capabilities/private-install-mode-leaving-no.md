---
id: cap.private-install-mode-leaving-no
status: active
---

# private install mode leaving no

### Requirement: The system SHALL satisfy the acceptance criteria of spec private-install-mode-leaving-no.

#### Scenario: AC-1
- when: a private install runs in a repository
- then: the footprint rules land in the clone-local exclude file resolved through git (never the literal `.git/info/exclude`), appended idempotently so a second run adds nothing.
- command: `cargo test -p mustard-core --test private_install ac1_private_upsert_writes_clone_local_exclude`

#### Scenario: AC-2
- when: a private install seeds the harness settings
- then: they land in `.claude/settings.local.json` and `.claude/settings.json` is never created.
- command: `cargo test -p mustard-core --test private_install ac2_private_upsert_seeds_local_settings`

#### Scenario: AC-3
- when: a footprint path is ALREADY tracked in the host repository
- then: the report names it as residue and the install unlinks nothing.
- command: `cargo test -p mustard-core --test private_install ac3_already_tracked_paths_are_reported_not_unlinked`

#### Scenario: AC-4
- when: a shared (non-private) install runs
- then: every path and byte it writes is identical to today's — the mode changes nothing for an ordinary project.
- command: `cargo test -p mustard-core --test private_install ac4_shared_install_is_byte_identical_to_today`

#### Scenario: AC-5
- when: `scan --full` runs under a private install
- then: each subproject's Guards are written to `<sub>/CLAUDE.local.md` and an existing `<sub>/CLAUDE.md` is left byte-identical.
- command: `cargo test -p mustard-rt --test private_scan ac5_private_scan_writes_local_guards_and_never_touches_claude_md`

#### Scenario: AC-6
- when: `mustard-rt run upsert --private` is invoked
- then: the flag is accepted and the emitted report carries the private outcome; the mode is thereafter autodetected with no flag and no versioned setting.
- command: `cargo test -p mustard-rt --test private_surface ac6_upsert_accepts_private_flag_and_mode_is_autodetected`

#### Scenario: AC-7
- when: `mustard init --private` is invoked
- then: the install is private and the `.github/` pull-request template is not seeded into the host repository.
- command: `cargo test -p mustard-cli --test private_init ac7_init_private_seeds_no_github_template`

#### Scenario: AC-8
- when: a host repository that ALREADY versions its own `CLAUDE.md` receives a private install plus a full scan plus a spec directory
- then: `git status --porcelain --untracked-files=all` comes back EMPTY against real git and that `CLAUDE.md` is byte-identical to before.
- command: `cargo test -p mustard-core --test private_install_leaves_no_trace ac8_host_repo_stays_clean_and_untouched`

#### Scenario: AC-10
- when: a dispatch prompt is rendered under a private install
- then: the inlined `## GUARDS` block carries the Guards from the local layer — the mode moves the Guards' destination, never their reach
- command: `cargo test -p mustard-rt --test private_guards ac10_private_dispatch_prompt_carries_the_guards`

#### Scenario: AC-11
- when: a private install cannot hide its footprint in a repository that EXISTS — the clone-local exclude file is unreadable or unwritable — then the install REFUSES and writes nothing, instead of laying the footprint down visibly while reporting itself private
- then: 
- command: `cargo test -p mustard-core --test private_install ac11_private_install_refuses_when_it_cannot_hide`

#### Scenario: AC-9
- when: 
- then: the project build passes green
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.private-install-mode-leaving-no]]

## Related

