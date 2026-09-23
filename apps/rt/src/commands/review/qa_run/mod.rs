//! The criteria runner. The `qa-run` command is gone; what stays alive here is
//! [`run_proof`], which runs one criterion's proof, [`run_criteria_proofs`],
//! the loop that the close and the round both call to run a list of
//! criteria in order and stop at the first that does not pass,
//! [`run_command`], which runs a flow command that is not a proof,
//! [`run_server_command`], which the close calls for the lint and the whole
//! suite the server runs, and the section reader the page uses.
//!
//! As duas portas rodam o mesmo comando do mesmo jeito e se separam numa
//! leitura só: quantos testes a saída diz ter rodado. Ela vale na prova de um
//! critério, que promete rodar teste, e nunca no lint nem em outro comando do
//! fluxo.

use std::path::Path;

mod runner;

#[cfg(test)]
pub(crate) use runner::{ceiling_secs, with_timeout_variable, Ceiling};

/// One AC execution outcome.
pub(crate) struct AcResult {
    status: String,
    exit: Option<i64>,
    duration_ms: u128,
    stderr_excerpt: String,
    /// Quantos testes a saída do comando disse ter rodado, quando um executor
    /// que o projeto usa se reconheceu nela; `None` quando a saída não
    /// responde à pergunta ou quando o comando nem chegou a rodar. É leitura,
    /// e não veredito: quem julga esse número é a prova de um critério, em
    /// [`run_proof`], e nunca o lint nem outro comando do fluxo.
    tests_run: Option<u64>,
}

/// Uma prova de critério rodada uma vez, como o fechamento a grava.
pub(crate) struct ProofRun {
    /// `pass` quando a prova passou, `fail` em qualquer outro desfecho.
    pub result: &'static str,
    /// O código de saída; o que não chegou a rodar sai com o código de erro.
    pub exit: i64,
    /// Quanto demorou, em milissegundos.
    pub ms: u64,
    /// O começo do que a prova escreveu, quando ela não passou — a saída do
    /// executor, e não uma frase montada sobre ela.
    pub output: String,
    /// A prova de critério saiu verde sem rodar teste nenhum, e por isso não
    /// passou, com o número que a saída do executor disse — é ele que a
    /// recusa mostra. `None` quando a prova passou, quando ela falhou por
    /// outro motivo, e em todo comando que não é prova de critério.
    pub ran_no_test: Option<u64>,
}

/// Roda a prova de um critério uma vez, pelo mesmo executor do QA: o mesmo
/// shell, o mesmo teto de tempo e a mesma classificação. É por aqui que o
/// fechamento roda cada critério, para que as duas portas nunca discordem
/// sobre o que é uma prova que passou.
///
/// Só a prova de um critério promete rodar teste, e por isso só ela é lida
/// assim: verde sem rodar teste nenhum não passa, porque o nome do teste não
/// casou. O comando do fluxo que não é prova de critério — o lint do projeto
/// — roda por [`run_command`], que não faz essa leitura.
pub(crate) fn run_proof(command: &str, cwd: &Path) -> ProofRun {
    graded(runner::run_ac_command(command, None, cwd), true)
}

/// Roda um comando do fluxo que não é prova de critério — hoje, a
/// compilação que a rodada confere — pelo mesmo executor do QA, com o mesmo
/// shell e o mesmo teto de tempo.
///
/// A leitura de quantos testes o comando rodou não vale aqui: um lint verde
/// cuja saída cite "no tests" não é uma prova que deixou de provar, e quem
/// lesse assim recusaria um verde legítimo.
pub(crate) fn run_command(command: &str, cwd: &Path) -> ProofRun {
    graded(runner::run_ac_command(command, None, cwd), false)
}

/// Roda um dos dois comandos que o servidor roda — o `lintCommand` e o
/// `testCommand` do `mustard.json` —, como o fechamento os repete. Mesmo
/// executor e mesma leitura de [`run_command`], com um teto só deles, de uma
/// hora: a suíte inteira de um projeto não cabe no teto de uma prova de
/// critério, e a variável `MUSTARD_QA_AC_TIMEOUT_SECS` vale só para a prova.
pub(crate) fn run_server_command(command: &str, cwd: &Path) -> ProofRun {
    graded(runner::run_server_command(command, cwd), false)
}

/// A prova de um critério que não passou: o código dele, o comando inteiro
/// que tentou rodar e a saída de erro — o que a recusa do fechamento e da
/// rodada nomeiam.
pub(crate) struct FailedProof {
    pub code: String,
    pub command: String,
    pub output: String,
    /// A saída disse zero teste rodado, com o número que ela leu — só quando
    /// foi esse o motivo da falha.
    pub ran_no_test: Option<u64>,
}

/// Roda a prova de cada critério de `criteria` (id, código, comando), na
/// ordem em que a lista chega, uma de cada vez, e devolve a execução de cada
/// um junto do primeiro que não passou. É o mesmo laço que o fechamento roda
/// para os critérios da spec inteira, em `close.rs`, e que a rodada roda,
/// antes de comitar, só para os que as ondas do relatório cobrem: quem chama
/// decide o que grava com cada execução e como nomeia a recusa — aqui só se
/// roda e se lê o resultado.
pub(crate) fn run_criteria_proofs(
    root: &Path,
    criteria: &[(u64, String, String)],
) -> (Vec<(u64, String, ProofRun)>, Option<FailedProof>) {
    let mut runs = Vec::new();
    let mut failed = None;
    for (id, code, proof) in criteria {
        let out = run_proof(proof, root);
        if out.result != "pass" && failed.is_none() {
            failed = Some(FailedProof {
                code: code.clone(),
                command: proof.clone(),
                output: out.output.clone(),
                ran_no_test: out.ran_no_test,
            });
        }
        runs.push((*id, code.clone(), out));
    }
    (runs, failed)
}

/// Uma execução classificada como o fechamento a grava. As duas portas
/// entram aqui, e a diferença entre elas é um lugar só: `is_proof`, que diz
/// se o comando é a prova de um critério. Só nela o verde sem rodar teste
/// nenhum vira recusa, e a recusa carrega o número que a saída do executor
/// disse — não há recusa sem contagem lida.
///
/// O que a execução leva é sempre o que o comando escreveu, e nunca uma frase
/// montada aqui: quem lê o evento gravado precisa ver a saída do executor. O
/// número lido vai pelo `ran_no_test`, e é dele que a recusa tira a contagem
/// que mostra.
fn graded(out: AcResult, is_proof: bool) -> ProofRun {
    let ran_no_test = out.tests_run.filter(|count| *count == 0 && is_proof && out.status == "pass");
    ProofRun {
        result: if out.status == "pass" && ran_no_test.is_none() { "pass" } else { "fail" },
        exit: out.exit.unwrap_or(1),
        ms: u64::try_from(out.duration_ms).unwrap_or(u64::MAX),
        ran_no_test,
        output: out.stderr_excerpt,
    }
}

/// Extract the `## Acceptance Criteria` section body (heading line stripped),
/// recognizing the EN and PT headings via [`crate::commands::spec::spec_sections`].
///
/// `pub(crate)`: shared with `analyze_validation` so section detection and AC
/// parsing cannot drift from what qa-run actually executes.
pub(crate) fn extract_ac_section(markdown: &str) -> Option<String> {
    // Reuse the shared, i18n-aware section extractor so this QA reader and the
    // rewave producer (which carries this section verbatim into `wave-plan.md`)
    // parse the heading identically and cannot drift.
    let block =
        crate::commands::spec::spec_sections::section_block(markdown, "acceptanceCriteria")?;
    // Body only — drop the heading line itself.
    Some(block.split_once('\n').map_or("", |(_, body)| body).to_string())
}

/// Options for one qa-run, carried on the thread-local the executor reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct QaRunOptions {
    /// `true` when invoked from a process that **could be** the binary some AC
    /// commands rebuild — this very `mustard-rt`.
    ///
    /// Setting this flag lets the executor ask the PATH question before
    /// spawning: when the file a `cargo build|test` would write IS the file
    /// this process is executing from, a `--workspace` command gets
    /// `--exclude <package>` appended and a direct `-p` command is refused
    /// outright with a reason naming that file, instead of failing with
    /// `failed to remove file mustard-rt.exe` (Windows os error 5). When the
    /// two paths differ — the shipped shape, an installed binary against the
    /// workspace `target/` — nothing is rewritten and nothing is refused. See
    /// [`targets_running_binary`].
    ///
    /// `complete_spec::run_qa_fail_open` sets this. External callers
    /// (`mustard-rt run qa-run --spec X` from a CI shell) leave it `false`.
    pub self_invoked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PT heading "Critérios de Aceitação globais" (suffix word after the
    /// canonical name) must still resolve — `is_heading` matches with a
    /// word-boundary tolerance after the variant. Regression guard for
    /// language-agnostic parsing.
    #[test]
    fn extracts_ac_section_pt_heading_with_suffix() {
        let md = "# Spec\n\n## Critérios de Aceitação globais\n- [ ] AC-G1: x — Command: `true`\n\n## Files\n- a.rs\n";
        let section = extract_ac_section(md).unwrap();
        assert!(section.contains("AC-G1"));
        assert!(!section.contains("Files"));
    }

    #[test]
    fn extracts_ac_section_body() {
        let md = "# Spec\n\n## Acceptance Criteria\n- [ ] AC-1: x — Command: `true`\n\n## Files\n- a.rs\n";
        let section = extract_ac_section(md).unwrap();
        assert!(section.contains("AC-1"));
        assert!(!section.contains("Files"));
    }

    /// A peça que roda a prova de um critério — [`run_proof`], a mesma que o
    /// fechamento e a rodada chamam — não se contenta com o código de saída:
    /// um comando real, de um executor real, cujo filtro não casa teste
    /// nenhum, sai verde e ainda assim não passa, porque a leitura da saída
    /// diz zero teste rodado. É a peça, e não a conversa entre close.rs e
    /// runner.rs, que promete essa leitura.
    #[test]
    fn a_verificacao_que_nao_roda_teste_nenhum_e_recusada() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"prova\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            "#[cfg(test)]\nmod tests {\n    #[test]\n    fn soma() { assert_eq!(1 + 1, 2); }\n}\n",
        )
        .unwrap();

        // Sanidade: o mesmo comando, com o nome certo, roda e passa — a
        // recusa abaixo é da leitura de zero testes, não de outro motivo.
        let matching = run_proof("cargo test --lib -- tests::soma --exact", root);
        assert_eq!(matching.result, "pass", "a prova com o nome certo passa");
        assert_eq!(matching.ran_no_test, None);

        let out = run_proof("cargo test --lib -- nome_que_nao_existe_em_lugar_nenhum", root);
        assert_eq!(out.result, "fail", "verde sem rodar teste não é prova aprovada");
        assert_eq!(out.ran_no_test, Some(0), "a recusa carrega o número que a saída disse");
    }
}
