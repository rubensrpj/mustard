//! Argument parsing and subcommand dispatch.
//!
//! Mirrors the JavaScript `cli.ts`, which used Commander: the same five
//! subcommands (`init`, `update`, `config`, `add`, `review`)
//! with the same flags. `clap`'s derive API builds the parser from the types
//! below.
//!
//! Every subcommand has a real body — Wave 1 ported `init`, Wave 2 ported the
//! rest. Each dispatch arm forwards to a module under [`crate::commands`].

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::commands::add::{self, AddOptions};
use crate::commands::config::{self, ConfigOptions};
use crate::commands::init::{self, InitOptions};
use crate::commands::install_nerd_font::{self, InstallNerdFontOptions};
use crate::commands::review::{self, ReviewOptions};
use crate::commands::update::{self, UpdateOptions};

/// Framework-agnostic CLI for Claude Code project setup.
#[derive(Debug, Parser)]
#[command(name = "mustard", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// The subcommands `mustard` accepts.
#[derive(Debug, Subcommand)]
enum Commands {
    /// Copy the `.claude/` structure into the current project.
    Init {
        /// Overwrite an existing `.claude/` directory without a backup.
        #[arg(short, long)]
        force: bool,
        /// Skip confirmation prompts (accept sensible defaults).
        #[arg(short = 'y', long)]
        yes: bool,
        /// Install the experimental Cursor IDE adapter.
        #[arg(long)]
        cursor: bool,
        /// Print intended actions without writing to disk.
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Update Mustard core files, preserving user customisations.
    Update {
        /// Skip the confirmation prompt (never skips the backup).
        #[arg(short, long)]
        force: bool,
    },
    /// Configure or reconfigure `mustard.json` (git flow).
    Config {
        /// Accept defaults without prompting.
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Install a community template.
    Add {
        /// Template identifier (e.g. `template:dotnet-clean-arch`).
        template: String,
        /// Overwrite existing files.
        #[arg(short, long)]
        force: bool,
    },
    /// Review a pull request (local or CI mode).
    Review {
        /// CI mode: post the review as a PR comment, exit non-zero on critical issues.
        #[arg(long)]
        ci: bool,
        /// PR number to review.
        #[arg(long)]
        pr: Option<u64>,
    },
    /// Install a Nerd Font on the host (required for powerline statusline themes).
    #[command(name = "install-nerd-font")]
    InstallNerdFont {
        /// Font family. Default: JetBrainsMono.
        /// One of: JetBrainsMono, CaskaydiaCove, FiraCode, Hack.
        #[arg(long)]
        font: Option<String>,
        /// Reinstall even if the font is already detected.
        #[arg(short, long)]
        force: bool,
        /// Print intended actions without invoking any package manager.
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
}

/// Parse process arguments and dispatch to the matching subcommand.
///
/// Returns `Err` if a subcommand fails; the binary maps that to a non-zero
/// exit code. `clap` itself handles `--help`/`--version` by printing and
/// exiting before this returns.
pub fn run() -> Result<()> {
    dispatch(Cli::parse())
}

/// Dispatch a parsed [`Cli`] — split out so tests can drive it without a real
/// process `argv`.
fn dispatch(cli: Cli) -> Result<()> {
    let cwd = std::env::current_dir()?;
    match cli.command {
        Commands::Init {
            force,
            yes,
            cursor,
            dry_run,
        } => init::init(
            &cwd,
            &InitOptions {
                force,
                yes,
                cursor,
                dry_run,
            },
        ),
        Commands::Update { force } => update::update(&cwd, &UpdateOptions { force }),
        Commands::Config { yes } => config::config(&cwd, &ConfigOptions { yes }),
        Commands::Add { template, force } => {
            add::add(&cwd, &template, &AddOptions { force })
        }
        Commands::Review { ci, pr } => review::review(&cwd, &ReviewOptions { ci, pr }),
        Commands::InstallNerdFont {
            font,
            force,
            dry_run,
        } => install_nerd_font::install_nerd_font(
            &cwd,
            &InstallNerdFontOptions {
                font,
                force,
                dry_run,
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_init_with_flags() {
        let cli = Cli::try_parse_from(["mustard", "init", "--yes", "--dry-run"]).unwrap();
        match cli.command {
            Commands::Init { yes, dry_run, .. } => {
                assert!(yes);
                assert!(dry_run);
            }
            other => panic!("expected Init, got {other:?}"),
        }
    }

    #[test]
    fn parses_add_positional() {
        let cli = Cli::try_parse_from(["mustard", "add", "template:foo"]).unwrap();
        match cli.command {
            Commands::Add { template, .. } => assert_eq!(template, "template:foo"),
            other => panic!("expected Add, got {other:?}"),
        }
    }

    #[test]
    fn parses_update_force() {
        let cli = Cli::try_parse_from(["mustard", "update", "--force"]).unwrap();
        match cli.command {
            Commands::Update { force } => assert!(force),
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn parses_review_pr_number() {
        let cli = Cli::try_parse_from(["mustard", "review", "--pr", "42", "--ci"]).unwrap();
        match cli.command {
            Commands::Review { pr, ci } => {
                assert_eq!(pr, Some(42));
                assert!(ci);
            }
            other => panic!("expected Review, got {other:?}"),
        }
    }
}
