//! `mustard init` — thin bootstrap for a Claude Code project (Mustard 2.0).
//!
//! The heavy `.claude/` payload — commands, skills, agents, refs, hooks — now
//! ships in the **`mustard` plugin**, distributed through a private git
//! marketplace. `init` no longer copies that payload; it lays down the small
//! set of files a plugin cannot ship, then enables the plugin. The flow:
//!
//! 1. probe RTK — a hard gate (the harness prefixes every Bash call with `rtk`);
//!    then guard the location: init refuses a directory that sits inside a git
//!    repository without being its root (the workspace resolver anchors on git
//!    roots — see [`guard_init_location`]);
//! 2. handle an already-present `.claude/` (force-overwrite, merge, or
//!    backup-then-overwrite — interactively prompted when no flag decides it);
//! 3. seed the harness into `.claude/` — delegated to the core seeding engine
//!    (`mustard_core::platform::project_seed`, fed by the compiled-in
//!    `platform::seeds` constants; `mustard-rt run upsert` consumes the same
//!    engine):
//!    - `settings.json` — the reduced SEED (env / permissions / statusLine /
//!      plansDirectory …); plugin enablement is NOT planted (user-scope
//!      choice) and the broken pair an older build wrote is retired
//!      (`mustard_core::retire_planted_plugin_enablement`);
//!    - `mustard/*.md` — the injectable instruction files: the router's three
//!      parts, all on `userPromptSubmit` — `orchestrator.md` (intent routing),
//!      `dispatch.md` (the unit's question, the base gate, the naming) and
//!      `material.md` (what the conversation settled, written when it is
//!      settled). One SIBLING HOOK each, because ONE hook RESPONSE carries at
//!      most 10,000 characters of `additionalContext`, and siblings on one
//!      event are separate responses that share no budget (measured
//!      2026-08-25); the session hooks splice them into the agent's window per
//!      `mustard.json#inject` — **no `CLAUDE.md` is planted anymore** (a
//!      planted orchestrator drowned in large root files; injection always
//!      lands);
//!    - `.gitignore` — covers the ephemeral harness state;
//!    - migration: a legacy Mustard-planted `.claude/CLAUDE.md` (identified by
//!      its `# Orchestrator Rules` marker) is deleted, and the Mustard import
//!      + breadcrumb lines are removed from the project-root `CLAUDE.md` —
//!        the file goes back to being fully the user's;
//! 4. copy `templates/.github/` → project-root `.github/` when a GitHub remote
//!    is detected (project-level scaffolding, not part of the plugin);
//! 5. write the single project-root `mustard.json`: git-flow + agnostically
//!    detected build/test/lint/type-check commands + spec language + tone +
//!    the `runtime`/`version` stamp + the default `inject` declarations
//!    (seeded only when the user has none — a curated list is preserved);
//! 6. settle that stamp (`mustard_core::record_version_stamp`): where the host
//!    repository TRACKS `mustard.json`, an install that found a clean tree
//!    commits the line it just wrote, so the install never hands the next
//!    command a dirty file to blame on the operator. A tree that already held
//!    the operator's work is left untouched, and the reason is printed.
//!
//! There is **one** config file, at the **project root** (the workspace anchor
//! `workspace_root` keys on) — never `.claude/mustard.json`. The `version`
//! stamp lets the dashboard read the installed Mustard version; because `init`
//! is idempotent, **re-running it re-stamps that version** — the job the retired
//! `mustard update` used to do.
//!
//! `.mcp.json` is deliberately **not** written: the `mustard` plugin ships its
//! own `.mcp.json`, so a project-level copy is redundant once the plugin is
//! enabled.
//!
//! ## `--private`
//!
//! `mustard init --private` installs the same harness under
//! `mustard_core::InstallMode::Private`: every file above still lands on disk —
//! the harness needs it there — but none of it is visible to the host
//! repository's git. Three differences, all decided by the mode:
//!
//! - a step 0 runs before anything is written, adding
//!   `mustard_core::footprint_rules` to the clone-local exclude file and
//!   reporting whatever that repository already tracks (see [`hide_footprint`]);
//! - the harness settings go to `.claude/settings.local.json`, the untracked
//!   local layer Claude Code reads beside `settings.json` — the core picks the
//!   destination from the mode, so no call site composes it by hand;
//! - step 4 (the `.github/` pull-request template) is skipped entirely: it is
//!   scaffolding for a repository the operator owns, and it lands outside
//!   `.claude/` where nothing else covers it.
//!
//! The mode lives in no versioned file — a knob in `mustard.json` would itself
//! be the trace the mode exists to remove. It is CHOSEN once with `--private`
//! and thereafter AUTODETECTED off the clone-local exclude file that choice
//! wrote (`mustard_core::detect_install_mode`), exactly as `mustard-rt run
//! upsert` resolves it. Both halves are load-bearing: without the flag there is
//! no way to say it the first time, and without the autodetection a plain
//! `mustard init` re-run over a private project would re-seed the versioned twin
//! of a file it had already hidden — two settings layers, hooks registered
//! twice. There is no prompt: an ordinary install is never asked a question it
//! does not need.
//! What is NOT a step of `init`, though it still happens on a `mustard init`
//! run: the global-permissions write and the RTK/ripgrep installers. Both live
//! in `cli::dispatch`, because they act on the MACHINE and a library call must
//! never take that on its caller's behalf — three reviews in a row found that
//! same shape in this file. `apps/cli/tests/library_is_pure.rs` measures it.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use dialoguer::Select;
use dialoguer::theme::ColorfulTheme;
use mustard_core::io::fs as mfs;
use serde_json::json;

use crate::commands::git_flow;
use crate::fs_ops::copy_dir;
use mustard_core::{InstallMode, ProjectConfig, Runtime, SeedOutcome};

/// Flags accepted by `mustard init`.
#[derive(Debug, Default, Clone)]
pub struct InitOptions {
    /// Overwrite an existing `.claude/` without a backup.
    pub force: bool,
    /// Accept defaults without prompting.
    pub yes: bool,
    /// Print intended actions without touching disk.
    pub dry_run: bool,
}

/// What to do with an already-present `.claude/` directory.
enum ExistingAction {
    /// Overwrite (the `--force` path, or the interactive "backup" choice once
    /// the backup has been taken).
    Overwrite,
    /// Keep user edits: seed only the files that are absent, but still merge the
    /// plugin-enable keys into `settings.json`.
    Merge,
    /// Abort without writing.
    Cancel,
}

/// Run `mustard init` against `project_path`.
///
/// This is the library entry point the dashboard backend calls. The binary passes
/// the process working directory; a caller may pass any folder. The bundled
/// `templates/` directory is located via [`resolve_templates_dir`]; callers
/// that already know its location use [`init_with_templates`].
pub fn init(project_path: &Path, options: &InitOptions) -> Result<InitOutcome> {
    let templates_dir = resolve_templates_dir()?;
    init_with_templates(project_path, &templates_dir, options)
}

/// What an `init` run actually DID, reported as a fact rather than folded into
/// `Result`.
///
/// The distinction is load-bearing and was found in review: `Ok(())` used to
/// cover both "the project was seeded" and "the operator answered Cancel", so a
/// caller that acted on success alone changed the machine after an explicit
/// refusal. A closed set of dispositions lets the caller judge; the callee keeps
/// its opinion to itself. Mold: `core-outcome-pattern`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitOutcome {
    /// The project was seeded: `.claude/` and `mustard.json` are on disk.
    Installed,
    /// An existing `.claude/` was found and the operator chose to stop.
    ///
    /// NOT "nothing was written": `hide_footprint` runs BEFORE the prompt, and a
    /// cancelled run was measured appending 31 rules to the project's
    /// `.git/info/exclude`. What this variant promises is narrower and is the
    /// part the caller needs — no install completed, so nothing about the
    /// MACHINE may change on this run's account.
    Cancelled,
    /// `--dry-run`: the plan was printed and no path was touched.
    DryRun,
}

/// [`init`] with the `templates/` directory supplied explicitly.
///
/// Splitting this out keeps template resolution (an environment concern) out
/// of the install logic, so tests can drive a fixture tree and the dashboard
/// backend can point at its own bundled payload — no process-global env var.
pub fn init_with_templates(
    project_path: &Path,
    templates_dir: &Path,
    options: &InitOptions,
) -> Result<InitOutcome> {
    // NO RTK PROBE HERE, and that is the point of this function's split.
    //
    // Probing `PATH` is an environment concern, which the doc above says this
    // half deliberately does not carry. It used to call `probe_rtk()`, and that
    // function ends in `std::process::exit(1)` — so this function, which
    // returns `Result<()>`, could instead KILL ITS CALLER'S PROCESS. Any
    // library consumer lost the chance to handle it; a `Result` that sometimes
    // terminates the program is not a contract.
    //
    // Measured, not theorised: `apps/dashboard/server/tests/mustard_cli_test.rs`
    // died as "test exited abnormally" the first time CI ran it (this crate had
    // never been in the CI test set), because a clean runner has no `rtk` and
    // the process simply vanished mid-test. The `cfg!(test)` escape hatch in
    // `probe_rtk` does not reach it: that flag is true only while THIS crate
    // compiles its own unit tests, never for an integration test living in
    // another crate.
    //
    // The gate itself is not softened — it moved to `cli::dispatch`, where the
    // terminal user still meets it before any disk write. See `probe_rtk`.

    let project_path = project_path
        .canonicalize()
        .with_context(|| format!("resolving project path {}", project_path.display()))?;
    let claude_path = project_path.join(".claude");
    // Unconditional, and both install faces spell it the same way. There is no
    // flag, no config key and no detection step that could answer otherwise:
    // installing the harness INTO someone else's repository is not an outcome
    // this command can be steered towards, by anyone, including by forgetting.
    let mode = InstallMode::Private;

    // Location guard — runs in dry-run too: the honest "intended action" for a
    // subdirectory of a git repository is a refusal, not a simulated install.
    guard_init_location(&project_path)?;

    println!("\nMustard\n");

    let runtime = Runtime::detect();
    println!("[mustard] runtime: {} {}/{}", runtime.kind, runtime.os, runtime.arch);

    if options.dry_run {
        if mode.is_private() {
            println!(
                "  (dry-run) would install PRIVATELY: the harness settings would go to settings.local.json,"
            );
            println!(
                "            the footprint would be added to this clone's git exclude file, and no .github/ would be seeded"
            );
        }
        println!("  (dry-run) would seed the harness into {}:", claude_path.display());
        println!("    settings.json  — reduced seed (plugin enablement stays at user scope; a planted placeholder pair is retired)");
        println!("    mustard/*.md   — injectable instruction files (orchestrator, dispatch, material); hooks inject them per mustard.json#inject");
        println!("    .gitignore     — ephemeral harness state");
        println!("  (dry-run) would migrate a legacy Mustard-planted .claude/CLAUDE.md away (and remove the Mustard import/breadcrumb lines from the root CLAUDE.md)");
        println!(
            "  (dry-run) would write git-flow + commands + runtime/version + inject declarations to {}",
            project_path.join("mustard.json").display()
        );
        println!("  (dry-run) content payload (commands/skills/agents/refs) + .mcp.json now ship in the `mustard` plugin — not written");
        return Ok(InitOutcome::DryRun);
    }

    // Sampled BEFORE the first write — the only moment at which the operator's
    // work and this run's own writes can still be told apart. Read by the last
    // step, which settles the version stamp.
    let found_clean = mustard_core::worktree_is_clean(&project_path);

    // Step 0, private only: hide the footprint BEFORE any of it exists — before
    // `.claude/` is created, and before the backup-and-overwrite branch below
    // can leave a `.claude.backup.<stamp>/` beside it. A refusal here writes
    // nothing at all, not even a directory.
    if mode.is_private() {
        hide_footprint(&project_path)?;
    }

    // Decide how to treat an existing `.claude/`. A fresh project is a plain
    // overwrite of an empty tree.
    let overwrite = if claude_path.exists() {
        match decide_existing_action(&claude_path, options)? {
            ExistingAction::Cancel => {
                println!("\n  Cancelled.\n");
                // Cancelled, NOT installed. The caller reads the difference: an
                // `Ok(())` here used to let `cli::dispatch` run the tool
                // installers after an explicit refusal, writing RTK's global
                // config on a run the operator stopped (found in review).
                return Ok(InitOutcome::Cancelled);
            }
            ExistingAction::Merge => false,
            ExistingAction::Overwrite => true,
        }
    } else {
        true
    };

    mfs::create_dir_all(&claude_path)
        .with_context(|| format!("creating {}", claude_path.display()))?;

    // Migration (idempotent, every run over an existing project): remove the
    // footprint the pre-injectable Mustard left in the project's instruction
    // files — the planted `.claude/CLAUDE.md` orchestrator and the import +
    // breadcrumb lines in the root `CLAUDE.md`. Runs BEFORE seeding so the
    // legacy layout is gone when the new one lands. Fail-open in the core
    // engine: any IO error degrades to "not migrated", never aborts the init.
    report_migration(&mustard_core::migrate_orchestrator_footprint(&project_path, &claude_path));

    // (a)+(e) settings.json: the reduced seed + the plugin-enablement retire —
    // the core engine owns the content (compiled-in seed), the merge rules and
    // the destination: a private install targets the untracked local layer
    // (`settings.local.json`) instead, and the shared file is never written.
    let settings_name = if mode.is_private() {
        ".claude/settings.local.json"
    } else {
        ".claude/settings.json"
    };
    let outcome = mustard_core::seed_settings(&claude_path, overwrite, mode)
        .with_context(|| format!("seeding {settings_name}"))?;
    report_seed(settings_name, outcome);
    // (b) injectable instruction files — the orchestrator is INJECTED by the
    // session hooks now (per `mustard.json#inject`), never planted as
    // `.claude/CLAUDE.md`.
    //
    // `overwrite` is deliberately NOT passed: every file the seed carries here
    // is the harness's own rules, not project configuration, so the answer to "merge or
    // overwrite?" is the same either way and the seeder takes no such argument.
    // Merge mode still means what it says for everything else init seeds —
    // `settings.json` above and `.gitignore` below keep the user's content.
    for (name, outcome) in mustard_core::seed_injectable_files(&claude_path)
        .context("seeding .claude/mustard/ injectables")?
    {
        report_seed(&format!(".claude/mustard/{name}"), outcome);
    }
    // (c) ephemeral-state .gitignore.
    let outcome = mustard_core::seed_gitignore(&claude_path, overwrite)
        .context("seeding .claude/.gitignore")?;
    report_seed(".claude/.gitignore", outcome);

    // (d) `.mcp.json` is intentionally NOT written — the `mustard` plugin ships
    // its own, so a project-level copy is redundant once the plugin is enabled.

    // Project-root `.github/` scaffolding (PR template) — not part of the
    // plugin, seeded only when the project has a GitHub remote. Never overwrites.
    //
    // A private install skips it outright. The template is project scaffolding
    // for a repository the operator OWNS, and it lands outside `.claude/`, where
    // nothing else covers it — writing it into a client's repository is exactly
    // the visible trace the mode exists to avoid.
    if mode.is_private() {
        println!("  skipped .github/ templates (private install — the host repository stays untouched)");
    } else {
        let gh = install_github_templates(templates_dir, &project_path)?;
        if gh > 0 {
            println!("  wrote {gh} GitHub template(s) at .github/");
        }
    }

    // `ensure_global_permissions` is NOT called here either — it writes
    // `$HOME/.claude/settings.json`, outside the project, which is the same
    // class of act as the installers below. A reviewer measured it happening
    // from a plain library call with `MUSTARD_GLOBAL_PERMISSIONS=1`; it sat
    // three lines above a comment forbidding exactly that. It now runs from
    // `cli::dispatch`, through `ensure_global_permissions_if_opted_in`.

    // NO TOOL INSTALLERS HERE — `ensure_rtk` / `ensure_ripgrep` live in
    // `cli::dispatch`, beside the gate, for the same reason the gate does.
    //
    // They were called from this spot, and moving the gate out without moving
    // them nearly shipped a worse bug than the one it fixed. While `probe_rtk`
    // exited at the top of this function, `ensure_rtk` could only ever run with
    // `rtk` ALREADY present — its install branch was unreachable from here. Take
    // the exit away and that branch goes live for every library and
    // integration-test caller, and it runs
    // `sh -c "curl … rtk/master/install.sh | sh"` on Unix, or `cargo install`
    // from git plus `cargo install ripgrep` on Windows.
    //
    // Measured in review, not argued: the dashboard's `mustard_cli_test` spawned
    // that curl pipeline TWICE under a logging `sh` shim. It would have run on
    // ubuntu, macOS and Windows runners, downloading and executing a remote
    // script inside a unit test, with no timeout.
    //
    // The verification that missed it is worth naming too: `PATH=/nonexistent`
    // makes every spawn fail instantly, so the installer degrades to printing
    // instructions — the one environment where this consequence cannot appear.
    // Re-measure this file with a SHIMMED PATH, never an empty one.

    // Write the single project-root mustard.json: git-flow + detected commands
    // + language/tone + runtime/version stamp. One file, one write. A re-run
    // re-stamps `version` — the idempotent replacement for `mustard update`.
    write_project_config(&project_path, &runtime, !options.yes)?;

    // …and settle the stamp that write just left behind. In a repository that
    // TRACKS `mustard.json` the re-stamp is an uncommitted change nobody asked
    // for, and the next command that guards on a clean tree refuses — naming
    // the operator's own work as the cause, when the writer was this installer.
    report_stamp(
        mustard_core::record_version_stamp(&project_path, found_clean),
        &project_path,
    );

    print_next_steps();
    Ok(InitOutcome::Installed)
}

/// Say what became of the version stamp, in the didactic voice the rest of the
/// install speaks. The ordinary run — an untracked or unchanged `mustard.json`
/// — says nothing at all: there was nothing to settle.
///
/// The recorded line NAMES the branch the commit landed on. A fresh clone is
/// checked out on the default branch, so the ordinary first install commits
/// there — the very branch `work_branch_gate` refuses to let the operator work
/// on. That asymmetry is deliberate (the stamp is project configuration, not
/// the operator's work, and refusing there would hand the dirty tree back in
/// the commonest case of all), and naming the branch is what keeps it a stated
/// outcome rather than a silent one.
fn report_stamp(outcome: mustard_core::RecordOutcome, root: &std::path::Path) {
    match outcome {
        mustard_core::RecordOutcome::Nothing => {}
        mustard_core::RecordOutcome::Recorded => {
            match mustard_core::current_branch(root) {
                Some(branch) => println!(
                    "  committed mustard.json on '{branch}' — the version stamp this install wrote"
                ),
                None => {
                    println!("  committed mustard.json — the version stamp this install wrote");
                }
            }
        }
        mustard_core::RecordOutcome::TreeNotClean => {
            println!(
                "  mustard.json carries a new version stamp, left UNCOMMITTED: this tree already"
            );
            println!(
                "  held your own changes, and they are never swept into an install's commit"
            );
        }
        mustard_core::RecordOutcome::Unavailable => {
            println!(
                "  mustard.json carries a new version stamp, left uncommitted (git could not record it)"
            );
        }
    }
}

/// Pre-flight location guard: refuse to init a directory that sits INSIDE a
/// git repository without being that repository's root.
///
/// Why: the workspace resolver (`mustard_core::io::workspace`) anchors on git
/// repository roots. A `mustard.json` + `.claude/` planted in a non-root
/// subdirectory would never win the resolution — it would only sit there as a
/// confusing phantom (the historical monorepo defect this guard closes).
///
/// Rules (filesystem probes only — fail-open, no `git` subprocess):
/// - the target IS a git repository root (`.git` as a directory, or as a file
///   for a submodule / linked worktree) → allow;
/// - the target lies inside a git repository but is not its root → refuse,
///   naming the repository root as the right place to init;
/// - no `.git` anywhere up the tree → allow with a note (projects without git
///   are supported through the resolver's loose fallback).
fn guard_init_location(project_path: &Path) -> Result<()> {
    use mustard_core::io::workspace::is_git_repo_root;

    if is_git_repo_root(project_path) {
        return Ok(());
    }
    let enclosing_root = project_path
        .ancestors()
        .skip(1)
        .find(|dir| is_git_repo_root(dir));
    let Some(repo_root) = enclosing_root else {
        println!(
            "  note: no git repository found here or above - proceeding (projects without git are supported)"
        );
        return Ok(());
    };
    anyhow::bail!(
        "this folder is inside a git repository, but it is not the repository's root.\n\
         Mustard anchors its workspace at the root of a git repository, so initializing here\n\
         would leave the harness state in the wrong place.\n\
         \n\
           repository root: {}\n\
         \n\
         Either run `mustard init` from that repository root, or - if this subfolder is meant\n\
         to be its own Mustard project - make it its own git repository (or a git submodule)\n\
         first, then re-run `mustard init` here.",
        repo_root.display()
    )
}

/// Private mode, step 0: hide the Mustard footprint from THIS clone's git, and
/// name whatever the host repository already tracks.
///
/// Mirrors the private step of `mustard_core::upsert_project` so the two
/// install faces never drift: the rules are [`mustard_core::footprint_rules`],
/// the residue question is asked with [`mustard_core::footprint_pathspecs`] (the
/// two are NOT the same list — a rule is a pattern, a pathspec is a path), the
/// write goes through the clone-local exclude layer (a path git resolves — never
/// the literal `.git/info/exclude`, which does not exist in a submodule or a
/// linked worktree), and an already-tracked path is REPORTED, never unlinked:
/// `git rm --cached` rewrites the host's index, and that is the operator's
/// decision, not an install-time cosmetic.
///
/// The residue report is SPLIT, and that split is the difference between advice
/// and damage. `git rm --cached` clears a file the install put there; aimed at
/// the client's own `CLAUDE.md` — which a private install never writes, because
/// the Guards go to `CLAUDE.local.md` beside it — the same command untracks
/// THEIR work, and their next commit deletes it. So only a path
/// [`mustard_core::is_written_footprint`] recognises is offered the command; the
/// host's own file is named for what it is and left alone.
///
/// One failure here is NOT narrated away, and it is the reason this function
/// returns a `Result` at all: when git resolved an exclude file in a real
/// repository and the write still did not land, the install refuses. Everything
/// after this point would then be written VISIBLY into a repository the operator
/// believes cannot see it — the one outcome this mode exists to prevent, and the
/// one an operator cannot notice for themselves. A tree with no repository is a
/// different thing entirely (there is nobody for a footprint to be visible to)
/// and still degrades to a printed line.
///
/// # Errors
///
/// [`mustard_core::ExcludeFailure::is_blocking`] — the exclude file could not be
/// read or written inside a repository that exists.
fn hide_footprint(project_path: &Path) -> Result<()> {
    let outcome = mustard_core::ensure_excluded(project_path, &mustard_core::footprint_rules());
    match (outcome.unavailable, outcome.appended.len()) {
        (Some(failure), _) if failure.is_blocking() => anyhow::bail!(
            "a private install must not write anything it cannot hide.\n\
             \n\
               {}\n\
             \n\
             Nothing was written. This clone's exclude file is where the footprint is hidden;\n\
             until it can be read and written, every file `mustard init` seeds would be visible\n\
             in this repository's `git status` while the install reported itself private.\n\
             Fix the file's permissions (or its type — it must be a FILE) and re-run.",
            failure.reason(),
        ),
        (Some(failure), _) => println!("  private install: {}", failure.reason()),
        (None, 0) => {
            println!("  private install: this clone's exclude file already carries every rule");
        }
        (None, count) => println!(
            "  private install: hid {count} path(s) from this clone's git (exclude file, never committed)"
        ),
    }

    let tracked =
        mustard_core::tracked_paths(project_path, &mustard_core::footprint_pathspecs());
    let (ours, theirs): (Vec<String>, Vec<String>) = tracked
        .into_iter()
        .partition(|path| mustard_core::is_written_footprint(path));
    if !ours.is_empty() {
        println!(
            "  note: this repository ALREADY tracks {} — a git ignore rule cannot hide a tracked path,",
            ours.join(", ")
        );
        println!("        so those stay visible. Nothing was unlinked; clear them yourself with:");
        println!("          git rm --cached {}", ours.join(" "));
    }
    for path in theirs {
        println!(
            "  note: {path} is the repository's OWN versioned file — a private install never \
             writes it, so it is left exactly as it is and no rule of ours hides it."
        );
    }
    Ok(())
}

/// Print one didactic line per seeded file. The seeding itself lives in the
/// core (`mustard_core::platform::project_seed`) — the CLI only narrates:
/// `Created`/`Updated` announce a write, `Preserved` confirms the user's file
/// survived the merge untouched.
fn report_seed(name: &str, outcome: SeedOutcome) {
    match outcome {
        SeedOutcome::Created | SeedOutcome::Updated => println!("  wrote {name}"),
        SeedOutcome::Preserved => println!("  kept {name} (yours, unchanged)"),
    }
}

/// Print the didactic lines for what the core migration engine
/// (`mustard_core::migrate_orchestrator_footprint`) found and did.
fn report_migration(outcome: &mustard_core::MigrationOutcome) {
    for entry in &outcome.migrated {
        match entry.as_str() {
            ".claude/CLAUDE.md" => println!(
                "  removed legacy .claude/CLAUDE.md (the orchestrator is injected from .claude/mustard/ now)"
            ),
            "CLAUDE.md" => println!(
                "  cleaned CLAUDE.md (removed the Mustard import + breadcrumb lines — the root file is fully yours again)"
            ),
            other => println!("  migrated {other}"),
        }
    }
    if outcome.foreign_claude_md {
        println!(
            "  note: .claude/CLAUDE.md exists but is not the Mustard orchestrator — left untouched (the file is yours; Mustard injects its rules from .claude/mustard/ instead)"
        );
    }
}

/// The `templates/` payload shipped beside `exe`, if there is one.
///
/// Covers both installed layouts: the payload in the binary's OWN directory
/// (the macOS `.pkg`, which puts CLI + payload inside `.app/Contents/MacOS`)
/// and one level up (the `.deb`, whose binaries live in `/usr/lib/mustard/bin`
/// next to `/usr/lib/mustard/templates`).
///
/// `exe` must be the CANONICAL executable path — a symlink's own directory
/// holds no payload; see [`resolve_templates_dir`]. Kept pure (nothing but
/// `is_dir` probes, no process env) so a test can drive it with a real symlink
/// instead of reasoning about the platform — see
/// `templates_resolve_through_a_symlinked_exe`.
fn templates_beside_exe(exe: &Path) -> Option<PathBuf> {
    let exe_dir = exe.parent()?;
    [exe_dir.join("templates"), exe_dir.join("../templates")]
        .into_iter()
        .find(|candidate| candidate.is_dir())
}

/// Resolve the bundled `templates/` directory.
///
/// Resolution order:
/// 1. the `MUSTARD_TEMPLATES_DIR` environment variable (explicit override —
///    used by tests and by the dashboard backend, which knows its own layout);
/// 2. `<exe-dir>/templates` and `<exe-dir>/../templates` (installed layout),
///    resolved from the CANONICALIZED executable path;
/// 3. `<CARGO_MANIFEST_DIR>/templates` (the in-repo layout, for `cargo run`).
///
/// Step 2 canonicalizes because `current_exe` promises nothing about symlinks:
/// the std docs state that some platforms return the path of the symlink and
/// others the path of its target, and on macOS the underlying
/// `_NSGetExecutablePath` is documented (dyld(3)) to return "a path", not "a
/// real path". Every installed layout ships the payload beside the REAL binary
/// and exposes symlinks on `PATH` — `/usr/local/bin` → inside the `.app`
/// (macOS), `/usr/bin` → `/usr/lib/mustard/bin` (Linux). Without
/// canonicalizing, `mustard init` invoked by name probes the LINK's directory
/// and never reaches the payload: that is precisely how macOS broke while
/// Linux, whose `/proc/self/exe` is pre-resolved by the kernel, did not.
fn resolve_templates_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("MUSTARD_TEMPLATES_DIR") {
        let path = PathBuf::from(dir);
        if path.is_dir() {
            return Ok(path);
        }
    }

    let exe = std::env::current_exe().ok();
    if let Some(exe) = exe.as_deref() {
        // Fail-open: an unresolvable path degrades to the original, never to an
        // error — resolution must not become more brittle than it was.
        let real = match std::fs::canonicalize(exe) {
            Ok(real) => real,
            Err(_) => exe.to_path_buf(),
        };
        if let Some(found) = templates_beside_exe(&real) {
            return Ok(found);
        }
        // Safety net: the pre-canonical path is still probed, so a canonical
        // form that points somewhere unhelpful can never resolve LESS than the
        // previous behaviour did.
        if real.as_path() != exe {
            if let Some(found) = templates_beside_exe(exe) {
                return Ok(found);
            }
        }
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");
    if manifest.is_dir() {
        return Ok(manifest);
    }

    // Name what was probed: the bare "set the env var" hint left the reader with
    // no way to tell an unpackaged binary from a payload that IS installed but
    // sits beside the symlink's target rather than the symlink.
    let exe_display = exe
        .as_deref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());
    anyhow::bail!(
        "could not locate the Mustard `templates/` directory.\n\
         Probed next to the running binary ({exe_display}) and at the \
         compile-time path ({}).\n\
         An installed Mustard ships the payload beside the REAL binary, not \
         beside the symlink on PATH; set MUSTARD_TEMPLATES_DIR to override.",
        manifest.display(),
    )
}

/// Decide how to treat an existing `.claude/`, prompting if no flag settles
/// it. On the interactive "backup" choice the backup is taken here, so the
/// returned action is then [`ExistingAction::Overwrite`].
fn decide_existing_action(claude_path: &Path, options: &InitOptions) -> Result<ExistingAction> {
    if options.force {
        return Ok(ExistingAction::Overwrite);
    }
    if options.yes {
        println!("  .claude/ exists - updating without overwriting user files");
        return Ok(ExistingAction::Merge);
    }
    // Non-interactive stdin (CI, tests, the dashboard backend): default to the
    // safe merge
    // rather than blocking on a prompt that can never be answered.
    if !std::io::stdin().is_terminal() {
        println!("  .claude/ exists - merging (non-interactive)");
        return Ok(ExistingAction::Merge);
    }

    let choices = ["Backup and overwrite", "Merge (keep my files)", "Cancel"];
    let choice = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(".claude/ already exists")
        .items(choices)
        .default(1)
        .interact()
        .context("reading the .claude/ conflict choice")?;

    match choice {
        0 => {
            backup_claude_dir(claude_path)?;
            Ok(ExistingAction::Overwrite)
        }
        1 => Ok(ExistingAction::Merge),
        _ => Ok(ExistingAction::Cancel),
    }
}

/// Copy `.claude/` to a timestamped `.backup.` sibling.
fn backup_claude_dir(claude_path: &Path) -> Result<()> {
    let stamp = mustard_core::time::filename_safe_now();
    let backup = claude_path.with_file_name(format!(
        "{}.backup.{stamp}",
        claude_path
            .file_name()
            .map_or_else(|| ".claude".to_string(), |n| n.to_string_lossy().into_owned())
    ));
    copy_dir(claude_path, &backup, true, &[])?;
    println!("  Backup: {}", backup.display());
    Ok(())
}

/// Copy `templates/.github/` → `<project>/.github/` when the project has a
/// GitHub remote. Never overwrites — user customisations win. Returns the
/// number of files copied (0 when there is no `.github` payload or no remote).
fn install_github_templates(templates_dir: &Path, project_path: &Path) -> Result<usize> {
    let src = templates_dir.join(".github");
    if !src.is_dir() || !has_github_remote(project_path) {
        return Ok(0);
    }
    copy_dir(&src, &project_path.join(".github"), false, &[])
}

/// Whether `origin`'s URL points at github.com.
fn has_github_remote(project_path: &Path) -> bool {
    Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(project_path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase())
        .is_some_and(|url| url.contains("github.com"))
}

/// Build and write the single project-root `mustard.json`.
///
/// Loads any existing config (so a re-run preserves user edits), folds in the
/// git-flow + locale choices and agnostically-detected commands — only when
/// interactive or on a fresh project; otherwise the existing git-flow is left
/// untouched — then stamps `runtime` + `version` and writes **once**. There is
/// no `.claude/mustard.json`: the file lives at the project root (the workspace
/// anchor), the single source of truth.
fn write_project_config(project_path: &Path, runtime: &Runtime, interactive: bool) -> Result<()> {
    let mut config = ProjectConfig::load(project_path);
    let fresh = !ProjectConfig::exists(project_path);

    if interactive || fresh {
        let facts = git_flow::probe_git(project_path);
        let choices = git_flow::collect_choices(&facts, &config, interactive)?;
        git_flow::apply_choices(&mut config, &choices, project_path);
    } else {
        println!("  mustard.json already exists - git flow preserved");
    }

    // Seed the default inject declarations only when the user has none — a
    // curated (non-empty) list is theirs and is preserved verbatim. The
    // defaults live in the core (`project_seed::default_inject_entries`).
    if config.inject.is_empty() {
        config.inject = mustard_core::default_inject_entries();
        println!("  seeded inject declarations (.claude/mustard/*.md ride the session hooks)");
    }

    config.runtime = Some(runtime.clone());
    // The stamp is the HARNESS version (plugin manifest when launched from the
    // plugin, the core line otherwise) — no longer this CLI crate's version.
    // The drift advisory + `/mustard:upsert` compare against the same source.
    config.version = Some(mustard_core::harness_version());
    config.write(project_path)?;
    println!("  wrote mustard.json");
    Ok(())
}

/// Tell the dashboard this machine has one more Mustard project.
///
/// **An environment act, so it lives on the binary side** — `cli::dispatch`
/// calls it, never the library. `~/.claude/` is outside the project, and
/// `library_is_pure` enforces that a library call never writes there; it caught
/// exactly this function placed one layer too deep. Same shape, and same
/// reason, as [`ensure_global_permissions_if_opted_in`].
///
/// `mustard.json` having just been written is the one moment a project becomes
/// a Mustard project, so it is the only honest moment to record it. Before
/// this, the machine-level registry had a single writer — the dashboard's own
/// "add folder" button — so installing Mustard told the dashboard nothing: the
/// operator installed, opened the dashboard and met an empty list with no hint
/// that anything was missing (reported in the field, 2026-08-28).
///
/// **Never fails the install.** A dashboard listing is a convenience, and an
/// unwritable home directory is not a reason to refuse a project its harness.
/// Idempotent by path, so a re-run adds no second row and says so.
///
/// This is NOT the global-settings write that [`ensure_global_permissions`]
/// guards behind `MUSTARD_GLOBAL_PERMISSIONS`: that rule protects the user's
/// own `~/.claude/settings.json`, which Mustard has no business editing
/// unprompted. This writes a file Mustard itself owns, whose whole purpose is
/// to list the projects it was installed into.
pub(crate) fn register_with_dashboard(project_path: &Path) {
    use mustard_core::dashboard_registry::{register, RegisterOutcome};
    // The registry's identity is the absolute path — a relative one would
    // register a row that resolves differently depending on where the dashboard
    // was started.
    let absolute = project_path
        .canonicalize()
        .unwrap_or_else(|_| project_path.to_path_buf());
    match register(&absolute) {
        Ok(RegisterOutcome::Added) => {
            println!("  registered with the dashboard (~/.claude/dashboard-projects.json)");
        }
        Ok(RegisterOutcome::AlreadyPresent) => {}
        Err(err) => {
            eprintln!("[mustard] warning: could not register with the dashboard: {err}");
        }
    }
}
/// The binary-side face of [`ensure_global_permissions`].
///
/// Exists so `cli::dispatch` can take this environment act without the library
/// taking it: it is the only caller, it swallows the failure the way the install
/// always did, and `pub(crate)` keeps it off the crate's public API.
pub(crate) fn ensure_global_permissions_if_opted_in() {
    ensure_global_permissions().unwrap_or_else(|err| {
        eprintln!("[mustard] warning: could not update global permissions: {err}");
    });
}

/// Ensure `~/.claude/settings.json` grants `Read`/`Write`/`Edit` and sets the
/// `CLAUDE_CODE_NO_FLICKER` env var. Non-destructive: only adds what is
/// missing, preserves everything else.
///
/// **Opt-in.** Mutating the user's *global* `~/.claude/settings.json` is off by
/// default — user policy is to never touch global settings unprompted. The
/// write only runs when `MUSTARD_GLOBAL_PERMISSIONS` is set to `1`/`true`;
/// otherwise this is a no-op and the project-local `.claude/settings.json` is
/// the only thing `init` writes.
fn ensure_global_permissions() -> Result<()> {
    if !global_permissions_opt_in() {
        println!(
            "  Global settings: skipped (set MUSTARD_GLOBAL_PERMISSIONS=1 to update ~/.claude/settings.json)"
        );
        return Ok(());
    }
    let Some(home) = home_dir() else {
        return Ok(());
    };
    let claude_dir = home.join(".claude");
    let settings_path = claude_dir.join("settings.json");

    let mut settings = crate::fs_ops::read_json_object(&settings_path);

    // permissions.allow — add the generic perm, dropping path-scoped variants.
    let permissions = settings
        .entry("permissions")
        .or_insert_with(|| json!({}));
    let allow = permissions
        .as_object_mut()
        .and_then(|p| {
            p.entry("allow")
                .or_insert_with(|| json!([]))
                .as_array_mut()
        });
    let mut added = Vec::new();
    if let Some(allow) = allow {
        for perm in ["Read", "Write", "Edit"] {
            let has_generic = allow.iter().any(|v| v.as_str() == Some(perm));
            if !has_generic {
                let scoped_prefix = format!("{perm}(");
                allow.retain(|v| {
                    !v.as_str().is_some_and(|s| s.starts_with(&scoped_prefix))
                });
                allow.push(json!(perm));
                added.push(perm);
            }
        }
    }

    // env.CLAUDE_CODE_NO_FLICKER = "1"
    let env = settings.entry("env").or_insert_with(|| json!({}));
    let mut env_added = false;
    if let Some(env) = env.as_object_mut() {
        if env.get("CLAUDE_CODE_NO_FLICKER").and_then(|v| v.as_str()) != Some("1") {
            env.insert("CLAUDE_CODE_NO_FLICKER".to_string(), json!("1"));
            env_added = true;
        }
    }

    if added.is_empty() && !env_added {
        println!("  Global settings: permissions and env already configured");
        return Ok(());
    }

    mfs::create_dir_all(&claude_dir)
        .with_context(|| format!("creating {}", claude_dir.display()))?;
    let mut serialized =
        serde_json::to_string_pretty(&serde_json::Value::Object(settings))
            .context("serializing global settings")?;
    serialized.push('\n');
    mfs::write_atomic(&settings_path, serialized.as_bytes())
        .with_context(|| format!("writing {}", settings_path.display()))?;
    if !added.is_empty() {
        println!("  Global permissions: added {} to ~/.claude/settings.json", added.join(", "));
    }
    if env_added {
        println!("  Global env: set CLAUDE_CODE_NO_FLICKER in ~/.claude/settings.json");
    }
    Ok(())
}

/// Whether the user opted in to having `init` mutate the *global*
/// `~/.claude/settings.json`. Off by default; enabled by setting
/// `MUSTARD_GLOBAL_PERMISSIONS` to `1` or `true` (case-insensitive).
fn global_permissions_opt_in() -> bool {
    std::env::var("MUSTARD_GLOBAL_PERMISSIONS")
        .is_ok_and(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true"
        })
}

/// The user's home directory, cross-platform, without a `dirs` crate
/// dependency: `HOME` on Unix, `USERPROFILE` on Windows.
fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
}

/// Ensure RTK (Rust Token Killer) is installed. Best-effort and fail-open: a
/// missing RTK — and a *failed* install — never blocks `init`.
///
/// Flow: if `rtk` is already on PATH, run `rtk init -g --no-patch` and return.
/// Otherwise attempt an auto-install (see [`install_rtk`]); on success re-run
/// the `rtk init`, on failure print the manual instructions and carry on.
pub(crate) fn ensure_rtk() {
    // No external-tool side effects under unit tests: on a clean CI runner this
    // would shell out to `cargo install --git …rtk` (slow / network-bound).
    if cfg!(test) {
        return;
    }
    if rtk_on_path() {
        println!("  RTK detected (token economy active)");
        let _ = Command::new("rtk").args(["init", "-g", "--no-patch"]).output();
        return;
    }

    println!("  RTK not found - attempting auto-install for 60-90% token savings...");
    if install_rtk(rtk_pinned_rev().as_deref()) && rtk_on_path() {
        println!("  RTK installed (token economy active)");
        let _ = Command::new("rtk").args(["init", "-g", "--no-patch"]).output();
    } else {
        println!("  RTK auto-install skipped or unavailable - install manually:");
        if cfg!(windows) {
            println!("    Windows: cargo install --git https://github.com/rtk-ai/rtk");
            println!("         or: scoop install rtk");
        } else {
            println!("    Unix: curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/master/install.sh | sh");
        }
    }
}

/// Whether `rtk --version` succeeds (RTK reachable on PATH).
fn rtk_on_path() -> bool {
    Command::new("rtk")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Probe `rtk --version` and exit hard with install instructions when it
/// fails. RTK is a mandatory dependency: the harness prefixes Bash commands
/// with `rtk`, so a Mustard install without `rtk` on `PATH` would produce a
/// `.claude/` that cannot run. We abort before touching disk rather than
/// failing later in a confusing way.
///
/// This is **not** fail-open — unlike [`ensure_rtk`], which is best-effort
/// during the install phase. The exit code is `1` so a script driving the
/// binary can detect the failure and surface it to the user. NOT library
/// callers: they never reach this function, which is the whole point of it
/// living in `cli::dispatch`. `pub(crate)` makes the compiler enforce that
/// rather than leaving it to this comment.
pub(crate) fn probe_rtk() {
    // Skip the hard gate under unit tests: a clean CI runner has no `rtk`, and a
    // `process::exit` here would kill the whole test process.
    //
    // That guard is narrower than it reads, which is why this function may only
    // be called from the BINARY's dispatch and never from the library: `cfg!(test)`
    // is true while this crate compiles its own unit tests and false everywhere
    // else — an integration test in another crate (the dashboard's
    // `mustard_cli_test`) compiles this as an ordinary dependency and gets the
    // `exit(1)`. That is exactly how it died on CI's first run of that crate.
    if cfg!(test) || rtk_on_path() {
        return;
    }
    eprintln!(
        "\nMustard requires RTK (Rust Token Killer) on PATH.\n\
         Could not run `rtk --version` — RTK is a mandatory dependency.\n\
         Install RTK and re-run `mustard init`:\n\
           - Unix:    curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/master/install.sh | sh\n\
           - Windows: scoop install rtk   (or)   cargo install --git https://github.com/rtk-ai/rtk\n"
    );
    std::process::exit(1);
}

/// Read the RTK revision pinned in the managed-artifact manifest
/// (`<templates_dir>/.artifacts.json`, record `tool:rtk`).
///
/// Fail-open: a missing / unreadable / unparseable manifest, an absent
/// `tool:rtk` record, or a null version all yield `None`, leaving the caller
/// on the current unpinned-install behavior. Never errors or panics.
///
/// A branch name (e.g. `develop`) is treated as "unpinned": only a concrete
/// rev is usable as `cargo install --rev`, so callers receive `None` for it.
fn rtk_pinned_rev() -> Option<String> {
    let manifest_path = resolve_templates_dir().ok()?.join(".artifacts.json");
    let raw = mfs::read_to_string(&manifest_path).ok()?;
    let manifest: mustard_core::domain::model::provenance::ArtifactManifest =
        serde_json::from_str(&raw).ok()?;
    let version = manifest
        .artifacts
        .into_iter()
        .find(|record| record.id == "tool:rtk")?
        .version?;
    // A 40-char hex string is a commit SHA; anything else is a branch/tag and
    // is not safe to pass to `cargo install --rev`.
    let is_sha = version.len() == 40 && version.bytes().all(|b| b.is_ascii_hexdigit());
    is_sha.then_some(version)
}

/// Best-effort RTK auto-install. Returns `true` only when an installer command
/// exited successfully. Every spawn failure is swallowed — a host without
/// `curl`/`cargo`/`scoop` simply falls through to the manual instructions.
///
/// `pinned_rev` is the RTK commit SHA from the manifest (`rtk_pinned_rev`);
/// when present it pins the `cargo install --git` to that rev, when `None` the
/// install runs unpinned.
fn install_rtk(pinned_rev: Option<&str>) -> bool {
    let run_ok = |cmd: &mut Command| -> bool {
        cmd.output().is_ok_and(|o| o.status.success())
    };

    if cfg!(windows) {
        if run_ok(Command::new("scoop").args(["install", "rtk"])) {
            return true;
        }
        let mut cargo = Command::new("cargo");
        cargo.args(["install", "--git", "https://github.com/rtk-ai/rtk"]);
        if let Some(rev) = pinned_rev {
            cargo.args(["--rev", rev]);
        }
        run_ok(&mut cargo)
    } else {
        run_ok(Command::new("sh").arg("-c").arg(
            "curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/master/install.sh | sh",
        ))
    }
}

/// Ensure ripgrep (`rg`) is installed. Best-effort and fail-open: a missing
/// `rg` — and a *failed* install — never blocks `init`.
///
/// Why: RTK's `grep`/`find` filters use `rg` as their search engine. When `rg`
/// is missing, RTK prints a fallback warning on every invocation that pollutes
/// every Bash tool output with ~50 tokens.
///
/// Flow: if `rg` is already on PATH, return silently. Otherwise attempt
/// auto-install via Scoop (Windows) or `cargo install ripgrep`; on Unix only
/// print manual instructions (the package manager varies).
pub(crate) fn ensure_ripgrep() {
    // No external-tool side effects under unit tests (would `cargo install
    // ripgrep` on a clean CI runner). Production keeps `cfg!(test) == false`.
    if cfg!(test) {
        return;
    }
    if rg_on_path() {
        return;
    }

    println!("  ripgrep not found - attempting auto-install (silences RTK `rg` fallback warning)...");
    if install_ripgrep() && rg_on_path() {
        println!("  ripgrep installed");
        return;
    }

    println!("  ripgrep auto-install skipped or unavailable - install manually:");
    if cfg!(windows) {
        println!("    Windows: scoop install ripgrep");
        println!("         or: cargo install ripgrep");
    } else if cfg!(target_os = "macos") {
        println!("    macOS:   brew install ripgrep");
    } else {
        println!("    Linux:   apt install ripgrep | pacman -S ripgrep | dnf install ripgrep");
    }
}

/// Whether `rg --version` succeeds (ripgrep reachable on PATH).
fn rg_on_path() -> bool {
    Command::new("rg")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Best-effort ripgrep auto-install. Returns `true` only when an installer
/// command exited successfully. Every spawn failure is swallowed.
///
/// - Windows: try `scoop install ripgrep` first, then `cargo install ripgrep`.
/// - Unix: return `false` so the caller prints manual instructions.
fn install_ripgrep() -> bool {
    let run_ok = |cmd: &mut Command| -> bool {
        cmd.output().is_ok_and(|o| o.status.success())
    };

    if cfg!(windows) {
        if run_ok(Command::new("scoop").args(["install", "ripgrep"])) {
            return true;
        }
        return run_ok(Command::new("cargo").args(["install", "ripgrep"]));
    }
    false
}

/// Print the closing "next steps" block.
///
/// This surface has to stand on its own, because `mustard init` is most often
/// run DIRECTLY in a project, with no installer around it and no document open.
/// It therefore prints the two commands verbatim: the placeholder this replaced
/// (`add <mustard repo or local directory>` → `install mustard`) typed as
/// written answers `Plugin "mustard" not found in any marketplace`.
///
/// The Linux installer also runs `mustard init --yes` at the end of a
/// `curl … | sh`. There this block is NOT the last thing on screen — the
/// installer prints its own closing block after it — so the two would otherwise
/// teach the same plugin step twice, in English and then in Portuguese. The
/// installer resolves that on its side: when it ran init itself it points back
/// at these lines instead of reprinting them (`packaging/installer/install.sh`).
/// NOTE: in a DIRECT run this is no longer the last thing on screen — the
/// global-settings line AND the RTK/ripgrep setup lines now follow it, because
/// all three moved to
/// `cli::dispatch` and run after `init` returns. The block is still the last
/// word of the INSTALL; what trails it is tool setup, not project state.
/// Keep these two commands here regardless; they are what the direct run needs.
fn print_next_steps() {
    println!("\nDone!\n");
    println!("Next:");
    println!("  1. Install the plugin INSIDE Claude Code — type these two lines there,");
    println!("     not in this terminal (already installed? nothing to do):");
    println!("     /plugin marketplace add rubensrpj/mustard");
    println!("     /plugin install mustard@mustard-local");
    println!("  2. Reload Claude Code, then run /scan to analyze your codebase.\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Build a minimal fake `templates/` tree and return its path. Tests point
    /// `init_with_templates` at this so they never touch the real payload. The
    /// four harness seeds (settings, injectables, `.gitignore`) come from the
    /// COMPILED-IN core constants now — this fixture only carries what the
    /// templates dir still owns for init (`.github/`, manifests) plus a
    /// `commands/` decoy: the thin init must NOT copy it into `.claude/`.
    fn fake_templates(root: &Path) -> PathBuf {
        let templates = root.join("templates");
        fs::create_dir_all(templates.join("commands")).unwrap();
        fs::write(templates.join("commands/feature.md"), "feature").unwrap();
        templates
    }

    /// Regression guard (2026-06-03): the legacy per-subproject guards file
    /// `.claude/commands/guards.md` (and its `patterns.md` companion) is
    /// OBSOLETE. No shipped template may point an agent at those non-existent
    /// paths. Walks the REAL bundled `templates/` payloads — the CLI's own
    /// tree AND the core seed tree (`packages/core/templates/`, where the
    /// harness seeds moved) — and fails if the obsolete path is reintroduced.
    #[test]
    fn templates_never_reference_obsolete_guards_file() {
        let templates = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");
        assert!(
            templates.is_dir(),
            "templates payload missing at {}",
            templates.display()
        );
        let core_templates = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/core/templates");
        assert!(
            core_templates.is_dir(),
            "core seed payload missing at {}",
            core_templates.display()
        );

        const FORBIDDEN: [&str; 2] = ["commands/guards.md", "commands/patterns.md"];
        let mut offenders: Vec<String> = Vec::new();

        // Iterative directory walk — no external crate.
        let mut stack = vec![templates.clone(), core_templates];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let Ok(bytes) = fs::read(&path) else {
                    continue;
                };
                let text = String::from_utf8_lossy(&bytes);
                for needle in FORBIDDEN {
                    if text.contains(needle) {
                        offenders.push(format!("{} → {needle}", path.display()));
                    }
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "templates must not reference the obsolete standalone guards file:\n{}",
            offenders.join("\n")
        );
    }

    /// Both installed layouts resolve, and an unpackaged binary resolves to
    /// nothing. Runs everywhere — the symlink half of the contract needs a
    /// privilege Windows may withhold, so it lives in the `#[cfg(unix)]` test
    /// below; this one keeps the candidate list itself covered on every host.
    #[test]
    fn templates_beside_exe_covers_both_installed_layouts() {
        let dir = tempdir().unwrap();

        // `.pkg` layout: payload in the binary's OWN directory.
        let pkg = dir.path().join("pkg");
        fs::create_dir_all(pkg.join("templates")).unwrap();
        assert!(templates_beside_exe(&pkg.join("mustard")).is_some());

        // `.deb` layout: payload one level up from the binary's directory.
        let deb = dir.path().join("deb");
        fs::create_dir_all(deb.join("bin")).unwrap();
        fs::create_dir_all(deb.join("templates")).unwrap();
        assert!(templates_beside_exe(&deb.join("bin/mustard")).is_some());

        // Neither: a bare binary with no payload anywhere near it.
        let bare = dir.path().join("bare/bin");
        fs::create_dir_all(&bare).unwrap();
        assert!(templates_beside_exe(&bare.join("mustard")).is_none());
    }

    /// Regression guard (2026-07-29): `mustard init` on macOS died with
    /// "could not locate the Mustard `templates/` directory". The `.pkg`
    /// installs the real binary + payload inside the `.app` and exposes
    /// `/usr/local/bin/mustard` as a SYMLINK — and `current_exe` is not
    /// required to resolve symlinks (on macOS `_NSGetExecutablePath` returns
    /// "a path", not "a real path", per dyld(3)). Probing the LINK's own
    /// directory therefore finds nothing, which is why the resolution
    /// canonicalizes first.
    ///
    /// Unix-only: creating a symlink on Windows needs a privilege the test
    /// host may not hold. The candidate list itself is covered on every host
    /// by `templates_beside_exe_covers_both_installed_layouts`.
    #[cfg(unix)]
    #[test]
    fn templates_resolve_through_a_symlinked_exe() {
        let dir = tempdir().unwrap();
        let real_bin = dir.path().join("real/bin");
        let link_dir = dir.path().join("link");
        fs::create_dir_all(real_bin.join("templates")).unwrap();
        fs::create_dir_all(&link_dir).unwrap();

        let real_exe = real_bin.join("mustard");
        fs::write(&real_exe, "").unwrap();
        let link_exe = link_dir.join("mustard");
        std::os::unix::fs::symlink(&real_exe, &link_exe).unwrap();

        // The defect, stated as an assertion: the symlink's own directory
        // holds no payload, so probing it (the pre-fix behaviour) finds
        // nothing. If this ever passes, the fixture stopped reproducing.
        assert!(
            templates_beside_exe(&link_exe).is_none(),
            "fixture broken: the symlink's directory must not hold a payload",
        );

        // The fix: canonicalize, then probe — the payload beside the TARGET.
        let canonical = fs::canonicalize(&link_exe).unwrap();
        let found = templates_beside_exe(&canonical)
            .expect("templates/ beside the symlink target must resolve");
        assert_eq!(
            fs::canonicalize(found).unwrap(),
            fs::canonicalize(real_bin.join("templates")).unwrap(),
        );
    }

    #[test]
    fn timestamp_slug_has_expected_shape() {
        let slug = mustard_core::time::filename_safe_now();
        // YYYY-MM-DDTHH-MM-SS
        assert_eq!(slug.len(), 19);
        assert_eq!(&slug[4..5], "-");
        assert_eq!(&slug[10..11], "T");
    }

    #[test]
    fn init_seeds_harness_and_enables_plugin() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        let claude = project.join(".claude");
        // The seed files are laid down — injectables replace the planted orchestrator.
        // The LOCAL layer, always: the install has no shared mode any more, so
        // the versioned twin must never appear.
        assert!(
            claude.join("settings.local.json").exists(),
            ".claude/settings.local.json seeded",
        );
        assert!(
            !claude.join("settings.json").exists(),
            "the versioned settings file is never created",
        );
        assert!(
            claude.join("mustard").join("orchestrator.md").exists(),
            ".claude/mustard/orchestrator.md seeded"
        );
        assert!(
            !claude.join("CLAUDE.md").exists(),
            "init must NOT plant .claude/CLAUDE.md — the orchestrator is injected now"
        );
        assert!(claude.join(".gitignore").exists(), ".claude/.gitignore seeded");

        // The content payload is the plugin's now — init must NOT copy it.
        assert!(
            !claude.join("commands").exists(),
            "commands/skills/agents/refs ship in the mustard plugin, never .claude/"
        );
        // The plugin ships `.mcp.json`; init writes no project-level copy.
        assert!(
            !project.join(".mcp.json").exists(),
            "init must not write .mcp.json — the plugin ships it"
        );

        // settings.json carries the reduced seed keys and NO plugin enablement —
        // that choice lives at user scope, never planted into the project.
        // Content now comes from the compiled-in core seed, so assert on a
        // stable key the real seed carries.
        let settings = crate::fs_ops::read_json_object(&claude.join("settings.local.json"));
        assert_eq!(
            settings
                .get("env")
                .and_then(|e| e.get("MUSTARD_SPEC_SIZE_MODE"))
                .and_then(|v| v.as_str()),
            Some("warn"),
            "the compiled-in seed's env is laid down verbatim"
        );
        assert!(settings.get("statusLine").is_some(), "seed statusLine present");
        assert!(
            settings
                .get("enabledPlugins")
                .and_then(|p| p.get("mustard@mustard"))
                .is_none(),
            "init must not plant enabledPlugins in the project"
        );
        assert!(
            settings
                .get("extraKnownMarketplaces")
                .and_then(|m| m.get("mustard"))
                .is_none(),
            "init must not plant a marketplace entry in the project"
        );

        // .gitignore covers the ephemeral harness state.
        assert!(
            fs::read_to_string(claude.join(".gitignore")).unwrap().contains(".events/"),
            ".gitignore covers the ephemeral .events/ dir"
        );

        // The SINGLE project-root mustard.json carries git-flow, the version
        // stamp, runtime, and the language/tone defaults — and there is NO
        // .claude/mustard.json.
        let cfg = crate::fs_ops::read_json_object(&project.join("mustard.json"));
        assert_eq!(
            cfg.get("version").and_then(|v| v.as_str()),
            Some(mustard_core::harness_version().as_str()),
            "the stamp is the harness version, not the CLI crate's"
        );
        assert!(cfg.get("runtime").is_some(), "runtime block written");
        assert!(cfg.get("git").is_some(), "git-flow block written");
        assert_eq!(cfg.get("specLang").and_then(|v| v.as_str()), Some("pt-BR"));
        assert_eq!(cfg.get("tone").and_then(|v| v.as_str()), Some("didactic"));
        // The default inject declarations are seeded: the router's three parts,
        // each on its OWN sibling hook. The cap is per hook RESPONSE, not per
        // event, so siblings share no budget — a part that outgrows the ceiling
        // is SPLIT and given another hook, never compressed until a rule drops
        // out. The response style is a plugin output-style now, not a
        // per-project injectable.
        let inject = cfg.get("inject").and_then(|v| v.as_array()).expect("inject seeded");
        assert_eq!(inject.len(), 3, "the router's three parts: {inject:?}");
        for (i, file) in [
            ".claude/mustard/orchestrator.md",
            ".claude/mustard/dispatch.md",
            ".claude/mustard/material.md",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(inject[i].get("file").and_then(|v| v.as_str()), Some(*file));
            // Every part rides `userPromptSubmit`: that event self-heals (the
            // `once` markers are per session_id, so a fork/resume re-delivers on
            // the next prompt), which `sessionStart` cannot do.
            assert_eq!(
                inject[i].get("on").and_then(|v| v.as_str()),
                Some("userPromptSubmit"),
                "{file} must ride the self-healing event",
            );
            assert_eq!(inject[i].get("once").and_then(|v| v.as_bool()), Some(true));
        }
        assert!(
            !claude.join("mustard.json").exists(),
            "no .claude/mustard.json — config lives only at the project root"
        );

        // init seeds no entity-registry — the repo model is grain's
        // `.claude/grain.model.json`, produced on demand by `mustard-rt run scan`.
        assert!(!claude.join("entity-registry.json").exists());
    }

    /// Run git in `dir`, failing the test loudly — a half-built repository
    /// would make the assertion below prove nothing.
    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
        assert!(out.status.success(), "git {args:?} failed: {out:?}");
    }

    /// `git status --porcelain` for `dir`, trimmed.
    fn porcelain(dir: &Path) -> String {
        let out = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(dir)
            .output()
            .expect("git status");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// The stamp commit lands on a PROTECTED branch too, and that is deliberate
    /// — locked here so nobody changes it by accident.
    ///
    /// `work_branch_gate` denies the OPERATOR an edit that would land on the
    /// default branch, so an installer committing there looks like the same
    /// rule broken. It is a different case: the stamp is project configuration
    /// the install itself wrote, not work, and a fresh clone is checked out on
    /// the default branch — refusing there would hand the dirty tree back in
    /// the commonest install of all, which is the defect this whole path
    /// exists to remove. What the design owes instead is saying so: the
    /// recorded line names the branch ([`report_stamp`]).
    #[test]
    fn install_commits_the_stamp_on_a_protected_branch() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();

        git(&project, &["init", "--initial-branch=main"]);
        git(&project, &["config", "user.email", "t@example.com"]);
        git(&project, &["config", "user.name", "t"]);
        fs::write(project.join("mustard.json"), "{\n  \"version\": \"0.0.0-old\"\n}\n").unwrap();
        git(&project, &["add", "mustard.json"]);
        git(&project, &["commit", "-m", "config"]);
        assert_eq!(porcelain(&project), "", "fixture must start clean");
        assert_eq!(
            mustard_core::current_branch(&project).as_deref(),
            Some("main"),
            "the fixture must really sit on the protected default branch",
        );

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        assert_eq!(
            porcelain(&project),
            "",
            "the stamp is committed on the default branch like any other — refusing there \
             would leave the commonest install dirty",
        );
    }

    /// AC-4 — a repository that TRACKS `mustard.json` gets its version stamp
    /// re-written on every install, and until now the installer left that
    /// change sitting uncommitted. The next command that guards on a clean tree
    /// then refused, naming the operator's own work as the cause — a false
    /// attribution, since the writer was this installer. An install that found
    /// the tree clean leaves it clean.
    #[test]
    fn install_leaves_the_git_tree_clean() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();

        git(&project, &["init"]);
        git(&project, &["config", "user.email", "t@example.com"]);
        git(&project, &["config", "user.name", "t"]);
        // The repository VERSIONS its config, stamped by an older harness — the
        // shape the defect needs (a private install's exclude rule cannot hide
        // a path git already tracks).
        fs::write(project.join("mustard.json"), "{\n  \"version\": \"0.0.0-old\"\n}\n").unwrap();
        git(&project, &["add", "mustard.json"]);
        git(&project, &["commit", "-m", "config"]);
        assert_eq!(porcelain(&project), "", "fixture must start clean");

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        // The stamp really moved — otherwise there was nothing to leave dirty
        // and this test would pass for the wrong reason.
        let cfg = crate::fs_ops::read_json_object(&project.join("mustard.json"));
        assert_eq!(
            cfg.get("version").and_then(|v| v.as_str()),
            Some(mustard_core::harness_version().as_str()),
        );
        assert_eq!(
            porcelain(&project),
            "",
            "the install found a clean tree and must leave one — no manual step in between",
        );
    }

    /// The other half of the same rule: a tree that already carries the
    /// operator's work is left entirely alone. Sweeping their change into an
    /// installer's commit would be a far worse defect than the dirty stamp.
    #[test]
    fn install_never_commits_over_the_operators_own_work() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();

        git(&project, &["init"]);
        git(&project, &["config", "user.email", "t@example.com"]);
        git(&project, &["config", "user.name", "t"]);
        fs::write(project.join("mustard.json"), "{\n  \"version\": \"0.0.0-old\"\n}\n").unwrap();
        fs::write(project.join("notes.md"), "draft\n").unwrap();
        git(&project, &["add", "."]);
        git(&project, &["commit", "-m", "seed"]);
        // The operator's own uncommitted edit, present BEFORE the install runs.
        fs::write(project.join("notes.md"), "draft, still being written\n").unwrap();

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        let status = porcelain(&project);
        assert!(status.contains("notes.md"), "the operator's edit survives: {status}");
        assert!(
            status.contains("mustard.json"),
            "and the stamp is left with it, uncommitted: {status}"
        );
    }

    #[test]
    fn init_dry_run_writes_nothing() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let dry = work.path().join("dry");
        fs::create_dir_all(&dry).unwrap();

        init_with_templates(
            &dry,
            &templates,
            &InitOptions { yes: true, dry_run: true, ..InitOptions::default() },
        )
        .unwrap();

        assert!(!dry.join(".claude").exists(), "dry-run wrote nothing");
    }

    /// Regression guard for the `.claude/.claude/` nesting bug (I1 rule): even
    /// if `templates/` carries a stray `.claude/` sub-directory, the thin init —
    /// whose harness seeds are compiled-in constants, not directory copies —
    /// must never propagate it.
    #[test]
    fn init_does_not_create_nested_claude_dir() {
        let work = tempdir().unwrap();

        let templates = work.path().join("templates");
        fs::create_dir_all(templates.join("commands")).unwrap();
        // Inject the offending .claude/ inside templates/.
        fs::create_dir_all(templates.join(".claude/commands")).unwrap();
        fs::write(templates.join(".claude/commands/notes.md"), "boilerplate").unwrap();

        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        let nested = project.join(".claude").join(".claude");
        assert!(!nested.exists(), ".claude/.claude/ must not be created — I1 rule");
    }

    #[test]
    fn init_merge_rewrites_the_injectable_and_backfills() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        let claude = project.join(".claude");
        // A diverged injectable already present in .claude/mustard/.
        fs::create_dir_all(claude.join("mustard")).unwrap();
        fs::write(claude.join("mustard/orchestrator.md"), "USER EDIT").unwrap();

        // Non-interactive existing-dir path resolves to a merge.
        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        // The injectable is the harness's own rules, so merge mode does not
        // reach it: the seed is laid down again whatever was there…
        assert_eq!(
            fs::read_to_string(claude.join("mustard/orchestrator.md")).unwrap(),
            mustard_core::ORCHESTRATOR_MD,
            "merge must still rewrite the injectable — it is not project configuration"
        );
        // …while a seed the user does not have is backfilled…
        assert!(
            claude.join(".gitignore").exists(),
            "merge backfills a missing seed"
        );
        // …and no plugin enablement is planted on the merge path either.
        let settings = crate::fs_ops::read_json_object(&claude.join("settings.local.json"));
        assert!(
            settings
                .get("enabledPlugins")
                .and_then(|p| p.get("mustard@mustard"))
                .is_none(),
            "merge must not plant plugin enablement"
        );
    }

    #[test]
    fn init_migrates_planted_orchestrator_and_root_lines() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        let claude = project.join(".claude");
        fs::create_dir_all(&claude).unwrap();
        // The legacy layout: a Mustard-planted orchestrator (carries the
        // marker) + a root CLAUDE.md carrying the import and breadcrumb lines.
        fs::write(
            claude.join("CLAUDE.md"),
            "# Orchestrator Rules\n\nYou are the router.\n",
        )
        .unwrap();
        fs::write(
            project.join("CLAUDE.md"),
            "@.claude/scan-map.md\n\n# (root)\n\n> Orchestrator: [.claude/CLAUDE.md](.claude/CLAUDE.md)\n\n## Guards\n\n- keep this guard\n",
        )
        .unwrap();

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        // (a) the planted orchestrator is gone (and not re-planted).
        assert!(
            !claude.join("CLAUDE.md").exists(),
            "the Mustard-planted .claude/CLAUDE.md must be migrated away"
        );
        // (b) the root file lost ONLY the Mustard lines; the rest survives.
        let root_md = fs::read_to_string(project.join("CLAUDE.md")).unwrap();
        assert!(!root_md.contains("@.claude/scan-map.md"), "import line removed: {root_md}");
        assert!(!root_md.contains("> Orchestrator:"), "breadcrumb removed: {root_md}");
        assert!(root_md.contains("# (root)"), "user heading survives: {root_md}");
        assert!(root_md.contains("## Guards"), "Guards section survives: {root_md}");
        assert!(root_md.contains("- keep this guard"), "guard line survives: {root_md}");

        // A `.claude/CLAUDE.md` WITHOUT the marker is the user's — it survives
        // a re-run untouched.
        fs::write(claude.join("CLAUDE.md"), "MY OWN NOTES\n").unwrap();
        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(claude.join("CLAUDE.md")).unwrap(),
            "MY OWN NOTES\n",
            "a user-authored .claude/CLAUDE.md (no marker) must never be deleted"
        );
    }

    #[test]
    fn init_preserves_user_inject_entries() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(&project).unwrap();
        // The user already curated their own inject list.
        fs::write(
            project.join("mustard.json"),
            r#"{"inject":[{"on":"sessionStart","file":"docs/my-rules.md","once":false}]}"#,
        )
        .unwrap();

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        let cfg = crate::fs_ops::read_json_object(&project.join("mustard.json"));
        let inject = cfg.get("inject").and_then(|v| v.as_array()).expect("inject present");
        assert_eq!(inject.len(), 1, "the curated list is preserved, not replaced: {inject:?}");
        assert_eq!(
            inject[0].get("file").and_then(|v| v.as_str()),
            Some("docs/my-rules.md"),
            "user entry survives verbatim"
        );
    }

    #[test]
    fn init_refuses_inside_git_repo_when_not_at_its_root() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        // `work` is a git repository root; the init target is a subdirectory.
        fs::create_dir_all(work.path().join(".git")).unwrap();
        let project = work.path().join("apps").join("dashboard");
        fs::create_dir_all(&project).unwrap();

        let err = init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap_err();

        let msg = format!("{err:#}");
        assert!(
            msg.contains("not the repository's root"),
            "refusal must be didactic, got: {msg}"
        );
        assert!(
            msg.contains("repository root:"),
            "refusal must name the repository root, got: {msg}"
        );
        // Refusal happens before any disk write.
        assert!(!project.join(".claude").exists(), "refusal wrote .claude/");
        assert!(!project.join("mustard.json").exists(), "refusal wrote mustard.json");
    }

    #[test]
    fn init_allows_at_git_repo_root() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("project");
        fs::create_dir_all(project.join(".git")).unwrap(); // project IS a repo root

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        assert!(project.join(".claude").join("settings.local.json").exists());
        assert!(project.join("mustard.json").exists());
    }

    #[test]
    fn init_allows_at_submodule_root_with_git_file() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        // Outer repository root…
        fs::create_dir_all(work.path().join(".git")).unwrap();
        // …and a submodule below it: `.git` is a FILE with a `gitdir:` pointer.
        let sub = work.path().join("backend").join("service");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join(".git"), "gitdir: ../../.git/modules/service\n").unwrap();

        init_with_templates(
            &sub,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        assert!(
            sub.join(".claude").join("settings.local.json").exists(),
            "a submodule root (.git file) is a legitimate init target"
        );
    }

    #[test]
    fn init_allows_in_git_less_tree() {
        let work = tempdir().unwrap();
        let templates = fake_templates(work.path());
        let project = work.path().join("plain");
        fs::create_dir_all(&project).unwrap(); // no .git anywhere up the tempdir

        init_with_templates(
            &project,
            &templates,
            &InitOptions { yes: true, ..InitOptions::default() },
        )
        .unwrap();

        assert!(project.join(".claude").join("settings.local.json").exists());
    }

    // The `retire_planted_plugin_enablement` unit tests moved to the core with
    // the function (`packages/core/src/platform/project_seed.rs`) — the CLI
    // only relays through `mustard_core::seed_settings`, covered above.
}
