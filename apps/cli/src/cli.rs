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

/// Framework-agnostic command line for Claude Code project setup.
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

/// A regra das palavras em maiúsculas nas frases do programa, a mesma do
/// catálogo e dos outros dois programas.
#[cfg(test)]
#[path = "../../../packages/core/tests/support/shouting_words.rs"]
mod shouting_words;

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
    fn the_grammar_suggester_is_no_longer_a_command() {
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

    /// As palavras em maiúsculas fora de crase em `texts`, cada uma uma vez,
    /// no defeito que diz onde ela está.
    fn push_defects(place: &str, texts: &[String], out: &mut Vec<String>) {
        let mut seen: Vec<&str> = Vec::new();
        for text in texts {
            for word in shouting_words::uppercase_words(text) {
                if !seen.contains(&word) {
                    seen.push(word);
                    out.push(format!("{place}: uppercase word {word} outside backticks"));
                }
            }
        }
    }

    /// Cada palavra em maiúsculas fora de crase na ajuda de `cmd` e dos
    /// comandos abaixo dele: o texto do comando, o de cada argumento e o de
    /// cada valor que o argumento aceita. O defeito diz o comando, o
    /// argumento, quando há, e a palavra.
    fn help_uppercase_defects(path: &str, cmd: &clap::Command, out: &mut Vec<String>) {
        let own: Vec<String> = [
            cmd.get_about(),
            cmd.get_long_about(),
            cmd.get_before_help(),
            cmd.get_before_long_help(),
            cmd.get_after_help(),
            cmd.get_after_long_help(),
        ]
        .into_iter()
        .flatten()
        .map(ToString::to_string)
        .collect();
        push_defects(path, &own, out);
        for arg in cmd.get_arguments() {
            let name = arg.get_long().map_or_else(|| arg.get_id().to_string(), |long| format!("--{long}"));
            let mut texts: Vec<String> =
                [arg.get_help(), arg.get_long_help()].into_iter().flatten().map(ToString::to_string).collect();
            texts.extend(arg.get_possible_values().iter().filter_map(|value| value.get_help().map(ToString::to_string)));
            push_defects(&format!("{path} {name}"), &texts, out);
        }
        for sub in cmd.get_subcommands() {
            help_uppercase_defects(&format!("{path} {}", sub.get_name()), sub, out);
        }
    }

    /// Os defeitos da árvore inteira, a partir do nome do programa.
    fn tree_defects(tree: &clap::Command) -> Vec<String> {
        let mut out = Vec::new();
        help_uppercase_defects(tree.get_name(), tree, &mut out);
        out
    }

    /// A ajuda de todo comando do `mustard` segue a regra das frases do
    /// programa: nenhuma palavra toda em maiúsculas fora de crase, salvo a
    /// lista curta de siglas e unidades. A falha lista o comando, o argumento
    /// e a palavra.
    #[test]
    fn every_command_help_keeps_uppercase_inside_backticks() {
        use clap::CommandFactory;
        let tree = Cli::command();
        assert!(tree.get_subcommands().count() >= 3, "the check reached every command");
        let defects = tree_defects(&tree);
        assert!(defects.is_empty(), "{} help texts break the uppercase rule:\n{}", defects.len(), defects.join("\n"));
    }

    /// Um "THE" solto na ajuda de um comando, ou na de um argumento dele, cai
    /// com o comando, o argumento e a palavra; entre crases ele passa.
    #[test]
    fn a_loose_uppercase_word_in_a_help_fails_naming_the_command() {
        use clap::CommandFactory;
        let with = |about: &str, help: &str| {
            let (about, help) = (about.to_string(), help.to_string());
            Cli::command().mut_subcommand("init", move |init| init.about(about).mut_arg("force", move |force| force.help(help)))
        };
        assert_eq!(
            tree_defects(&with("Seeds THE bootstrap.", "Overwrites the folder.")),
            vec!["mustard init: uppercase word THE outside backticks"]
        );
        assert_eq!(
            tree_defects(&with("Seeds the bootstrap.", "Overwrites THE folder.")),
            vec!["mustard init --force: uppercase word THE outside backticks"]
        );
        assert_eq!(tree_defects(&with("Seeds `THE` bootstrap.", "Overwrites `THE` folder.")), Vec::<String>::new());
    }
}