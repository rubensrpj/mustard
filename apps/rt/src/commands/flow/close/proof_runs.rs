//! A prova de cada critério, e a de cada pedido que uma tarefa entregue
//! cobre, no código de antes da obra. A máquina do
//! fechamento roda a prova na cópia limpa, no commit da obra, onde ela tem de
//! passar rodando pelo menos um teste; e roda de novo numa segunda cópia, no
//! commit base da obra, com os arquivos de teste que a obra mudou por cima
//! (os que [`is_test_path`] dá como teste), onde ela tem de cair. Assim o
//! teste que mora em arquivo próprio roda contra o código antigo, e a prova
//! que passa ali, rodando teste, não prova nada novo: é recusada.
//!
//! Cair, no código de antes, é qualquer desfecho que não seja passar: a falha
//! do teste, a falha de compilação do código antigo e a prova que não acha
//! teste nenhum, porque o teste ainda não existia.
//!
//! **Limite.** Onde o teste mora no mesmo arquivo do código, como no Rust, o
//! arquivo não é de teste, e o código de antes não tem o teste novo: a prova
//! cai por achar zero teste, sem ter rodado contra o código antigo.
//!
//! A cópia do código de antes é uma vaga da obra, com nome que nenhuma onda
//! usa, e guarda a compilação e as dependências entre um fechamento e o
//! seguinte, como a cópia limpa. Os arquivos de teste entram nela só durante
//! as provas e saem logo depois: a vaga fica sempre no commit base, e nada
//! dela é guardado como código de alguém quando a obra fecha.

use std::path::{Path, PathBuf};

use mustard_core::domain::ast::is_test_path;
use mustard_core::domain::spec_events::SpecLog;
use mustard_core::io::wave_prompt::{shown, spec_copies_dir};
use mustard_core::platform::git;

use super::CloseRefusal;
use crate::commands::git_settle::CopiesLock;
use crate::commands::review::qa_run::{ProofRun, run_criteria_proofs};

/// O nome da pasta da cópia do código de antes dentro da pasta das cópias da
/// obra, ao lado da cópia limpa (`check`).
const FOLDER: &str = "check-base";

/// A cópia do código de antes, pronta: a pasta, o commit base em que ela
/// está e o commit da obra de onde vêm os arquivos de teste.
pub(super) struct BeforeCopy {
    path: PathBuf,
    base: String,
    head: String,
}

/// Prepara a cópia do código de antes da obra `spec`, com a trava das cópias
/// presa por quem chama (`_copies`): a vaga no commit base da obra
/// ([`super::super::validation::base_commit`]), sem arquivo local, e as
/// dependências declaradas instaladas como na cópia limpa. `None` quando a
/// obra não tem commit base que o git ache: não há código de antes onde
/// rodar a prova.
pub(super) fn prepare(root: &Path, spec: &str, log: &SpecLog, _copies: &CopiesLock) -> Result<Option<BeforeCopy>, CloseRefusal> {
    let Some(base) = super::super::validation::base_commit(root, log) else {
        return Ok(None);
    };
    let path = spec_copies_dir(root, spec).join(FOLDER);
    let failed = |detail: String| CloseRefusal::CleanCopyFailed { copy: shown(&path), detail };
    let head = git::run(root, &["rev-parse", "HEAD"]).result().map_err(&failed)?;
    // Os arquivos de teste que um fechamento interrompido deixou na vaga não
    // são código de ninguém: saem antes de a vaga voltar ao commit base, que
    // guardaria o que ela tem e o commit não tem.
    if path.join(".git").is_file() {
        back_to_base(&path, &base).map_err(&failed)?;
    }
    let owner = crate::commands::flow::round::slot_owner(spec, log, &path);
    crate::commands::flow::round::clean_copy(root, &path, &base, &owner).map_err(&failed)?;
    crate::commands::flow::round::prepare_copy_dependencies(root, &path).map_err(&failed)?;
    Ok(Some(BeforeCopy { path, base, head }))
}

impl BeforeCopy {
    /// Roda cada prova de `proofs` (id, código do item, comando) no código de
    /// antes, com os arquivos de teste da obra por cima, e devolve a execução
    /// de cada uma, na ordem de `proofs`. O arquivo de teste que não pôde
    /// entrar faz a execução de todas cair com o motivo, e a vaga volta ao
    /// commit base no fim, em todo caso.
    pub(super) fn run(&self, proofs: &[(u64, String, String)]) -> Vec<(u64, String, ProofRun)> {
        let runs = match self.lay_work_tests() {
            Ok(()) => run_criteria_proofs(&self.path, proofs),
            Err(detail) => proofs.iter().map(|(id, code, _)| (*id, code.clone(), not_laid(&detail))).collect(),
        };
        if let Err(detail) = back_to_base(&self.path, &self.base) {
            eprintln!("[Mustard] {}: {detail}", shown(&self.path));
        }
        runs
    }

    /// Põe na vaga os arquivos de teste que a obra mudou do commit base ao
    /// commit da obra, como o commit da obra os tem; o que a obra apagou sai
    /// da vaga.
    fn lay_work_tests(&self) -> Result<(), String> {
        let changed = |filter: &str| -> Result<Vec<String>, String> {
            let range = [self.base.as_str(), self.head.as_str()];
            let args = ["diff", "--name-only", "-z", "--no-renames", filter, range[0], range[1]];
            let listed = git::run(&self.path, &args).result()?;
            Ok(listed.split('\0').filter(|file| !file.is_empty() && is_test_path(file)).map(str::to_string).collect())
        };
        let present = changed("--diff-filter=d")?;
        if !present.is_empty() {
            let mut args = vec!["checkout", self.head.as_str(), "--"];
            args.extend(present.iter().map(String::as_str));
            git::run(&self.path, &args).result()?;
        }
        for file in changed("--diff-filter=D")? {
            match std::fs::remove_file(self.path.join(&file)) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(format!("{file}: {err}")),
                _ => {}
            }
        }
        Ok(())
    }
}

/// A prova que passou no código de antes da obra rodando pelo menos um teste,
/// com o número de testes que a saída disse; `None` quando ela caiu — falhou,
/// não compilou, não achou teste — ou quando a saída não diz quantos testes
/// rodaram, e não há como afirmar que um teste passou.
pub(super) fn passed_before(out: &ProofRun) -> Option<u64> {
    out.tests.filter(|count| out.result == "pass" && *count > 0)
}

/// A execução de uma prova que não rodou porque os arquivos de teste da obra
/// não entraram na vaga, com o motivo: conta como queda, como a prova que não
/// acha o teste.
fn not_laid(detail: &str) -> ProofRun {
    ProofRun {
        reused: false,
        result: "fail",
        exit: 1,
        ms: 0,
        output: detail.to_string(),
        ran_no_test: None,
        missing_test: None,
        tests: None,
        evidence: String::new(),
    }
}

/// Volta a vaga `path` ao commit `base`, sem arquivo novo nem mudança, e com
/// o que o git ignora — a compilação, as dependências — no lugar.
fn back_to_base(path: &Path, base: &str) -> Result<(), String> {
    git::run(path, &["reset", "-q", "--hard", base]).result()?;
    git::run(path, &["clean", "-q", "-fd"]).result().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::super::tests::{close, close_for, commit_file, git_at, ready_with_files, ready_with_waves, returned, round_for, verdict_written, wave_file};
    use crate::commands::flow::close::CloseOpts;
    use crate::commands::flow::round::RoundOpts;
    use mustard_core::io::spec_events as store;
    use mustard_core::platform::i18n::{Locale, translate};
    use serde_json::json;
    use std::path::Path;

    /// O commit em que o repositório de `root` nasceu.
    fn first_commit(root: &Path) -> String {
        let out = std::process::Command::new("git").args(["rev-list", "--max-parents=0", "HEAD"]).current_dir(root).output().expect("git");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A obra muda `src/w1.rs`, que passa a chamar `dois`, e traz um arquivo
    /// de teste próprio; a base da spec é a branch `dev`, no commit em que o
    /// repositório nasceu. O fechamento roda cada prova no commit base, com o
    /// arquivo de teste da obra por cima, e no commit da obra. Passa só a
    /// prova que cai antes e passa depois rodando teste. São recusadas a que
    /// passa antes rodando teste — também a do arquivo de teste da obra, que
    /// roda contra o código antigo —, a que cai depois e a que não roda teste
    /// nenhum depois.
    #[test]
    fn a_proof_green_before_the_change_or_red_after_it_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let falls_before = "grep -q dois src/w1.rs && echo Tests: 1 total";
        let green_before = "echo Tests: 2 total";
        let work_test = "sh tests/old_code.sh";
        let red_after = "grep -q tres src/w1.rs && echo Tests: 1 total";
        let no_test_after = "grep -q dois src/w1.rs && echo Tests: 0 total";
        ready_with_waves(root, "x", &[falls_before, green_before, work_test, red_after, no_test_after], 1);
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(root.join("tests/old_code.sh"), "echo Tests: 3 total\n").unwrap();
        commit_file(root, "tests/old_code.sh");
        git_at(root, &["branch", "dev", &first_commit(root)]);

        let refused = close(root, "x");
        assert_eq!(refused["ok"], json!(false), "{refused}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let criteria: Vec<String> =
            log.visible().into_iter().filter(|e| e.event_type == "criterion").map(|e| codes[&e.id].clone()).collect();
        let passed_before = |code: &str, command: &str, count: &str| {
            translate("close.criterion_passed_before", Locale::PtBr).replace("{code}", code).replace("{command}", command).replace("{count}", count)
        };
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(&passed_before(&criteria[1], green_before, "2")), "a prova verde antes é recusada: {hint}");
        assert!(hint.contains(&passed_before(&criteria[2], work_test, "3")), "o teste da obra roda contra o código antigo: {hint}");
        let reasons: Vec<&str> = refused["reasons"].as_array().into_iter().flatten().filter_map(|r| r.as_str()).collect();
        assert_eq!(
            reasons,
            ["criterion-failed", "criterion-ran-no-test", "criterion-passed-before", "criterion-passed-before"],
            "a que cai depois e a que não roda teste depois também: {refused}"
        );
        assert!(!hint.contains(&format!("{} ", criteria[0])), "a prova que cai antes e passa depois passa: {hint}");
    }

    /// Cada achado vira um pedido com a prova no relato, e a tarefa da obra
    /// o cobre. Entregue a tarefa, o fechamento roda a prova de cada pedido
    /// nas mesmas duas cópias das provas dos critérios: a que já passa no
    /// commit base rodando teste é recusada pelo código do pedido, com o texto
    /// do pedido, e a que cai antes e passa depois passa.
    #[test]
    fn a_finding_proof_green_before_the_change_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let green_before = "echo Tests: 2 total";
        let falls_before = "grep -q dois src/w1.rs && echo Tests: 1 total";
        let findings = [
            format!("- A soma de `src/w1.rs` ignora o estouro. Prova: `{green_before}`"),
            format!("- A soma de `src/w1.rs` não chama dois. Prova: `{falls_before}`"),
        ];
        let findings: Vec<&str> = findings.iter().map(String::as_str).collect();
        ready_with_files(root, "x", &[], &[wave_file(1)], &[], &findings);
        git_at(root, &["branch", "dev", &first_commit(root)]);

        let refused = close(root, "x");
        assert_eq!(refused["ok"], json!(false), "{refused}");
        assert_eq!(refused["reason"], json!("criterion-passed-before"), "só a prova verde antes recusa: {refused}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let requests: Vec<String> = log.visible().into_iter().filter(|e| e.event_type == "request").map(|e| codes[&e.id].clone()).collect();
        let passed_before = translate("close.request_proof_passed_before", Locale::PtBr)
            .replace("{code}", &requests[0])
            .replace("{command}", green_before)
            .replace("{count}", "2");
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(&passed_before), "a prova do achado verde antes é recusada: {hint}");
        assert!(!hint.contains(&format!("{} ", requests[1])), "a prova do achado que cai antes e passa depois passa: {hint}");
    }

    /// A recusa da prova de um achado reabre a onda da tarefa que cobre o
    /// pedido dele, e só ela: a outra onda da obra, que não cobre o pedido,
    /// fica como está. O texto que a onda reaberta recebe é o da recusa do
    /// pedido.
    #[test]
    fn a_refused_finding_proof_reopens_the_wave_whose_task_covers_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let finding = "- A soma de `src/w1.rs` ignora o estouro. Prova: `echo Tests: 2 total`";
        ready_with_files(root, "x", &[], &[wave_file(1), wave_file(2)], &[], &[finding]);
        git_at(root, &["branch", "dev", &first_commit(root)]);

        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("criterion-passed-before"), "{refused}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert_eq!(log.last_rejected().keys().copied().collect::<Vec<_>>(), vec![1], "só a onda que cobre o pedido reabre: {refused}");
        let codes = log.codes();
        let request = log.visible().into_iter().find(|e| e.event_type == "request").map(|e| codes[&e.id].clone()).expect("the request");
        let failure = log.events.iter().rev().find(|e| e.event_type == "validation_failure" && e.wave() == Some(1)).expect("the reopening");
        let said = translate("close.request_proof_passed_before", Locale::PtBr).replace("{code}", &request);
        let said = said.split("{command}").next().unwrap_or_default();
        assert!(failure.str_field("text").unwrap_or_default().contains(said), "a onda reaberta lê a recusa do pedido: {:?}", failure.fields);
    }

    /// A obra fecha verde e grava o recibo da validação. A revisão final
    /// aprova com um achado cuja prova já passa no commit base, e a tarefa
    /// dele volta sem mudar arquivo nenhum. O fechamento seguinte não
    /// reaproveita o recibo: a prova do achado entrou na lista das provas,
    /// roda nas duas cópias e recusa.
    #[test]
    fn a_finding_delivered_without_change_runs_its_proof_instead_of_reusing_the_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 1);
        git_at(root, &["branch", "dev", &first_commit(root)]);
        let opts = CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() };
        let asked = close_for(&opts, None);
        assert_eq!((asked["review"]["final"].clone(), asked["validation"]["reused"].clone()), (json!(true), json!(false)), "{asked}");

        let finding = json!({"title": "A soma não trata estouro", "detail": "A soma estoura em src/w1.rs. Prova: `echo Tests: 2 total`",
            "files": [wave_file(1)], "rule": "MSTD-CRIT-0001", "places": [wave_file(1)]});
        verdict_written(root, "x", json!({"final": true, "result": "approved", "text": "Pronta, com um achado.", "leftovers": [finding]}));
        let recorded = close_for(&opts, None);
        assert_eq!(recorded["phase"], json!("running"), "{recorded}");
        let round = || round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        let dispatched = round();
        assert_eq!(dispatched["ok"], json!(true), "{dispatched}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let wave = log.planned_waves().into_iter().max().expect("the wave of the finding");
        assert!(wave > 1, "o achado saiu numa onda própria: {dispatched}");
        returned(
            root,
            "x",
            json!({"wave": wave, "text": "Nada a mudar: a soma já estava assim. Rodei a prova e passou.",
                "checked_places": [{"place": wave_file(1), "reason": "A soma já trata o estouro."}]}),
        );
        let back = round();
        assert_eq!(back["ok"], json!(true), "{back}");
        assert!(back.get("commit").is_none(), "a tarefa do achado voltou sem mudança: {back}");

        let refused = close_for(&opts, None);
        assert_eq!(refused["reason"], json!("criterion-passed-before"), "a prova do achado rodou e recusou: {refused}");
    }
}
