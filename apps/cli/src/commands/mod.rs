//! One module per `mustard` subcommand.
//!
//! Mustard 2.0 thin bootstrap: [`init`] seeds the harness (settings.json,
//! the `.claude/mustard/` injectables, `.gitignore`, `mustard.json`) and enables the
//! `mustard` plugin; [`config`] reconfigures git flow. `update`, `add` e o
//! sugeridor de gramáticas saíram — a atualização vem do marketplace do
//! plugin, o molde de terceiros nenhum item da spec pede, e o sugeridor não
//! baixava nem compilava nada: só imprimia um repositório e uma linha de
//! shell, sem nenhum chamador e fora da lista de comandos aprovada.
//!
//! [`git_flow`] is not a subcommand but the shared git-flow configuration
//! routine `init` runs to produce the project-root `mustard.json`; [`config`]
//! is a thin wrapper over it.

pub mod config;
pub mod git_flow;
pub mod init;
pub mod install_nerd_font;