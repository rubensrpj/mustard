//! A linha de comando de `mustard-rt`: o programa e as suas duas portas.
//!
//! Mora fora do `main.rs` porque o binário não roda teste (`test = false` em
//! `apps/rt/Cargo.toml`). A biblioteca declara este módulo também, e assim o
//! teste da ajuda percorre a árvore que a pessoa digita, a partir da raiz: o
//! texto do programa, o de `on` e o de cada comando de `run`.

use crate::commands;
use clap::{Parser, Subcommand};

/// The `mustard-rt` command line.
#[derive(Debug, Parser)]
#[command(name = "mustard-rt", version = env!("MUSTARD_VERSION_FULL"), about = "Mustard enforcement runtime")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// As faces do binário. `Run` não lê o stdin.
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum — single-use stack alloc, indirection adds no value
pub enum Command {
    /// Roda os ganchos de um evento do Claude Code.
    On {
        /// O nome do evento, como `PreToolUse` ou `Stop`.
        event: String,
    },
    /// Roda um comando. Recebe argumentos, não o stdin.
    Run {
        #[command(subcommand)]
        command: commands::RunCmd,
    },
}
