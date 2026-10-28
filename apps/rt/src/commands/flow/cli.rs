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
    /// List native measurement receipts or attach one to a survey point.
    #[command(display_order=38)]
    Evidence {
        #[arg(long,default_value=".")] root:PathBuf,
        #[arg(long)] spec:Option<String>,
        #[arg(long)] wave:Option<u64>,
        #[arg(long,requires_all=["receipt","text"])] point:Option<String>,
        #[arg(long,requires="point")] receipt:Option<String>,
        #[arg(long,requires="point")] text:Option<String>,
    },
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
    /// Conduz o levantamento de uma spec: grava o tipo de trabalho, monta a
    /// lista de pontos — as lacunas de cada tipo (juntas sem repetir num
    /// pedido misto), as lições e as specs anteriores que casam com o
    /// objetivo, e até três mensagens antigas do usuário como lembretes
    /// dentro desses pontos — e grava cada ponto que falta, aberto, com os
    /// fatos que a lista traz. O assistente inclui os fatos conferidos
    /// no próprio `answer --facts`, que grava a resposta e fecha o ponto. Rodar o grill de novo
    /// não grava nada e devolve o primeiro ponto aberto.
    #[command(display_order = 1)]
    Grill {
        /// A spec levantada. Sem ela, a spec atual.
        #[arg(long)]
        spec: Option<String>,
        /// O tipo de trabalho, separado por vírgula: `feature`, `fix`,
        /// `refactor`. Sem ele, vale o tipo já gravado e, na primeira vez, o
        /// do começo da branch da spec (`fix/x` dá `fix`).
        #[arg(long)]
        kinds: Option<String>,
        /// Um pedido que cabe numa frase: todos os pontos num bloco só,
        /// mostrados de uma vez para um sim só.
        #[arg(long)]
        condensed: bool,
        /// A resposta inteira, com cada ponto da lista e o conhecimento achado.
        /// Sem ela, vêm a dica, o próximo ponto, os códigos dos pontos abertos
        /// e só o que a dica manda usar.
        #[arg(long)]
        full: bool,
        /// Qualquer pasta dentro do repositório. Por padrão, a pasta atual.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// O passo do plano, depois que a especificação, as ondas e as tarefas
    /// estão gravadas: monta o pedido de cada onda com as lições e as skills;
    /// confere que não há ponto do levantamento aberto, que o plano não tem
    /// erro de montagem, que os arquivos e os nomes citados existem, que o arquivo citado está no git e
    /// que ondas da mesma rodada não dividem arquivo; avisa os itens sem
    /// tarefa; refaz o índice e prepara a pergunta de aprovação. Grava a
    /// fase do plano. O acompanhamento é local; não prepara publicação.
    #[command(display_order = 2)]
    Plan {
        /// A spec cujo plano é conferido. Sem ela, a spec atual.
        #[arg(long)]
        spec: Option<String>,
        /// Qualquer pasta dentro do repositório. Por padrão, a pasta atual.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Sem opções de ação, consulta o estado sem alterar a execução.
    /// Com --dispatch ou --report, executa uma rodada:
    /// escolhe as ondas que podem sair juntas, monta o pedido de cada uma,
    /// grava o envio com o pedido exato e marca a spec como em execução na
    /// primeira rodada. Com a entrega que uma onda gravou na spec, primeiro
    /// assume a volta e grava o veredito da revisão, formata só os arquivos
    /// da rodada e faz o commit, sem compilar nem rodar prova, e então
    /// despacha as dependentes liberadas. A compilação, o lint, a suíte e as
    /// provas ficam para o fechamento.
    /// Nenhuma etapa da rodada publica uma página externa.
    #[command(display_order = 3)]
    Round {
        /// A spec cuja rodada corre. Sem ela, a spec atual.
        #[arg(long)]
        spec: Option<String>,
        /// O relatório da rodada anterior, uma linha por marca. A entrega da
        /// onda e o veredito não vêm aqui: o agente de onda grava a entrega
        /// com `run write delivered`, o revisor grava o veredito com `run
        /// write verdict`, e a linha `<DELIVERED>` ou `<VERDICT>` no relatório
        /// é recusada. A onda que gravou a entrega vai sem linha de uso: o
        /// binário sabe quem voltou, e o consumo a rodada mede nos arquivos
        /// de conversa que a plataforma grava, nunca digitado pelo agente nem
        /// por quem despacha. A linha `<USAGE>{"wave":1}</USAGE>`, só com a
        /// onda, fica para o agente que terminou sem gravar a entrega, e a
        /// `<PAUSED>` para a pausa. Para reprovar a volta de uma onda, vai a
        /// linha `<REJECTED>{"wave":1,"reason":"…"}</REJECTED>`,
        /// com o motivo: a rodada segura a volta fora do commit, a onda espera
        /// um agente novo, e o motivo vai a ele. Para recusar a mudança de
        /// plano que uma onda propôs, quando a mensagem do usuário já recusa,
        /// vai a linha `<PLAN_REFUSED>{"wave":1,"reason":"…"}</PLAN_REFUSED>`:
        /// como o clique em "Recusar", a rodada comita o que a onda mudou e
        /// devolve as tarefas dela soltas ao backlog. A onda que mudou código
        /// leva a `REVIEWED` dela na mesma rodada. Aceitar a mudança é só
        /// pelo clique do usuário.
        /// Código exige revisão prévia por `run review-wave`, que responde com
        /// o resumo. Depois de ler e conferir regras/testes, aceite com
        /// `<REVIEWED>{"wave":1,"note":"…"}</REVIEWED>`: o aceite vale para a
        /// impressão da volta que você viu, sem copiá-la; `fingerprint` na
        /// linha prende o aceite a outra. `USAGE` sozinho não aprova;
        /// alteração invalida a revisão.
        #[arg(long)]
        report: Option<String>,
        /// Autoriza avançar a execução e despachar ondas. Sem relatório nem
        /// esta opção, o comando somente consulta o estado.
        #[arg(long)]
        dispatch: bool,
        /// A resposta inteira, com cada tarefa segurada e o detalhe dela. Sem
        /// ela, as seguradas vêm agrupadas pelo motivo, com o detalhe uma vez.
        #[arg(long)]
        full: bool,
        /// Qualquer pasta dentro do repositório. Por padrão, a pasta atual.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Mostra o resumo do diff real da onda para revisão antes do commit: os
    /// arquivos com as linhas postas e tiradas, o que saiu do plano e os
    /// passos e provas da onda. Depois de ler, passe
    /// `<REVIEWED>{"wave":1,"note":"…"}</REVIEWED>` no relatório da rodada,
    /// sem copiar a impressão. Mudança posterior invalida a revisão. A
    /// resposta abre com a impressão digital e traz cada regra do projeto
    /// pelo caminho e pelo tamanho.
    #[command(display_order = 22)]
    ReviewWave {
        #[arg(long)]
        wave: u64,
        #[arg(long)]
        spec: Option<String>,
        /// A resposta inteira, com o diff e o texto de cada regra do projeto.
        #[arg(long)]
        full: bool,
        /// O diff inteiro da onda, junto do resumo.
        #[arg(long)]
        diff: bool,
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Fecha uma spec: grava o que voltou da última rodada, confere se a obra
    /// terminou (nenhuma onda sem commit, nenhuma reprovada sem o conserto e
    /// nenhum pedido do usuário sem onda que o entregue), roda em ambiente
    /// limpo a compilação, o lint e a suíte que o `mustard.json` declara e
    /// cada critério uma vez, gravando a execução de cada um, e então grava a
    /// fase fechada, solta a spec da sessão e prepara a cópia para o banco de
    /// dados das páginas. Toda obra, mesmo a de uma onda só, recebe antes o pedido do
    /// agente de teste dedicado, numa cópia que o fechamento cria e apaga, e
    /// só fecha com a linha dele aprovada.
    #[command(display_order = 4)]
    Close {
        /// A spec que fecha. Sem ela, a spec atual.
        #[arg(long)]
        spec: Option<String>,
        /// O relatório da última rodada, no mesmo formato da rodada. O
        /// veredito do agente de teste dedicado não vem aqui: ele o grava com
        /// `run write verdict`.
        #[arg(long)]
        report: Option<String>,
        /// A resposta "fica para depois" do usuário a uma pendência desta
        /// obra, uma por item (`--pending-later "P-83=o motivo"`): a
        /// pendência solta da obra e passa a ser do projeto.
        #[arg(long = "pending-later", value_name = "ID=MOTIVO")]
        pending_later: Vec<String>,
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
    /// Reabre a spec para um pedido novo, com o motivo gravado no evento da
    /// volta. Nada do que está gravado é apagado. A spec que ainda não fechou
    /// volta ao levantamento: o `grill` roda de novo, e os pontos novos
    /// convivem com o que já foi decidido. A fechada ou com o pull request
    /// aberto volta à execução, já aprovada, na mesma branch: o pedido novo
    /// entra pelo `write request`, como tarefas novas. A entregue na base, a
    /// descartada e a sem fase gravada são recusadas.
    ///
    /// Com `--fix`, este passo é a porta de conserto do pull request que o
    /// servidor reprovou: só com o vermelho relatado pelo provedor, abre a
    /// onda de conserto dentro da mesma spec e, depois que a rodada a entrega
    /// e comita na mesma branch, empurra a branch para o servidor rodar de
    /// novo — sem reabrir a obra e sem spec nova. Sem o vermelho, recusa, e
    /// nada é gravado.
    #[command(display_order = 11)]
    Reopen {
        /// Por que a spec volta, numa frase; com `--fix`, o que o servidor
        /// reprovou. Obrigatório: é ele que explica depois por que a spec
        /// voltou.
        #[arg(long)]
        reason: String,
        /// A porta de conserto do pull request que o servidor reprovou, em
        /// vez da reabertura.
        #[arg(long)]
        fix: bool,
        /// A spec que volta. Sem ela, a spec atual.
        #[arg(long)]
        spec: Option<String>,
        /// Qualquer pasta dentro do repositório. Por padrão, a pasta atual.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Grava numa chamada só a resposta de um ponto do levantamento: o item
    /// da resposta, pela mesma porta e com as mesmas recusas do `write`; o
    /// ponto que fecha o aberto; e o passo seguinte, como o `write` o
    /// devolve. O item sem `origin` recebe o número da última mensagem do
    /// usuário, e o fechamento aponta a mesma mensagem. As duas gravações vão
    /// juntas, sob a trava da spec: o item recusado não fecha o ponto, e o
    /// fechamento recusado tira o item. No levantamento condensado, `points`
    /// traz o código, o número e a lacuna de cada ponto aberto, e `point`
    /// traz o próximo inteiro. O texto longo do item entra cru, sem virar
    /// JSON: `--text -` ou `--agent -` lê esse campo da entrada padrão, por
    /// heredoc, junto dos outros campos do `--json`.
    #[command(display_order = 25)]
    Answer {
        /// A spec do levantamento. Sem ela, a spec atual.
        #[arg(long)]
        spec: Option<String>,
        /// O ponto respondido, pelo número ou pelo código que a página
        /// mostra. Sem ele, o primeiro ponto aberto.
        #[arg(long)]
        point: Option<String>,
        /// O tipo do item da resposta, como `decision` ou `rule`. Vem junto
        /// com `--json`.
        #[arg(long = "type", value_name = "TYPE")]
        event_type: Option<String>,
        /// Os campos do item da resposta, num objeto JSON, nos mesmos campos
        /// do `write`. Vem junto com `--type`.
        #[arg(long)]
        json: Option<String>,
        /// O campo `text` do item, cru: `-` lê a entrada padrão inteira como
        /// foi escrita; outro valor entra como veio. Vem junto com `--type`, e
        /// o campo não vem também no `--json`.
        #[arg(long, value_name = "TEXT")]
        text: Option<String>,
        /// O campo `agent` do item, cru, como o `--text`. Só um dos dois lê
        /// a entrada padrão.
        #[arg(long, value_name = "AGENT")]
        agent: Option<String>,
        /// Fatos conferidos no código ou na conversa, como uma lista JSON
        /// de {"text":"...","source":"..."}. Gravados com o fechamento.
        #[arg(long)]
        facts: Option<String>,
        /// Os itens já gravados que respondem o ponto, por número ou código,
        /// separados por vírgula: somam-se ao item novo, ou vêm sozinhos.
        #[arg(long)]
        result: Option<String>,
        /// O ponto não se aplica: fecha sem item, com o motivo em `--reason`.
        #[arg(long)]
        not_applicable: bool,
        /// O motivo do fechamento. Obrigatório com `--not-applicable`.
        #[arg(long)]
        reason: Option<String>,
        /// Qualquer pasta dentro do repositório. Por padrão, a pasta atual.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Reorganiza a lista aberta pelo classificador das atividades, numa
    /// chamada ao Jev: os grupos de mesmo sentido (a primeira tarefa de cada
    /// grupo fica), as tarefas que uma entrega feita já cobre e a ordem dos
    /// assuntos por valor (o que destrava mais tarefas ou o usuário, o pedido
    /// urgente, o que corta gasto; no empate, o de resultado mais rápido).
    /// Sem `--apply`, não grava nada e responde curto. A tarefa presa em onda
    /// no ar fica de fora, com aviso; sem Jev, a lista fica como está.
    #[command(display_order = 41)]
    Triage {
        /// A spec cuja lista aberta é reorganizada. Sem ela, a spec atual.
        #[arg(long)]
        spec: Option<String>,
        /// Grava, sob a trava e tudo ou nada, a versão nova da primeira
        /// tarefa de cada grupo, com o trabalho das outras, e remove as outras
        /// e as cobertas, com o motivo. A resposta traz, para cada versão
        /// nova, o pedido ao agente de análise da sessão que escreve o texto.
        #[arg(long)]
        apply: bool,
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
        FlowCmd::Evidence {root,spec,wave,point,receipt,text}=>{
            let report=flow::evidence::run(&root,spec.as_deref(),wave,point.as_deref(),receipt.as_deref(),text.as_deref());
            flow::answer("evidence",&root,spec.as_deref(),started,&report, false);
        }
        FlowCmd::Open { kind, name, base, pending, root } => {
            let session = crate::shared::spec_state::session_from_env();
            let opts = flow::open::OpenOpts { root, kind, name, base, pending, session };
            let named = opts.name.clone();
            flow::answer("open", &opts.root, named.as_deref(), started, &flow::open::open_at(&opts), false);
        }
        FlowCmd::Grill { spec, kinds, condensed, full, root } => {
            let opts = flow::grill::GrillOpts { root, spec, kinds, condensed };
            flow::answer("grill", &opts.root, opts.spec.as_deref(), started, &flow::grill::grill_at(&opts), full);
        }
        FlowCmd::Plan { spec, root } => {
            let opts = flow::plan::PlanOpts { root, spec };
            flow::answer("plan", &opts.root, opts.spec.as_deref(), started, &flow::plan::plan_at(&opts), false);
        }
        FlowCmd::Round { spec, report, root, dispatch, full } => {
            if !dispatch && report.is_none() {
                // O estado da rodada não liga a conversa à spec: só a abertura e
                // a retomada ligam.
                let session = crate::shared::spec_state::session_from_env();
                let mut out = flow::resume::resume_for(&flow::resume::ResumeOpts { root, spec }, session.as_deref());
                out["mode"] = serde_json::json!("status");
                out["mutated"] = serde_json::json!(false);
                println!("{out}");
                if out["ok"] != true {
                    std::process::exit(1);
                }
                return;
            }
            let opts = flow::round::RoundOpts { root, spec, report };
            flow::answer("round", &opts.root, opts.spec.as_deref(), started, &flow::round::round_at(&opts), full);
        }
        FlowCmd::ReviewWave { wave, spec, full, diff, root } => {
            let out = flow::round::review::view_at(&root, spec.as_deref(), wave, diff || full);
            flow::answer("review-wave", &root, spec.as_deref(), started, &out, full);
        }
        FlowCmd::Close { spec, report, pending_later, root } => {
            let opts = flow::close::CloseOpts { root, spec, report, pending_later };
            flow::answer("close", &opts.root, opts.spec.as_deref(), started, &flow::close::close_at(&opts), false);
        }
        FlowCmd::Resume { spec, root } => {
            let opts = flow::resume::ResumeOpts { root, spec };
            flow::answer("resume", &opts.root, opts.spec.as_deref(), started, &flow::resume::resume_at(&opts), false);
        }
        FlowCmd::Discard { spec, remote, delete, confirm, root } => {
            let opts = flow::discard::DiscardOpts { root, spec, remote, delete, confirm };
            flow::answer("discard", &opts.root, opts.spec.as_deref(), started, &flow::discard::discard_at(&opts), false);
        }
        FlowCmd::Reopen { reason, fix, spec, root } => {
            let opts = flow::reopen::ReopenOpts { root, spec, reason, fix };
            flow::answer("reopen", &opts.root, opts.spec.as_deref(), started, &flow::reopen::reopen_at(&opts), false);
        }
        FlowCmd::Triage { spec, apply, root } => {
            let opts = flow::triage::TriageOpts { root, spec, apply };
            flow::answer("triage", &opts.root, opts.spec.as_deref(), started, &flow::triage::triage_at(&opts), false);
        }
        FlowCmd::Answer { spec, point, event_type, json, text, agent, facts, result, not_applicable, reason, root } => {
            use crate::commands::spec_events::{project, refused, write::with_raw_fields};
            use crate::shared::json_arg::resolve_opt;
            let lang = project(&root).lang;
            // O arquivo do `--json` ou do `--facts` que não se lê recusa sem
            // gravar nada, e os campos crus do item entram no `--json`.
            let read = resolve_opt(json)
                .and_then(|json| Ok((json, resolve_opt(facts)?)))
                .map_err(|detail| refused(&mustard_core::domain::spec_events::Refusal::Io { detail }, lang))
                .and_then(|(json, facts)| {
                    if text.is_none() && agent.is_none() {
                        return Ok((json, facts));
                    }
                    let stdin = || std::io::read_to_string(std::io::stdin());
                    let joined = with_raw_fields(json.unwrap_or_else(|| "{}".into()), [("text", text), ("agent", agent)], stdin);
                    joined.map(|json| (Some(json), facts)).map_err(|refusal| refusal.to_value(lang))
                });
            match read {
                Ok((json, facts)) => {
                    let opts = flow::answer::AnswerOpts { root, spec, point, event_type, json, facts, result, not_applicable, reason };
                    flow::answer("answer", &opts.root, opts.spec.as_deref(), started, &flow::answer::answer_at(&opts), false);
                }
                Err(refused) => flow::answer("answer", &root, spec.as_deref(), started, &refused, false),
            }
        }
    }
}
