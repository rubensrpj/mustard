//! The `run` subcommands for the spec lifecycle (`spec/`).
//!
//! A new command takes its variant in [`SpecCmd`] and its arm in
//! [`dispatch`] below (the compiler demands the arm), its line in
//! `tests/fixtures/run-surface.txt`, which `tests/run_command_surface.rs`
//! compares with the clap tree, and a caller in the product text, which
//! `tests/template_parity.rs` demands with no exception list.
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
    /// `HTML`. Sem `--title`, o título é a primeira linha `# Título`.
    /// Devolve `{ok, path}`.
    #[command(name = "page")]
    #[command(display_order = 17)]
    Page {
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
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Conta o consumo local da máquina pelas conversas.
    ///
    /// Sem argumento, atualiza os dias fechados e mostra o resumo; não gera
    /// páginas ou lotes externos. `--publish` prepara esses recursos quando
    /// solicitados. `--republish` prepara uma nova publicação com todos os dias.
    /// `--url` grava um endereço confirmado. Funciona sem spec aberta.
    #[command(name = "spend")]
    #[command(display_order = 23)]
    Spend {
        /// Prepara a página e os lotes externos sob pedido explícito.
        #[arg(long, conflicts_with = "url")]
        publish: bool,
        /// Prepara a publicação nova da página e a cópia de todos os dias.
        #[arg(long, conflicts_with = "url")]
        republish: bool,
        /// Grava o endereço que a publicação da página devolveu.
        #[arg(long)]
        url: Option<String>,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Mede o gasto do Claude no projeto antes e depois da marca de uma
    /// versão do Mustard.
    ///
    /// A marca é a primeira sessão de cada compilação do Mustard no projeto,
    /// gravada pelo início da sessão. Compara os dias contados (fechados, com
    /// 100 ações ou mais) dos dois lados, pela mesma conta da página do gasto,
    /// e diz o veredito numa frase, com quantas buscas o Mustard respondeu
    /// desde a marca. Só lê: não grava nada e não chama o Jev.
    #[command(name = "measure")]
    #[command(display_order = 24)]
    Measure {
        /// O instante da marca, em `RFC 3339` ou `AAAA-MM-DD`; sem ele, a última
        /// marca do projeto.
        #[arg(long)]
        since: Option<String>,
        /// Uma linha JSON por busca que o Mustard respondeu desde a marca, no
        /// lugar da resposta.
        #[arg(long)]
        lines: bool,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

/// Dispatch one `spec`-family `run` subcommand.
pub fn dispatch(cmd: SpecCmd) {
    match cmd {
        SpecCmd::Page { body, out, title, subtitle, kind, root } => {
            spec::page::run(&spec::page::PageOpts { root, body, out, title, subtitle, kind });
        }
        SpecCmd::Spend { publish, republish, url, root } => {
            spec::spend::run(&spec::spend::SpendOpts { root, publish, republish, url });
        }
        SpecCmd::Measure { since, lines, root } => {
            spec::measure::run(&spec::measure::MeasureOpts { root, since, lines });
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

    /// A lista dos itens combinados sem dono saiu: `--spec` e `--owners`, que
    /// só ela usava, não existem mais no comando de página. Pedi-los é
    /// recusado na linha de comando, como qualquer opção que não existe,
    /// antes de qualquer leitura de disco.
    #[test]
    fn the_owners_page_is_gone() {
        let cases: [&[&str]; 2] = [&["t", "page", "--spec", "x", "--owners"], &["t", "page", "--owners", "x"]];
        for args in cases {
            let refused = Probe::try_parse_from(args).map(|probe| probe.cmd).err().map(|e| e.kind());
            assert_eq!(refused, Some(clap::error::ErrorKind::UnknownArgument), "{args:?}");
        }
    }
}
