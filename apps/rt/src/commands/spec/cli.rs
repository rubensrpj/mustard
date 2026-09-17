//! The `run` subcommands for the spec lifecycle (`spec/`).
//!
//! FOUR registrations per command. Two live in this file: the variant in
//! [`SpecCmd`] AND its arm in [`dispatch`] below; forgetting the arm still
//! compiles, but the command vanishes from the CLI. The other two live in
//! the tests: the name in `tests/run_command_surface.rs`, and a caller (or a
//! justified `RUNTIME_WHITELIST` line) in `tests/template_parity.rs`.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run spec <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{spec};

/// The `run` subcommands owned by the spec lifecycle (`spec/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum SpecCmd {
    /// Gera uma página no layout do Mustard, pelo motor de página, com as
    /// fontes do Google Fonts.
    ///
    /// Com `--body` e `--out`, gera uma página avulsa (análise, relatório,
    /// plano) a partir de um arquivo markdown: escreve-se markdown, nunca
    /// HTML. Sem `--title`, o título é a primeira linha `# Título`. Com
    /// `--spec`, refaz o `spec.md` e o `spec.html` da spec a partir do
    /// `spec.ndjson`; com `--spec` e `--owners`, grava a lista dos itens sem
    /// dono, com a proposta de dono de cada um, para conferir antes de gravar.
    /// Devolve `{ok, path}`, `{ok, spec, md, html}` ou
    /// `{ok, spec, html, unowned, proposed, given, left, items}`.
    #[command(name = "page")]
    #[command(display_order = 17)]
    Page {
        /// A spec cuja página e cujo `.md` são refeitos.
        #[arg(long, conflicts_with_all = ["body", "out", "title", "subtitle", "kind"])]
        spec: Option<String>,
        /// O arquivo markdown da página avulsa.
        #[arg(long)]
        body: Option<PathBuf>,
        /// Onde gravar a página avulsa; pastas ausentes são criadas.
        #[arg(long)]
        out: Option<PathBuf>,
        /// O título, no `<title>` e no `<h1>`; sem ele, a primeira linha
        /// `# Título` do markdown.
        #[arg(long)]
        title: Option<String>,
        /// Uma linha solta sob o título, na faixa do cabeçalho.
        #[arg(long)]
        subtitle: Option<String>,
        /// O que vem depois de `Mustard · ` na faixa do cabeçalho.
        #[arg(long)]
        kind: Option<String>,
        /// Com `--spec`, grava a lista dos itens combinados sem dono
        /// (`owners.html`) no lugar da página da spec. O arquivo, quando vem,
        /// é uma lista de linhas `{code, waves | applies_to, why}` com o dono
        /// que o orquestrador dá aos itens.
        #[arg(long, requires = "spec", value_name = "DONOS_JSON")]
        // O jeito do clap de dizer "opção com valor opcional": ausente,
        // sozinha ou com o arquivo.
        #[allow(clippy::option_option)]
        owners: Option<Option<PathBuf>>,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

/// Dispatch one `spec`-family `run` subcommand.
pub fn dispatch(cmd: SpecCmd) {
    match cmd {
        SpecCmd::Page { spec: slug, body, out, title, subtitle, kind, owners, root } => {
            spec::page::run(&spec::page::PageOpts {
                root,
                spec: slug,
                body,
                out,
                title,
                subtitle,
                kind,
                owners: owners.is_some(),
                given: owners.flatten(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SpecCmd;
    use clap::Parser;

    /// A wrapper so the family enum can be parsed on its own — the binary's own
    /// `Cli` lives in `main.rs` and is out of reach from the lib.
    #[derive(Parser)]
    struct Probe {
        #[command(subcommand)]
        cmd: SpecCmd,
    }

    /// `--owners` vem sozinho ou com o arquivo de donos, e só junto de
    /// `--spec`.
    #[test]
    fn owners_comes_alone_or_with_the_file_and_only_with_a_spec() {
        let owners = |args: &[&str]| match Probe::try_parse_from(args).map(|probe| probe.cmd) {
            Ok(SpecCmd::Page { owners, .. }) => Ok(owners),
            Err(e) => Err(e.kind()),
        };
        assert_eq!(owners(&["t", "page", "--spec", "x", "--owners"]), Ok(Some(None)));
        assert_eq!(
            owners(&["t", "page", "--spec", "x", "--owners", "donos.json"]),
            Ok(Some(Some("donos.json".into())))
        );
        assert_eq!(owners(&["t", "page", "--spec", "x"]), Ok(None));
        assert_eq!(owners(&["t", "page", "--owners"]), Err(clap::error::ErrorKind::MissingRequiredArgument));
    }
}
