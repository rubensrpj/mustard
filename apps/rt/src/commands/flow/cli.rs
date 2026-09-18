//! Os comandos `run` do fluxo da spec (`flow/`).
//!
//! Um comando novo leva a variante em [`FlowCmd`] e o braço dela no
//! [`dispatch`] abaixo (o compilador cobra o braço), a linha dele em
//! `tests/fixtures/run-surface.txt`, que `tests/run_command_surface.rs` compara
//! com a árvore do clap, e um chamador no texto do produto, que
//! `tests/template_parity.rs` exige sem lista de exceções.
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
    #[command(display_order = 0)]
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
    #[command(display_order = 1)]
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
    /// O passo do plano, depois que a especificação, as ondas e as tarefas
    /// estão gravadas: monta o pedido de cada onda com as lições e as skills;
    /// confere que não há ponto do levantamento aberto, que o plano não tem
    /// erro de montagem, que cada pedido cabe no teto de linhas, que os
    /// arquivos e os nomes citados existem, que o arquivo citado está no git e
    /// que ondas da mesma rodada não dividem arquivo; avisa os itens sem
    /// tarefa; refaz o índice, prepara a cópia da spec para o banco de dados
    /// da página e responde o próximo passo: publicar a página que ainda não
    /// tem endereço, copiar os lotes e fazer a pergunta de aprovação. Grava a
    /// fase do plano; a spec que já está nela só tem a cópia preparada.
    #[command(display_order = 2)]
    Plan {
        /// A spec cujo plano é conferido. Sem ela, a spec atual.
        #[arg(long)]
        spec: Option<String>,
        /// Qualquer pasta dentro do repositório. Por padrão, a pasta atual.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Uma rodada de ondas, que é uma chamada só. Sem o relatório, despacha:
    /// escolhe as ondas que podem sair juntas, monta o pedido de cada uma,
    /// grava o envio com o pedido exato e marca a spec como em execução na
    /// primeira rodada. Com o relatório da rodada anterior, primeiro grava o
    /// que cada onda entregou e o veredito da revisão, formata só os arquivos
    /// da rodada e faz o commit, e só então despacha a rodada seguinte. A
    /// resposta manda copiar para o banco de dados das páginas o que entrou na
    /// spec desde a última cópia.
    #[command(display_order = 3)]
    Round {
        /// A spec cuja rodada corre. Sem ela, a spec atual.
        #[arg(long)]
        spec: Option<String>,
        /// O relatório da rodada anterior: a linha do fim de cada agente,
        /// como ela veio — `<DELIVERED>{…}</DELIVERED>` do agente de onda e
        /// `<VERDICT>{…}</VERDICT>` do revisor, uma por linha.
        #[arg(long)]
        report: Option<String>,
        /// Qualquer pasta dentro do repositório. Por padrão, a pasta atual.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Fecha uma spec: grava o que voltou da última rodada, confere se a obra
    /// terminou (nenhuma onda sem commit, nenhuma reprovada e nenhum pedido
    /// do usuário sem onda que o entregue), roda o lint do projeto inteiro e
    /// cada critério uma vez, gravando a execução de cada um, e então grava a
    /// fase fechada, solta a spec da sessão e prepara a cópia para o banco de
    /// dados das páginas. A spec de duas
    /// ondas ou mais recebe antes o pedido da revisão final do conjunto, e só
    /// fecha com a linha dela aprovada.
    #[command(display_order = 4)]
    Close {
        /// A spec que fecha. Sem ela, a spec atual.
        #[arg(long)]
        spec: Option<String>,
        /// O relatório da última rodada, no mesmo formato da rodada; na spec
        /// de duas ondas ou mais, também a linha da revisão final do conjunto.
        #[arg(long)]
        report: Option<String>,
        /// Qualquer pasta dentro do repositório. Por padrão, a pasta atual.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Retoma uma spec: lê só o estado e devolve, pela fase em que ela está,
    /// o próximo passo em palavras e o comando que o faz. É o que o
    /// `/mustard:continue` chama. Nenhum endereço de página entra na resposta.
    #[command(display_order = 10)]
    Resume {
        /// A spec retomada. Sem ela, a spec atual.
        #[arg(long)]
        spec: Option<String>,
        /// Qualquer pasta dentro do repositório. Por padrão, a pasta atual.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Descarta uma spec, em dois passos. A primeira chamada mostra o que vai
    /// sair — o pull request, a branch local, a do servidor quando a opção
    /// vier e a pasta da spec — e devolve um código; a segunda, com esse
    /// código e depois do sim do usuário, faz. A spec guardada continua no
    /// índice e na página do projeto, marcada como descartada.
    #[command(display_order = 12)]
    Discard {
        /// A spec descartada. Sem ela, a spec atual.
        #[arg(long)]
        spec: Option<String>,
        /// Apagar também a branch do servidor. Sem ela, só a local sai.
        #[arg(long)]
        remote: bool,
        /// Apagar a pasta da spec, e a linha dela no índice, em vez de
        /// guardá-la ao lado das outras descartadas.
        #[arg(long)]
        delete: bool,
        /// O código que a primeira chamada devolveu, passado depois do sim do
        /// usuário.
        #[arg(long)]
        confirm: Option<String>,
        /// Qualquer pasta dentro do repositório. Por padrão, a pasta atual.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Leva a spec de volta ao levantamento, com o motivo gravado no evento
    /// da volta: a fase volta a ser a de levantamento, o `grill` roda de
    /// novo, e os pontos novos convivem com o que já foi decidido. Nada do
    /// que está gravado é apagado. Uma spec fechada, com o pull request
    /// aberto, entregue ou descartada é recusada, dizendo a fase em que está.
    #[command(display_order = 11)]
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

/// Despacha um comando `run` da família do fluxo: roda o passo e responde
/// por [`flow::answer`], que grava a chamada.
pub fn dispatch(cmd: FlowCmd) {
    let started = std::time::Instant::now();
    match cmd {
        FlowCmd::Open { kind, name, base, pending, root } => {
            let opts = flow::open::OpenOpts { root, kind, name, base, pending };
            let named = opts.name.clone();
            flow::answer("open", &opts.root, named.as_deref(), started, &flow::open::open_at(&opts));
        }
        FlowCmd::Grill { spec, kinds, condensed, root } => {
            let opts = flow::grill::GrillOpts { root, spec, kinds, condensed };
            flow::answer("grill", &opts.root, opts.spec.as_deref(), started, &flow::grill::grill_at(&opts));
        }
        FlowCmd::Plan { spec, root } => {
            let opts = flow::plan::PlanOpts { root, spec };
            flow::answer("plan", &opts.root, opts.spec.as_deref(), started, &flow::plan::plan_at(&opts));
        }
        FlowCmd::Round { spec, report, root } => {
            let opts = flow::round::RoundOpts { root, spec, report };
            flow::answer("round", &opts.root, opts.spec.as_deref(), started, &flow::round::round_at(&opts));
        }
        FlowCmd::Close { spec, report, root } => {
            let opts = flow::close::CloseOpts { root, spec, report };
            flow::answer("close", &opts.root, opts.spec.as_deref(), started, &flow::close::close_at(&opts));
        }
        FlowCmd::Resume { spec, root } => {
            let opts = flow::resume::ResumeOpts { root, spec };
            flow::answer("resume", &opts.root, opts.spec.as_deref(), started, &flow::resume::resume_at(&opts));
        }
        FlowCmd::Discard { spec, remote, delete, confirm, root } => {
            let opts = flow::discard::DiscardOpts { root, spec, remote, delete, confirm };
            flow::answer("discard", &opts.root, opts.spec.as_deref(), started, &flow::discard::discard_at(&opts));
        }
        FlowCmd::Reopen { reason, spec, root } => {
            let opts = flow::reopen::ReopenOpts { root, spec, reason };
            flow::answer("reopen", &opts.root, opts.spec.as_deref(), started, &flow::reopen::reopen_at(&opts));
        }
    }
}
