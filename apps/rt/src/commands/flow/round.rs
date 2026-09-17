//! `mustard-rt run round [--spec <nome>]` — uma rodada de ondas.
//!
//! É a porta única da execução, e cada rodada é uma chamada só. Sem relatório,
//! a rodada despacha: escolhe as ondas que podem sair juntas, monta o pedido
//! de cada uma, grava o envio com o pedido exato como foi injetado e marca a
//! spec como em execução na primeira rodada. Com o relatório da rodada
//! anterior (`--report`), ela primeiro fecha o que voltou e só então despacha
//! a rodada seguinte.
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
//! nenhuma das duas linhas, ou com uma linha sem campo obrigatório; um
//! `entregou` acima do teto de caracteres; um arquivo entregue que está
//! reservado para outra onda em andamento, ou que não está no disco nem no
//! git; uma mensagem de commit fora do
//! modelo (título e corpo acima do teto, link do claude.ai, o nome do modelo,
//! assinatura de coautoria ou e-mail de alguém); o relatório em que um agente
//! diz que o plano da onda não funciona, que para a rodada e só segue com o
//! "sim" do usuário. O "sim" da mudança de plano é o clique em "Aceitar" na
//! pergunta dela, gravado pela testemunha como na aprovação da spec, e nunca a
//! leitura que o modelo faz de uma frase: a rodada não aceita código nenhum de
//! quem a chama.
//!
//! **O que para sem travar.** A onda reprovada depois da segunda rodada de
//! conserto segura só ela e as ondas que dependem dela: o resto da rodada
//! segue, e a resposta traz a pergunta ao usuário com os vereditos dela. A
//! onda que sai do plano deixa de contar, na rodada e no fechamento.
//!
//! **O que avisa.** O formatador que o projeto declara e que não foi achado
//! sai pelo nome, em vez de a formatação ser pulada em silêncio; e a prova
//! nova que sai verde sem rodar teste nenhum sai pelo código do critério.
//!
//! A página da spec e a do projeto são refeitas no fim da rodada, e a resposta
//! manda publicá-las: a rodada é um dos marcos de publicação. Nenhum endereço
//! é impresso na conversa.

mod answer;
mod commit;
mod queue;
mod report;
mod stops;

use std::path::PathBuf;

use serde_json::Value;

use crate::commands::spec_events;
use crate::shared::spec_state::session_from_env;

pub(crate) use answer::RoundRefusal;
pub(crate) use queue::{wave_states, waves_in_progress};
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
    use std::path::{Path, PathBuf};
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
        git_at(root, &["add", "-A"]);
        git_at(root, &["commit", "-q", "-m", "semente"]);
        git_at(root, &["config", "user.email", "t@t"]);
        git_at(root, &["config", "user.name", "t"]);
        git_at(root, &["config", "commit.gpgsign", "false"]);

        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        let said = id_of(&write(root, spec, "message", json!({"author": "user", "text": "o objetivo"})));
        let crit = id_of(&write(
            root,
            spec,
            "criterion",
            json!({"when": "a onda roda", "then": "a suíte passa", "proof": "cargo test", "origin": said}),
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
                "files": declared, "origin": said}));
        }
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join(spec));
    }

    pub(super) fn round(root: &Path, spec: &str, report: Option<&str>) -> Value {
        round_for(
            &RoundOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report: report.map(str::to_string) },
            None,
        )
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

    /// A linha `VERDICT` da onda `wave`, com o critério pelo código.
    pub(super) fn verdict(wave: u64, result: &str, text: &str) -> String {
        line("VERDICT", json!({"wave": wave, "result": result, "text": text,
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
    /// pedido dela.
    pub(super) fn replan(root: &Path, n: u64) {
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
        id_of(&write(root, "x", "task", body));
    }

    /// Um pedido da onda `n` gravado sem passar pela rodada.
    pub(super) fn seed_send(root: &Path, n: u64) {
        crate::shared::spec_state::seed_event(root, "x", "send", json!({"wave": n, "role": "wave",
            "text": "pedido", "lines": 1, "chars": 6, "items": [1], "mustard": "0", "author": "binary"}));
    }

    /// As linhas de código de um arquivo: as que não são vazias nem
    /// comentário, antes do módulo de testes dele.
    fn code_lines(source: &str) -> usize {
        let lines: Vec<&str> = source.lines().map(str::trim).collect();
        let end = lines
            .windows(2)
            .position(|pair| pair[0] == "#[cfg(test)]" && pair[1] == "mod tests {")
            .unwrap_or(lines.len());
        lines[..end].iter().filter(|line| !line.is_empty() && !line.starts_with("//")).count()
    }

    /// Nenhum arquivo da rodada passa de 800 linhas de código: a porta e cada
    /// parte da pasta dela, contadas sem as linhas vazias, os comentários e o
    /// módulo de testes.
    #[test]
    fn no_file_of_the_round_goes_over_the_code_line_cap() {
        const CAP: usize = 800;
        let flow = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("commands").join("flow");
        let mut parts: Vec<PathBuf> = std::fs::read_dir(flow.join("round"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
            .collect();
        parts.sort();
        let files: Vec<PathBuf> = std::iter::once(flow.join("round.rs")).chain(parts).collect();
        let measured: Vec<(String, usize)> = files
            .iter()
            .map(|path| (path.display().to_string(), code_lines(&std::fs::read_to_string(path).unwrap())))
            .collect();
        let over: Vec<&(String, usize)> = measured.iter().filter(|(_, lines)| *lines > CAP).collect();
        assert!(over.is_empty(), "passam de {CAP} linhas de código: {over:?}");
    }
}
