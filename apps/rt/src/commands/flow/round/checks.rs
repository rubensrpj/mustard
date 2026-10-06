//! Os comandos que o projeto declara e que a rodada roda no repositório
//! principal antes de cada commit: a compilação, o lint e a suíte inteira.
//! Quem é dono da suíte verde é a rodada — não o agente da onda, que roda só
//! os testes do que mudou, nem quem conduz, que não roda teste nem lint por
//! conta própria. O primeiro que cai recusa com o comando e o fim da saída, e
//! nada é comitado; o que o projeto não declara não roda.

use std::path::Path;

use mustard_core::platform::i18n::{translate, Locale};

use super::answer::RoundRefusal;
use crate::commands::review::qa_run::{run_command, run_server_command, ProofRun};

/// Qual dos comandos que o projeto declara caiu antes do commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Check {
    /// A compilação (`buildCommand`).
    Build,
    /// O lint (`lintCommand`).
    Lint,
    /// A suíte inteira (`testCommand`).
    Suite,
}

impl Check {
    /// O motivo da recusa na resposta da rodada.
    pub(super) fn reason(self) -> &'static str {
        match self {
            Self::Build => "round-build-failed",
            Self::Lint => "round-lint-failed",
            Self::Suite => "round-tests-failed",
        }
    }

    /// A frase da recusa no catálogo, com as vagas do comando e do fim da
    /// saída.
    pub(super) fn message_key(self) -> &'static str {
        match self {
            Self::Build => "round.build_failed",
            Self::Lint => "round.lint_failed",
            Self::Suite => "round.tests_failed",
        }
    }
}

/// Quem roda um dos comandos declarados, na raiz, pelo executor do QA.
type Runner = fn(&str, &Path) -> ProofRun;

/// Roda no repositório principal, antes do commit, a compilação, o lint e a
/// suíte inteira que o projeto declara, nessa ordem — a compilação primeiro,
/// porque sem ela os outros dois nem rodam, e o lint e a suíte na ordem do
/// fechamento. O primeiro que cai recusa com o comando e o fim da saída, e a
/// rodada não comita nada. O que o projeto não declara não roda, porque não há
/// como rodá-lo sem saber o comando.
///
/// A compilação roda com o teto de uma prova que compila. O lint e a suíte
/// rodam com o teto dos comandos do servidor, de uma hora: a suíte inteira
/// leva o tempo que o projeto pede, e o teto de uma prova a cortaria no meio.
pub(super) fn ensure_checks_pass(root: &Path) -> Result<(), RoundRefusal> {
    let declared = mustard_core::ProjectConfig::load(root).commands();
    let checks: [(Check, Option<String>, Runner); 3] = [
        (Check::Build, declared.build, run_command),
        (Check::Lint, declared.lint, run_server_command),
        (Check::Suite, declared.test, run_server_command),
    ];
    for (check, command, run) in checks {
        let Some(command) = command else { continue };
        let out = run(&command, root);
        if out.result != "pass" {
            return Err(RoundRefusal::CheckFailed { check, command, output: out.output });
        }
    }
    Ok(())
}

/// O que fazer quando os agentes voltarem. No projeto que declara o lint ou a
/// suíte, a frase diz também que é a rodada quem os roda antes do commit —
/// quem conduz não os roda por conta própria — e manda rodá-la em segundo
/// plano, porque a suíte inteira pode passar do tempo que o terminal espera.
pub(super) fn report_back(root: &Path, lang: Locale) -> String {
    let declared = mustard_core::ProjectConfig::load(root).commands();
    let back = translate("round.report", lang);
    if declared.test.is_none() && declared.lint.is_none() {
        return back.to_string();
    }
    format!("{back} {}", translate("round.report.checks", lang))
}
