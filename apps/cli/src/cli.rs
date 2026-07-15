//! Argument parsing and subcommand dispatch.
//!
//! The `mustard` binary exposes a small set of subcommands: `init` (the thin
//! 2.0 bootstrap), `config` (git-flow), `add` (third-party community template),
//! and the opt-in `install-nerd-font` / `install-grammars` helpers. `clap`'s
//! derive API builds the parser from the types below.
//!
//! Retired: `update` (versioned refreshes come from the plugin marketplace; a
//! re-run of `init` re-stamps `mustard.json#version`) and `review` (the
//! /mustard:review SKILL drives the native code-review skill).

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::commands::add::{self, AddOptions};
use crate::commands::config::{self, ConfigOptions};
use crate::commands::init::{self, InitOptions};
use crate::commands::install_grammars::{self, InstallGrammarsArgs};
use crate::commands::install_nerd_font::{self, InstallNerdFontOptions};

/// Framework-agnostic CLI for Claude Code project setup.
#[derive(Debug, Parser)]
#[command(name = "mustard", version = env!("MUSTARD_VERSION_FULL"), about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// The subcommands `mustard` accepts.
#[derive(Debug, Subcommand)]
enum Commands {
    /// Seed the thin `.claude/` bootstrap and enable the `mustard` plugin.
    Init {
        /// Overwrite an existing `.claude/` directory without a backup.
        #[arg(short, long)]
        force: bool,
        /// Skip confirmation prompts (accept sensible defaults).
        #[arg(short = 'y', long)]
        yes: bool,
        /// Print intended actions without writing to disk.
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Configure or reconfigure `mustard.json` (git flow).
    Config {
        /// Accept defaults without prompting.
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Install a third-party community template into `.claude/`.
    Add {
        /// Template identifier (e.g. `template:dotnet-clean-arch`).
        template: String,
        /// Overwrite existing files.
        #[arg(short, long)]
        force: bool,
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
    /// Suggest tree-sitter grammar repos for languages detected in the project.
    ///
    /// Mustard never downloads or compiles grammars — it only prints the
    /// canonical repository and a shell-ready install command for each
    /// detected language. See `mustard_core::domain::ast::GrammarLoader` for how the
    /// regression gate consumes the grammars once they are installed.
    #[command(name = "install-grammars")]
    InstallGrammars {
        /// Project root to scan. Defaults to the current working directory.
        #[arg(long = "project-root")]
        project_root: Option<std::path::PathBuf>,
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
        Commands::Init { force, yes, dry_run } => {
            init::init(&cwd, &InitOptions { force, yes, dry_run })
        }
        Commands::Config { yes } => config::config(&cwd, &ConfigOptions { yes }),
        Commands::Add { template, force } => {
            add::add(&cwd, &template, &AddOptions { force })
        }
        Commands::InstallNerdFont { font, force, dry_run } => {
            install_nerd_font::install_nerd_font(
                &cwd,
                &InstallNerdFontOptions { font, force, dry_run },
            )
        }
        Commands::InstallGrammars { project_root } => {
            install_grammars::run(InstallGrammarsArgs { project_root })
        }
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
}