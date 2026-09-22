//! `mustard-rt run round [--spec <nome>]` — uma rodada de ondas.
//!
//! É a porta única da execução, e cada rodada é uma chamada só. Sem relatório,
//! a rodada despacha: escolhe as ondas que podem sair juntas — duas no mesmo
//! arquivo inclusive —, cria a cópia separada de cada uma no commit atual e
//! escolhe a pasta de compilação dela, monta o pedido de cada uma com as duas,
//! grava o envio com o pedido exato como foi injetado e marca a spec como em
//! execução na primeira rodada. Com o relatório da rodada anterior
//! (`--report`), ela primeiro fecha o que voltou — junta ao repositório
//! principal os arquivos que cada cópia entregou, comita e apaga a cópia — e
//! só então despacha a rodada seguinte.
//!
//! **A escolha antes do envio.** Antes de criar a cópia de uma onda pronta,
//! a rodada olha os candidatos dela: os itens combinados do projeto todo, os
//! sem dono e as lições do banco que casam com ela. Com algum, a onda só sai
//! com a escolha do orquestrador, a conversa principal, e nenhum agente é
//! aberto para isso: a resposta traz em `analysis` os candidatos de cada onda,
//! cada um com o título, e o orquestrador devolve a linha
//! `<ANALYSIS>{…}</ANALYSIS>` no `--report` seguinte, sozinha ou junto das
//! outras. O envio gravado leva os itens que ficaram e, à parte, no campo
//! `analysis`, o que saiu e o que entrou, cada um com o motivo. Os itens que
//! as tarefas da onda fazem vão sempre, sem escolha. A mesma onda que sai de
//! novo sem plano novo usa a escolha do envio anterior, quando ela julgou cada
//! candidato de agora. Sem escolha, a onda espera; nada é recusado.
//!
//! **O relatório é o que os agentes devolvem, como veio.** A rodada lê, do
//! texto recebido, cada linha `<DELIVERED>{…}</DELIVERED>` do agente de onda e
//! cada linha `<VERDICT>{…}</VERDICT>` do revisor, no formato que os textos
//! deles ensinam. A linha da entrega traz a onda, a entrega, os arquivos, o
//! resumo do commit e, quando é o caso, a prova nova de um critério cujo teste
//! mudou de nome, as ondas que o conserto fecha e a mudança de plano; a do
//! veredito traz a onda, o resultado, o texto e cada critério pelo código que
//! a página mostra. Com isso a rodada grava o veredito e depois a entrega —
//! também na onda que o conserto fecha, o que pede a revisão dela de novo —,
//! grava a versão nova do critério com a prova nova, formata só os arquivos da
//! rodada e faz o commit com a mensagem montada do resumo.
//!
//! **O que trava.** Uma spec que ainda não foi aprovada; um relatório sem
//! nenhuma linha de entrega, de veredito ou de escolha, ou com uma linha de
//! entrega ou de veredito sem campo obrigatório; um
//! `entregou` acima do teto de caracteres; um arquivo entregue que não está no
//! disco nem no git, nem no repositório principal nem na cópia; uma mensagem
//! de commit fora do
//! modelo (título e corpo acima do teto, link do claude.ai, o nome do modelo,
//! assinatura de coautoria ou e-mail de alguém); a prova de um critério que
//! as ondas do relatório cobrem que não executa ou não passa — a rodada roda
//! cada uma, na ordem do código, antes de comitar, e recusa nomeando o
//! critério, o comando inteiro e a saída de erro; o relatório em que um agente
//! diz que o plano da onda não funciona, que para a rodada e só segue com o
//! "sim" do usuário. O "sim" da mudança de plano é o clique em "Aceitar" na
//! pergunta dela, gravado pela testemunha como na aprovação da spec, e nunca a
//! leitura que o modelo faz de uma frase: a rodada não aceita código nenhum de
//! quem a chama.
//!
//! **O que para sem travar.** A onda reprovada depois da segunda rodada de
//! conserto segura só ela e as ondas que dependem dela: o resto da rodada
//! segue, e a resposta traz a pergunta ao usuário com os vereditos dela. A
//! onda que sai do plano deixa de contar, na rodada e no fechamento. A
//! entrega com um trecho que a junção da cópia não resolve fica de fora, sem
//! nada dela gravado: o resto do relatório é juntado, comitado e gravado, e a
//! resposta traz a recusa dela, com os trechos e o comando que a resolve na
//! cópia; só quando o relatório não traz mais nada a recusa é a resposta.
//!
//! **O passo do git.** Da leitura do repositório à junção, ao commit e ao
//! desfazer quando o git recusa, a rodada segura uma trava só dela, a mesma da
//! remoção das cópias e do despacho — da escolha das ondas à criação das
//! cópias e à gravação dos envios: duas rodadas ao mesmo tempo no mesmo
//! checkout fazem cada um desses passos uma depois da outra, e nunca soltam a
//! mesma onda duas vezes. O commit leva só os arquivos da rodada, por caminho;
//! a recusa do git volta o disco e o índice deles, e nada da onda recusada
//! entra no commit de outra. O arquivo de dentro de um submódulo é comitado no
//! submódulo, na branch de mesmo nome da spec, e o commit do principal leva o
//! ponteiro novo; a cópia da onda traz os submódulos que ela toca.
//!
//! **O que avisa.** O formatador que o projeto declara e que não foi achado
//! sai pelo nome, em vez de a formatação ser pulada em silêncio; a prova nova
//! que sai verde sem rodar teste nenhum sai pelo código do critério; a cópia
//! que não pôde ser criada, cuja onda fica para a rodada seguinte; e a cópia
//! com mudança fora da entrega, que fica no disco em vez de ser apagada.
//!
//! A página da spec e a do projeto são refeitas no fim da rodada, e a resposta
//! manda publicá-las: a rodada é um dos marcos de publicação. Nenhum endereço
//! é impresso na conversa.

mod answer;
mod commit;
mod queue;
mod report;
mod stops;

/// O código de mudança que um texto traz: a testemunha dos gestos o lê no
/// cabeçalho da pergunta que decide a mudança.
pub(crate) use stops::change_code_of;

pub(crate) use commit::{reinstall_binary, refresh_map_if_stale};

use std::path::PathBuf;

use serde_json::Value;

use crate::commands::spec_events;
use crate::shared::spec_state::session_from_env;

pub(crate) use answer::RoundRefusal;
pub(crate) use queue::{ensure_copy, wave_states, waves_in_progress, waves_pending_fix};
pub(crate) use report::take_report;

/// As opções de `mustard-rt run round`.
pub struct RoundOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec cuja rodada corre; sem ela, a spec atual.
    pub spec: Option<String>,
    /// O relatório da rodada anterior, em JSON.
    pub report: Option<String>,
}

/// O passo que a rodada devolve quando todas as ondas estão entregues e
/// aprovadas.
pub const DONE_STEP: &str = "close";

/// O núcleo testável de [`run`]. A sessão vem do ambiente. Nunca entra em
/// pânico.
pub(crate) fn round_at(opts: &RoundOpts) -> Value {
    round_for(opts, session_from_env().as_deref())
}

/// [`round_at`] com a sessão recebida, que é como um teste a escolhe.
pub(crate) fn round_for(opts: &RoundOpts, session: Option<&str>) -> Value {
    let project = spec_events::project(&opts.root);
    let lang = project.lang;
    match answer::run_round(opts, &project.root, lang, session) {
        Ok(report) => report,
        Err(refusal) => refusal.to_value(lang),
    }
}

/// A spec na fase `phase` pode ter ondas despachadas: está aprovada, ou já em
/// execução. É a mesma pergunta para a rodada e para o gancho que monta o
/// pedido no despacho.
pub(crate) fn can_run(phase: &str) -> bool {
    matches!(phase, "approved" | "running")
}

/// O que os testes das partes da rodada dividem: a spec aprovada com o plano
/// que o teste pede, a chamada da rodada e as linhas que os agentes devolvem.
#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use mustard_core::io::spec_events as store;
    use serde_json::{json, Value};

    use super::*;
    use crate::commands::spec_events::write::{record_open, seed_at, WriteOpts};

    pub(super) fn write(root: &Path, spec: &str, event_type: &str, body: Value) -> Value {
        seed_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some(spec.to_string()),
            event_type: event_type.into(),
            json: body.to_string(),
        })
    }

    pub(super) fn id_of(report: &Value) -> u64 {
        report["id"].as_u64().unwrap_or_else(|| panic!("não gravou: {report}"))
    }

    /// A resposta do usuário à pergunta `question`, feita com o cabeçalho
    /// `header` — onde vai o código da mudança —, dada pela testemunha dos
    /// gestos, como o harness a entrega depois do clique.
    pub(super) fn click(root: &Path, session: &str, question: &str, header: &str, answer: &str) {
        use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger};
        let input = HookInput {
            hook_event_name: Some("PostToolUse".to_string()),
            tool_name: Some("AskUserQuestion".to_string()),
            session_id: Some(session.to_string()),
            tool_input: json!({ "questions": [{ "question": question, "header": header,
                "options": [{ "label": "Aceitar" }, { "label": "Recusar" }] }] }),
            raw: json!({ "tool_response": { "answers": { question: answer } } }),
            ..HookInput::default()
        };
        let ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::PostToolUse));
        crate::hooks::observe::approval_witness::ApprovalWitness.evaluate(&input, &ctx).expect("never errors");
    }

    pub(super) fn git_at(root: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Um projeto com arquivos no git e uma spec já aprovada, com o plano que
    /// o teste pedir: uma entrada por onda, com os arquivos das tarefas dela e
    /// as ondas de que ela depende.
    pub(super) fn approved(root: &Path, spec: &str, plan: &[(u64, &[&str], &[u64])]) {
        approved_with(root, spec, plan, |_| {});
    }

    /// [`approved`] com o que o teste grava antes da aprovação (`before`), que
    /// recebe o número da mensagem de origem: é antes dela que o levantamento
    /// grava os itens combinados, inclusive os sem dono.
    pub(super) fn approved_with(root: &Path, spec: &str, plan: &[(u64, &[&str], &[u64])], before: impl FnOnce(u64)) {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        for (_, files, _) in plan {
            for file in *files {
                let path = root.join(file);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, "fn um() {}\n").unwrap();
            }
        }
        git_at(root, &["init", "-q"]);
        // O fim de linha do repositório de teste é fixo: no Windows o git
        // converteria os arquivos ao criar a cópia da onda, e a junção
        // devolveria CRLF onde o teste espera LF.
        git_at(root, &["config", "core.autocrlf", "false"]);
        git_at(root, &["config", "core.eol", "lf"]);
        git_at(root, &["add", "-A"]);
        git_at(root, &["commit", "-q", "-m", "semente"]);
        git_at(root, &["config", "user.email", "t@t"]);
        git_at(root, &["config", "user.name", "t"]);
        git_at(root, &["config", "commit.gpgsign", "false"]);

        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        let said = id_of(&write(root, spec, "message", json!({"author": "user", "text": "o objetivo"})));
        // A prova é um comando que sempre passa, sem exigir um projeto Cargo
        // de verdade na cópia de teste: desde que a rodada roda a prova de
        // cada critério coberto antes de comitar, `cargo test` recusaria
        // todo commit destes testes, que escrevem em pastas soltas, sem
        // `Cargo.toml`.
        let crit = id_of(&write(
            root,
            spec,
            "criterion",
            json!({"when": "a onda roda", "then": "a suíte passa", "proof": "git --version", "form": "ubiquitous",
                "origin": said}),
        ));
        for (n, files, depends) in plan {
            let mut wave = json!({"n": n, "text": format!("Onda {n}."), "criteria": [crit],
                "done_when": "A suíte passa.", "origin": said});
            if !depends.is_empty() {
                wave["depends_on"] = json!(depends);
            }
            write(root, spec, "wave", wave);
            let declared: Vec<Value> = files.iter().map(|f| json!({"path": f})).collect();
            write(root, spec, "task", json!({"wave": n, "text": format!("Tarefa da onda {n}."),
                "files": declared, "depends_on": [], "origin": said}));
        }
        before(said);
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join(spec));
    }

    /// O mapa do projeto em `root`, como o scan o grava, com uma parte só, na
    /// raiz, do tipo `kind` (`cargo`, `npm`). Chame depois de [`approved`]:
    /// o mapa fica fora do commit, como no projeto de verdade.
    pub(super) fn mapped(root: &Path, kind: &str) {
        let model = json!({"projects": [{"name": "(root)", "dir": "", "kind": kind, "code_files": 1}]});
        std::fs::write(mustard_core::io::project_map::model_path(root), model.to_string()).unwrap();
    }

    /// O projeto em `root` com o submódulo `libs/sub`, clonado de um servidor
    /// em `servers` cuja base é `main`, com o arquivo `lib.txt` e quem comita
    /// configurado no submódulo. Chame antes de [`approved`].
    pub(super) fn with_submodule(root: &Path, servers: &Path) {
        let (server, seed) = (servers.join("sub.git"), servers.join("semente"));
        std::fs::create_dir_all(&seed).unwrap();
        std::fs::create_dir_all(root).unwrap();
        git_at(servers, &["init", "-q", "--bare", "-b", "main", "sub.git"]);
        git_at(&seed, &["init", "-q", "-b", "main"]);
        std::fs::write(seed.join("lib.txt"), "fn um() {}\n").unwrap();
        git_at(&seed, &["add", "-A"]);
        git_at(&seed, &["commit", "-q", "-m", "biblioteca"]);
        git_at(&seed, &["push", "-q", &server.to_string_lossy(), "main"]);
        git_at(root, &["init", "-q"]);
        git_at(root, &["config", "core.autocrlf", "false"]);
        git_at(root, &["config", "core.eol", "lf"]);
        let url = server.to_string_lossy().to_string();
        git_at(root, &["-c", "protocol.file.allow=always", "submodule", "add", "-q", &url, "libs/sub"]);
        git_at(root, &["commit", "-q", "-m", "submodulo"]);
        for (key, value) in [("user.email", "t@t"), ("user.name", "t"), ("commit.gpgsign", "false")] {
            git_at(&root.join("libs/sub"), &["config", key, value]);
        }
    }

    /// A saída do git em `dir`, sem as bordas.
    pub(super) fn git_text(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git").args(args).current_dir(dir).output().expect("git");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    pub(super) fn round(root: &Path, spec: &str, report: Option<&str>) -> Value {
        round_for(
            &RoundOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report: report.map(str::to_string) },
            None,
        )
    }

    /// [`round`] com quem relê o mapa depois do commit da rodada (`mine`),
    /// que um teste escolhe sem instalar a ferramenta do scan de verdade.
    pub(super) fn round_with_mine(
        root: &Path,
        spec: &str,
        report: Option<&str>,
        mine: &dyn Fn(
            &Path,
            &Path,
        ) -> mustard_core::platform::error::Result<mustard_core::domain::scan::ScanReport>,
    ) -> Value {
        let opts =
            RoundOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report: report.map(str::to_string) };
        let project = spec_events::project(&opts.root);
        match answer::run_round_with_mine(&opts, &project.root, project.lang, None, mine) {
            Ok(report) => report,
            Err(refusal) => refusal.to_value(project.lang),
        }
    }

    /// Uma linha do fim de um agente, como os textos dele ensinam.
    pub(super) fn line(tag: &str, body: Value) -> String {
        format!("<{tag}>{body}</{tag}>")
    }

    /// A linha `DELIVERED` da onda `wave`, com o resumo do commit. Cada
    /// arquivo entregue que existe ganha uma linha, para o commit ter o que
    /// levar.
    pub(super) fn delivered(root: &Path, wave: u64, text: &str, files: &[&str]) -> String {
        for file in files {
            let path = root.join(file);
            if let Ok(before) = std::fs::read_to_string(&path) {
                std::fs::write(&path, format!("{before}// {text}\n")).unwrap();
            }
        }
        line("DELIVERED", json!({"wave": wave, "text": text, "files": files, "commit": format!("a onda {wave} saiu")}))
    }

    /// A linha `VERDICT` da onda `wave`, com o critério pelo código. `final:
    /// true`, porque só o veredito final do agente de teste dedicado pode
    /// reprovar ou aprovar uma onda.
    pub(super) fn verdict(wave: u64, result: &str, text: &str) -> String {
        line("VERDICT", json!({"wave": wave, "result": result, "final": true, "text": text,
            "criteria": [{"criterion": "MSTD-CRIT-0001", "tests_rule": true}]}))
    }

    pub(super) fn delivered_count(root: &Path) -> usize {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        log.visible().iter().filter(|e| e.event_type == "delivered").count()
    }

    /// A linha do fim que o texto de um agente ensina, tirada do próprio
    /// texto, com os valores de exemplo trocados por `values`.
    pub(super) fn taught_line(template: &str, tag: &str, values: &[(&str, &str)]) -> String {
        let open = format!("<{tag}>");
        let found = template.lines().find(|l| l.starts_with(&open)).unwrap_or_else(|| panic!("no {tag} line"));
        values.iter().fold(found.to_string(), |line, (from, to)| line.replacen(from, to, 1))
    }

    /// O subject e o corpo do último commit.
    pub(super) fn last_commit(root: &Path) -> (String, String) {
        let out = Command::new("git").args(["log", "-1", "--format=%s%n%b"]).current_dir(root).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let (subject, body) = text.split_once('\n').unwrap_or((&text, ""));
        (subject.to_string(), body.trim().to_string())
    }

    /// As ondas de uma resposta da rodada, num campo dela.
    pub(super) fn waves_in(out: &Value, field: &str) -> Vec<u64> {
        out[field].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect()
    }

    /// A versão nova da tarefa da onda `n`: o plano da onda muda depois do
    /// pedido dela, com a mesma origem da versão anterior.
    pub(super) fn replan(root: &Path, n: u64) {
        replan_from(root, n, None);
    }

    /// [`replan`] com a origem `origin`, quando ela é dada: a mensagem ou a
    /// decisão de onde a versão nova nasce.
    pub(super) fn replan_from(root: &Path, n: u64, origin: Option<u64>) {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let task = log
            .visible()
            .into_iter()
            .find(|e| e.event_type == "task" && e.wave() == Some(n))
            .unwrap_or_else(|| panic!("sem tarefa da onda {n}"));
        let mut fields = task.fields.clone();
        for key in ["v", "id", "code", "at", "search", "type", "author"] {
            fields.remove(key);
        }
        let mut body = Value::Object(fields);
        body["replaces"] = json!(task.id);
        body["text"] = json!(format!("Tarefa da onda {n}, revista."));
        if let Some(origin) = origin {
            body["origin"] = json!(origin);
        }
        id_of(&write(root, "x", "task", body));
    }

    /// Um pedido da onda `n` gravado sem passar pela rodada.
    pub(super) fn seed_send(root: &Path, n: u64) {
        crate::shared::spec_state::seed_event(root, "x", "send", json!({"wave": n, "role": "wave",
            "text": "pedido", "lines": 1, "chars": 6, "items": [1], "mustard": "0", "author": "binary"}));
    }

    /// Nenhum arquivo da rodada passa do teto de linhas de código: a porta e
    /// cada parte da pasta dela, pela medida única do núcleo.
    #[test]
    fn no_file_of_the_round_goes_over_the_code_line_cap() {
        let gate = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("commands").join("flow").join("round.rs");
        assert_eq!(mustard_core::io::fs::files_over_code_line_cap(&gate), Ok(Vec::new()));
    }
}
