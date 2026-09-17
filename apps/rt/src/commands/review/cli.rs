//! The `run` subcommands for the REVIEW and QA gates (`review/`).
//!
//! A new command takes its variant in [`ReviewCmd`] and its arm in
//! [`dispatch`] below (the compiler demands the arm), its line in
//! `tests/fixtures/run-surface.txt`, which `tests/run_command_surface.rs`
//! compares with the clap tree, and a caller in the product text, which
//! `tests/template_parity.rs` demands with no exception list.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run review <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{review};

/// The `run` subcommands owned by the REVIEW and QA gates (`review/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum ReviewCmd {
    /// The `/mustard:pr` door's REVIEW step: resolve a pull request to its work
    /// unit and print the review brief — the spec the unit belongs to, the
    /// subproject its `## Files` name, and that subproject's skill shelf (the
    /// same molds the implementer was dispatched with). With `--verdict` it
    /// refuses and records nothing: the verdict of each wave is recorded by
    /// the round.
    #[command(name = "pr-review")]
    #[command(display_order = 7)]
    PrReview {
        /// PR number. Omitted: the open pull requests are LISTED, so the
        /// reviewer picks the colleague's one instead of being handed their
        /// own branch's.
        #[arg(long)]
        pr: Option<u64>,
        /// O comando recusa e não grava nada com ele: o veredito de cada onda
        /// é gravado pela rodada, da linha do agente de revisão. Lista
        /// fechada: `approved` ou `rejected`; sem ele, o pedido é impresso.
        #[arg(long, value_parser = ["approved", "rejected"])]
        verdict: Option<String>,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// The `/mustard:pr` door's MERGE step: merge the pull request, then prune
    /// the unit (back to the base, pull it, remove the worktree, delete the
    /// local + remote branch). A unit whose review did not
    /// come back `approved` is WARNED about and ASKED — the command answers
    /// `action:"confirm"` and touches nothing; it never refuses. `--confirm` is
    /// the operator's answer coming back.
    #[command(name = "pr-merge")]
    #[command(display_order = 6)]
    PrMerge {
        /// PR number. Omitted: the open pull requests are LISTED, so the
        /// reviewer picks the colleague's one instead of being handed their
        /// own branch's.
        #[arg(long)]
        pr: Option<u64>,
        /// The operator's answer to the unreviewed-merge question. Without it
        /// an unreviewed unit is asked about, never merged and never refused.
        #[arg(long)]
        confirm: bool,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// The `/mustard:pr` door's OPEN step: open the unit's pull request through
    /// the provider IN FORCE (`git.provider` declared, else the `origin`
    /// remote, else the fallback) — the prose names this command, never a
    /// provider CLI. The title is the body file's first heading. Answers one
    /// JSON report (`ok`/`provider`/`number`/`url`); failure degrades into the
    /// `error` field with exit 0, never a panic.
    #[command(name = "pr-open")]
    #[command(display_order = 5)]
    PrOpen {
        /// The integration base the PR targets (short branch name).
        #[arg(long)]
        base: String,
        /// The work branch the PR is opened FROM (short branch name).
        #[arg(long)]
        head: String,
        /// A spec whose event file the title and the body are BUILT from.
        /// Nobody writes them: the goal becomes the title, the recorded
        /// summary and what each wave delivered become the body.
        #[arg(long)]
        spec: Option<String>,
        /// Derive title/body from the commits `base..head` carries (title =
        /// newest subject, body = the subject list) — for a repository with
        /// no spec of its own.
        #[arg(long)]
        fill: bool,
        /// Open as a draft. With a submodule the spec changed, the main pull
        /// request already opens as a draft on its own.
        #[arg(long)]
        draft: bool,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

/// Dispatch one `review`-family `run` subcommand.
pub fn dispatch(cmd: ReviewCmd) {
    match cmd {
        ReviewCmd::PrReview { pr, verdict, root } => {
            review::pr_door::run_review(&root, pr, verdict.as_deref());
        }
        ReviewCmd::PrMerge { pr, confirm, root } => {
            review::pr_door::run_merge(&root, pr, confirm);
        }
        ReviewCmd::PrOpen { base, head, spec, fill, draft, root } => {
            review::pr_publish::run_open(&root, &base, &head, spec.as_deref(), fill, draft);
        }
    }
}
