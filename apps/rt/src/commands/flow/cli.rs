//! Os comandos `run` do fluxo da spec (`flow/`).
//!
//! QUATRO registros por comando. Dois moram neste arquivo: a variante em
//! [`FlowCmd`] E o braço dela no [`dispatch`] abaixo; esquecer o braço ainda
//! compila, mas o comando some da linha de comando. Os outros dois moram nos
//! testes: o nome em `tests/run_command_surface.rs` e um chamador (ou uma
//! linha justificada no `RUNTIME_WHITELIST`) em `tests/template_parity.rs`.
//!
//! O [`crate::commands::RunCmd`] junta este enum com `#[command(flatten)]`,
//! então todo nome fica RASO: `mustard-rt run open`, nunca `run flow open`.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::flow;

/// Os comandos `run` que são do fluxo da spec (`flow/`).
#[derive(Debug, Subcommand)]
pub enum FlowCmd {
    /// Abre uma spec: a branch `<tipo>/<nome>` e a spec `<nome>`, nascida na
    /// fase de levantamento com a branch e a base dela. O nome vale
    /// exatamente como foi escrito; o que o git recusa (espaço, acento,
    /// barra) é ajustado e mostrado para um sim antes de qualquer coisa ser
    /// criada. O tipo, o nome ou a base que faltam voltam como um passo
    /// (`choose_kind`, `choose_name`, `choose_base`), com os candidatos; nada
    /// é criado enquanto os três não forem sabidos. A resposta termina com a
    /// pergunta do objetivo, para fazer ao usuário.
    #[command(display_order = 81)]
    Open {
        /// O tipo da branch, como `feature` ou `fix`. Sem ele, um nome
        /// escrito como `<tipo>/<nome>` é partido nos dois.
        #[arg(long)]
        kind: Option<String>,
        /// O nome da spec, exatamente como o usuário escreveu. É ele que
        /// nomeia a branch `<tipo>/<nome>` e a pasta da spec.
        #[arg(long)]
        name: Option<String>,
        /// A branch de que a spec sai. Sem ela, a resposta lista as bases que
        /// o `git.flow` declara, ou as branches do repositório quando ele não
        /// declara nenhuma.
        #[arg(long)]
        base: Option<String>,
        /// A pendência aberta de que esta spec veio, como `P-12`: a pendência
        /// ganha a nota de que virou esta spec, e o merge da spec a fecha.
        #[arg(long)]
        pending: Option<String>,
        /// Qualquer pasta dentro do repositório. A branch é criada neste
        /// checkout; a spec mora no principal. Por padrão, a pasta atual.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Conduz o levantamento de uma spec: grava o tipo de trabalho e monta a
    /// lista de pontos — as lacunas de cada tipo (juntas sem repetir num
    /// pedido misto), as lições e as specs anteriores que casam com o
    /// objetivo, e até três mensagens antigas do usuário como lembretes
    /// dentro desses pontos. O assistente grava cada ponto com `write point`;
    /// rodar o grill de novo com os mesmos tipos não grava nada e devolve o
    /// primeiro ponto aberto.
    #[command(display_order = 82)]
    Grill {
        /// A spec levantada. Sem ela, a spec atual.
        #[arg(long)]
        spec: Option<String>,
        /// O tipo de trabalho, separado por vírgula: `feature`, `fix`,
        /// `refactor`.
        #[arg(long)]
        kinds: Option<String>,
        /// Um pedido que cabe numa frase: todos os pontos num bloco só,
        /// mostrados de uma vez para um sim só.
        #[arg(long)]
        condensed: bool,
        /// Qualquer pasta dentro do repositório. Por padrão, a pasta atual.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Leva a spec de volta ao levantamento, com o motivo gravado no evento
    /// da volta: a fase volta a ser a de levantamento, o `grill` roda de
    /// novo, e os pontos novos convivem com o que já foi decidido. Nada do
    /// que está gravado é apagado. Uma spec fechada, com o pull request
    /// aberto, entregue ou descartada é recusada, dizendo a fase em que está.
    #[command(display_order = 83)]
    Reopen {
        /// Por que a spec volta ao levantamento, numa frase. Obrigatório: é
        /// ele que explica depois por que o levantamento recomeçou.
        #[arg(long)]
        reason: String,
        /// A spec que volta. Sem ela, a spec atual.
        #[arg(long)]
        spec: Option<String>,
        /// Qualquer pasta dentro do repositório. Por padrão, a pasta atual.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

/// Despacha um comando `run` da família do fluxo.
pub fn dispatch(cmd: FlowCmd) {
    match cmd {
        FlowCmd::Open { kind, name, base, pending, root } => {
            flow::open::run(&flow::open::OpenOpts { root, kind, name, base, pending });
        }
        FlowCmd::Grill { spec, kinds, condensed, root } => {
            flow::grill::run(&flow::grill::GrillOpts { root, spec, kinds, condensed });
        }
        FlowCmd::Reopen { reason, spec, root } => {
            flow::reopen::run(&flow::reopen::ReopenOpts { root, spec, reason });
        }
    }
}
