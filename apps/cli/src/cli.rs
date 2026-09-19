//! Argument parsing and subcommand dispatch.
//!
//! The `mustard` binary exposes a small set of subcommands: `init` (the thin
//! 2.0 bootstrap), `config` (git-flow) and the opt-in `install-nerd-font`
//! helper. `clap`'s derive API builds the parser from the types below.
//!
//! Retired: `update` (versioned refreshes come from the plugin marketplace; a
//! re-run of `init` re-stamps `mustard.json#version`), `review` (the
//! `/mustard:pr review` step drives the native code-review skill), `add`, o
//! molde de terceiros que nenhum item da spec pede, e o sugeridor de
//! gramáticas do tree-sitter, que não baixava nem compilava gramática
//! nenhuma — só imprimia um repositório e uma linha de shell para a pessoa
//! rodar à mão.

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::commands::config::{self, ConfigOptions};
use crate::commands::init::{self, InitOptions};
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
        Commands::Init { force, yes, dry_run } => {
            // The RTK gate lives HERE, in the binary, not inside `init`. It ends
            // in `process::exit(1)`, and a library function that returns
            // `Result` must never take that decision away from its caller — the
            // dashboard's integration test proved the cost by vanishing mid-run
            // on a CI machine with no `rtk`. A terminal user still meets the gate
            // before any disk write, which is all it ever promised.
            //
            // Dry-run writes nothing, so a missing `rtk` cannot leave a broken
            // `.claude/` behind and the gate does not apply.
            if !dry_run {
                init::probe_rtk();
            }
            let outcome = init::init(&cwd, &InitOptions { force, yes, dry_run })?;
            // Environment acts live HERE, never in the library: putting software
            // on the operator's machine is something a library call must never
            // take on its caller's behalf. Nothing here writes under `~/.claude/`.
            //
            // The condition is `Installed`, not "no error". `Ok` used to cover
            // the operator answering Cancel to an existing `.claude/` — and on
            // that path this arm still ran a machine-wide act after an explicit
            // refusal. Measured through a pty in review. `InitOutcome` exists so
            // the caller can tell the two apart.
            if outcome == init::InitOutcome::Installed {
                init::ensure_ripgrep();
                let model_path = mustard_core::io::project_map::model_path(&cwd);
                let path_env = std::env::var("PATH").unwrap_or_default();
                init::ensure_code_tools(&cwd, &model_path, &path_env);
            }
            Ok(())
        }
        Commands::Config { yes } => config::config(&cwd, &ConfigOptions { yes }),
        Commands::InstallNerdFont { font, force, dry_run } => {
            install_nerd_font::install_nerd_font(
                &cwd,
                &InstallNerdFontOptions { font, force, dry_run },
            )
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

    /// O sugeridor de gramáticas do tree-sitter não é mais um comando.
    ///
    /// Ele não baixava nem compilava gramática nenhuma: imprimia um endereço
    /// de repositório e uma linha de shell para a pessoa rodar à mão, não
    /// tinha um único chamador e não está na lista de comandos aprovada.
    /// Registrá-lo de volta é o defeito que este teste pega — um nome que a
    /// lista não tem volta a responder, e ninguém percebe porque a compilação
    /// continua passando.
    #[test]
    fn o_sugeridor_de_gramaticas_nao_e_mais_um_comando() {
        for argv in [
            vec!["mustard", "install-grammars"],
            vec!["mustard", "install-grammars", "--project-root", "."],
        ] {
            assert!(
                Cli::try_parse_from(&argv).is_err(),
                "{argv:?} tem de ser recusado — o sugeridor de gramáticas saiu da superfície",
            );
        }
    }

    /// The install has NO mode switch. A private install is the only install
    /// there is, so `init` takes no flag for it and none against it — nothing
    /// to pass, nothing to remember, and no argv that can produce a visible
    /// footprint by accident.
    #[test]
    fn init_offers_no_switch_for_the_install_mode() {
        for argv in [
            ["mustard", "init", "--private"],
            ["mustard", "init", "--shared"],
        ] {
            assert!(
                Cli::try_parse_from(argv).is_err(),
                "{argv:?} must be rejected — the install mode is not a choice",
            );
        }
    }

}