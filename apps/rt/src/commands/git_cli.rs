//! The `run` subcommands for the git work-unit ritual (`git_settle`).
//!
//! TWO registrations per command, both in this file: the variant in
//! [`GitCmd`] AND its arm in [`dispatch`] below. Forgetting the second
//! still compiles, but the command vanishes from the CLI.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run git <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{git_delete, git_settle, work_unit_open};

/// The `run` subcommands owned by the git work-unit ritual (`git_settle`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum GitCmd {
    /// The EXIT RITUAL of a delivered work unit (the engine of `/git pr close`):
    /// runs from the WORK BRANCH (bare invocation on `dev`/`main` REFUSES),
    /// verifies the unit is 100% merged on its base (ancestry + `gh` fallback
    /// for squash merges — not merged: hard stop, nothing touched), advances
    /// EVERY local base (ff-only merge on the checked-out one, ff-safe
    /// `fetch base:base` on the rest), then prunes the unit's worktree +
    /// local branch (remote delete fail-open). Inside the unit's own worktree
    /// it verifies + updates and answers `exit-and-rerun` — leave, then
    /// finish with `--unit <branch>` from the main checkout. `--report` is the
    /// same ritual's READING face: it classifies every work branch of every
    /// repo (local and remote alike) and prints, settling nothing.
    #[command(name = "git-settle")]
    #[command(display_order = 1)]
    GitSettle {
        /// The work branch to settle. Omitted: read from the invocation
        /// directory's HEAD (which must NOT be an integration base).
        #[arg(long)]
        unit: Option<String>,
        /// Report only: one entry per repository listing every work branch
        /// (local AND remote) with the single state it is in — abandoned draft,
        /// pushed without PR, in review, awaiting prune, danger, remote-only.
        /// Nothing is settled, pruned or pushed.
        #[arg(long)]
        report: bool,
        /// Any directory inside the repo (worktrees welcome — the command
        /// resolves the main checkout itself). Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// The ENTRY RITUAL of a work unit: idempotently create its isolated
    /// worktree at `.claude/worktrees/{kind}/{slug}`, cut from a fresh
    /// `origin/{base}` (offline degrades to the local base ref), so the
    /// orchestrator can switch the session into it via
    /// `EnterWorktree path=<returned path>`. An explicit `--base` MUST name a
    /// branch this repository really has; the branch name matches what
    /// `emit-pipeline` stored in the `pending-work-branch` marker. Cleanup is
    /// the `/git pr close` ritual's job, never this command's.
    #[command(name = "work-unit-open")]
    #[command(display_order = 76)]
    WorkUnitOpen {
        /// Full work-branch name (e.g. `feature/my-spec`); its prefix names a
        /// work kind, or — for a unit still in the older `{base}_{slug}` shape
        /// — a branch this repository has. Alternative to --spec/--intent.
        #[arg(long)]
        branch: Option<String>,
        /// Spec slug — used verbatim as the branch slug.
        #[arg(long)]
        spec: Option<String>,
        /// Free-form intent, slugified when --spec is absent.
        #[arg(long)]
        intent: Option<String>,
        /// What the unit IS: `feature`, `fix`, `hotfix`, or any token that can
        /// be a git ref segment. Names the branch (`{kind}/{slug}`). It does
        /// NOT decide the base. Omitted → `feature`.
        #[arg(long = "type")]
        work_kind: Option<String>,
        /// Base to cut from; must name a branch this repository really has.
        /// Omitted → the base recorded with the unit, else the project's
        /// primary one.
        #[arg(long)]
        base: Option<String>,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// The CANCEL path of an ABANDONED work unit — the counterpart of
    /// `git-settle`, which retires a unit that was DELIVERED. Never runs from
    /// INSIDE a unit (it refuses and names the base to switch to, touching
    /// nothing) and removes the named unit whole: its open pull request is
    /// closed, its worktree removed (`--force` — an abandoned unit is abandoned
    /// precisely because it still holds uncommitted work), its local branch
    /// deleted and its remote branch deleted best-effort. A name that is
    /// nobody's work unit, that is protected, or that no local or remote ref
    /// carries, is REFUSED rather than reported as deleted.
    #[command(name = "git-delete")]
    #[command(display_order = 89)]
    GitDelete {
        /// The work branch to delete, e.g. `dev_my-unit`.
        #[arg(long)]
        unit: String,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

/// Dispatch one `git`-family `run` subcommand.
pub fn dispatch(cmd: GitCmd) {
    match cmd {
        GitCmd::GitSettle { unit, report, root } => {
            git_settle::run(&root, unit.as_deref(), report);
        }
        GitCmd::WorkUnitOpen { branch, spec, intent, work_kind, base, root } => {
            work_unit_open::run(work_unit_open::WorkUnitOpenOpts {
                root,
                branch,
                spec,
                intent,
                work_kind,
                base,
            });
        }
        GitCmd::GitDelete { unit, root } => git_delete::run(&root, &unit),
    }
}
