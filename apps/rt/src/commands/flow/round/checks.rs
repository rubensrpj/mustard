//! The integrated delivery runs the declared build. Lint and the full suite
//! belong to final validation; targeted criterion proofs remain per delivery.

use std::path::Path;

use mustard_core::platform::i18n::{Locale, translate};

use super::answer::RoundRefusal;
use super::report::WaveReport;
use super::stops::{fix_limit_refusal, split_at_fix_limit};
use crate::commands::review::qa_run::run_build_command;

/// Qual dos comandos que o projeto declara caiu antes do commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Check {
    /// A compilação (`buildCommand`).
    Build,
}

impl Check {
    /// O motivo da recusa na resposta da rodada.
    pub(super) fn reason(self) -> &'static str {
        match self {
            Self::Build => "round-build-failed",
        }
    }

    /// A frase da recusa no catálogo, com as vagas do comando e do fim da
    /// saída.
    pub(super) fn message_key(self) -> &'static str {
        match self {
            Self::Build => "round.build_failed",
        }
    }
}

/// Um comando declarado que caiu antes do commit, e as ondas a quem o
/// conserto volta.
pub(crate) struct CheckFailure {
    pub check: Check,
    pub command: String,
    /// O fim da saída do comando.
    pub output: String,
    /// As ondas da rodada a quem o conserto volta, na ordem do relatório: as
    /// que mudaram um arquivo que a saída cita, ou, quando ela não cita
    /// nenhum, todas as que mudaram arquivo.
    pub waves: Vec<u64>,
    /// A saída cita arquivo de alguma onda: `waves` são só essas.
    pub cited: bool,
    /// As ondas de `waves` que esperam um agente novo, cada uma com o título
    /// que o despacha: a rodada as preenche depois de gravar os trechos.
    pub new_agents: Vec<(u64, String)>,
}

impl CheckFailure {
    /// A recusa a quem conduz: o que caiu, com o fim da saída, e o próximo
    /// passo — mandar cada onda abaixo de volta ao agente dela, ou despachar
    /// um agente novo para a que perdeu o dela. Sem onda a quem o conserto
    /// volta, só o que caiu.
    pub(super) fn message(&self, lang: Locale) -> String {
        let failed = translate(self.check.message_key(), lang).replace("{command}", &self.command).replace("{output}", &self.output);
        let line = if self.cited { "round_checks.cited" } else { "round_checks.joined" };
        let mut text = failed;
        if !self.waves.is_empty() {
            text.push_str("\n\n");
            text.push_str(translate("round_checks.next", lang));
        }
        for wave in &self.waves {
            text.push_str("\n- ");
            text.push_str(&translate(line, lang).replace("{wave}", &wave.to_string()));
        }
        for (wave, title) in &self.new_agents {
            let new_agent = translate("round.after_wave.new_agent", lang).replace("{wave}", &wave.to_string()).replace("{title}", title);
            text.push_str("\n\n");
            text.push_str(&new_agent);
        }
        text
    }

    /// O trecho de conserto que volta ao agente da onda `wave`: o comando que
    /// caiu e o fim da saída, com o que fazer.
    pub(super) fn fix(&self, wave: u64, lang: Locale) -> String {
        translate("round_checks.fix", lang).replace("{wave}", &wave.to_string()).replace("{command}", &self.command).replace("{output}", &self.output)
    }
}

/// Run the declared build once over the integrated delivery, preserving
/// attribution and the existing repair limit on failure.
pub(super) fn ensure_checks_pass(root: &Path, spec: &str, waves: &[WaveReport], lang: Locale) -> Result<(), RoundRefusal> {
    let Some(command) = mustard_core::ProjectConfig::load(root).commands().build else {
        return Ok(());
    };
    let out = run_build_command(&command, root);
    super::super::validation::record_run(root, spec, "delivery", "build", &command, &out).map_err(RoundRefusal::Refused)?;
    if out.result != "pass" {
        let (culprit_waves, cited) = culprits(&out.output, waves);
        let failure = CheckFailure { check: Check::Build, command, output: out.output, waves: culprit_waves, cited, new_agents: Vec::new() };
        return Err(capped(failure, waves, lang));
    }
    Ok(())
}

/// A recusa do comando que caiu, com o teto de rodadas de conserto que a
/// conferência depois da onda também tem ([`split_at_fix_limit`] em
/// `reports`). Nenhuma onda a quem o conserto volta passou por todas: a
/// recusa sai como veio. Alguma passou: a rodada para e faz ao usuário a
/// mesma pergunta da conferência depois da onda, com o que caiu e o fim da
/// saída ([`fix_limit_refusal`]); a onda no teto vai ao usuário, e não ao
/// agente, e só as outras ganham o trecho de conserto.
fn capped(failure: CheckFailure, reports: &[WaveReport], lang: Locale) -> RoundRefusal {
    let (stuck, back) = split_at_fix_limit(&failure.waves, reports);
    if stuck.is_empty() {
        return RoundRefusal::CheckFailed(Box::new(failure));
    }
    let rest = CheckFailure { waves: back, ..failure };
    let fixes = rest.waves.iter().map(|wave| (*wave, rest.fix(*wave, lang))).collect();
    fix_limit_refusal(&stuck, &rest.message(lang), fixes, lang)
}

/// As ondas de `waves` a quem volta o conserto do comando que caiu com a
/// saída `output`: as que mudaram um arquivo que a saída cita pelo caminho,
/// como o compilador e o executor de testes citam o arquivo da falha, e
/// `true`. Sem nenhuma citada, todas as que mudaram arquivo, e `false`: a
/// rodada juntou as ondas antes de rodar, e não sabe dizer qual causou.
fn culprits(output: &str, waves: &[WaveReport]) -> (Vec<u64>, bool) {
    let changed: Vec<&WaveReport> = waves.iter().filter(|wave| !wave.files.is_empty()).collect();
    let cited: Vec<u64> = changed.iter().filter(|wave| wave.files.iter().any(|file| output.contains(file.as_str()))).map(|wave| wave.wave).collect();
    if cited.is_empty() {
        return (changed.iter().map(|wave| wave.wave).collect(), false);
    }
    (cited, true)
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mustard_core::io::spec_events as store;
    use mustard_core::platform::i18n::{Locale, translate};
    use serde_json::json;
    use tempfile::tempdir;

    use crate::commands::flow::round::fix_file;
    use crate::commands::flow::round::tests::*;

    /// O trecho de conserto gravado para a volta pendente da onda `wave`, ou
    /// nada quando a rodada não gravou nenhum para ela.
    fn fix_kept(root: &Path, wave: u64) -> Option<String> {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        fix_file(root, "x", &log, wave).and_then(|file| std::fs::read_to_string(file).ok())
    }

    /// A onda `wave` volta com o arquivo `file` mudado na cópia dela.
    fn back_with(root: &Path, wave: u64, file: &str) {
        let copy = mustard_core::io::wave_prompt::slot_path(root, "x", usize::try_from(wave).unwrap() - 1);
        std::fs::write(copy.join(file), format!("fn one() {{}}\n// onda {wave}\n")).unwrap();
        let body = json!({"wave": wave, "text": "Saiu.", "files": [file], "commit": format!("a onda {wave} sai")});
        assert_eq!(returned(root, body)["ok"], json!(true));
    }

    /// Duas ondas na mesma rodada e a suíte vermelha. A saída que cita o
    /// arquivo de uma delas manda o conserto só a ela: a recusa diz a quem
    /// conduz que a mande de volta ao agente, e o trecho com o comando e o fim
    /// da saída fica gravado para a volta dela, e não para a outra. A saída
    /// que não cita arquivo de onda nenhuma manda o conserto às duas.
    #[test]
    fn a_red_build_goes_back_to_the_wave_whose_file_the_output_cites_or_to_every_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        let cites = "echo 'thread panicked at src/b.rs:2:5'; exit 1";
        let config = |test: &str| json!({"maxCompilingWaves": 2, "buildCommand": test}).to_string();
        std::fs::write(root.join("mustard.json"), config(cites)).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);
        back_with(root, 1, "src/a.rs");
        back_with(root, 2, "src/b.rs");
        let line = |key: &str, wave: &str| format!("\n- {}", translate(key, Locale::PtBr).replace("{wave}", wave));
        let fix_head = |wave: &str, command: &str| {
            let fix = translate("round_checks.fix", Locale::PtBr).replace("{wave}", wave).replace("{command}", command);
            fix.split_once("{output}").map(|(head, _)| head.to_string()).unwrap()
        };

        let cited = round(root, "x", None);
        assert_eq!(cited["reason"], json!("round-build-failed"), "{cited}");
        let hint = cited["hint"].as_str().unwrap_or_default();
        let next = translate("round_checks.next", Locale::PtBr);
        assert!(hint.ends_with(&format!("\n\n{next}{}", line("round_checks.cited", "2"))), "{hint}");
        let fix = fix_kept(root, 2).unwrap_or_else(|| panic!("the fix of wave 2 is kept: {cited}"));
        assert!(fix.starts_with(&fix_head("2", cites)) && fix.contains("src/b.rs:2:5"), "{fix}");
        assert_eq!(fix_kept(root, 1), None, "the wave the output does not cite gets no fix: {cited}");

        let silent = "echo 'o teste soma caiu'; exit 1";
        std::fs::write(root.join("mustard.json"), config(silent)).unwrap();
        let joined = round(root, "x", None);
        assert_eq!(joined["reason"], json!("round-build-failed"), "{joined}");
        let hint = joined["hint"].as_str().unwrap_or_default();
        let both = format!("\n\n{next}{}{}", line("round_checks.joined", "1"), line("round_checks.joined", "2"));
        assert!(hint.ends_with(&both), "{hint}");
        for wave in [1, 2] {
            let fix = fix_kept(root, wave).unwrap_or_else(|| panic!("the fix of wave {wave} is kept: {joined}"));
            assert!(fix.starts_with(&fix_head(&wave.to_string(), silent)) && fix.ends_with("o teste soma caiu"), "{fix}");
        }
        assert_eq!(delivered_count(root), 0, "nothing was taken: {joined}");
    }

    /// Duas ondas na mesma rodada e a suíte vermelha sem citar arquivo de
    /// nenhuma: o conserto volta às duas. Só a onda 1 grava a entrega de
    /// novo. Na primeira e na segunda rodada de conserto dela a suíte ainda
    /// devolve o conserto; na volta que vem depois das duas, a rodada para e
    /// faz ao usuário a pergunta da conferência depois da onda, com o fim da
    /// saída, e não grava trecho para a onda 1. A onda 2, que não passou por
    /// rodada de conserto nenhuma, ainda recebe o dela. Nada é comitado.
    #[test]
    fn a_red_build_returns_the_fix_for_two_fix_rounds_and_then_asks_the_user() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        let silent = "echo 'o teste soma caiu'; exit 1";
        let config = json!({"maxCompilingWaves": 2, "buildCommand": silent}).to_string();
        std::fs::write(root.join("mustard.json"), config).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);
        back_with(root, 2, "src/b.rs");
        for fix_round in 1..=2 {
            back_with(root, 1, "src/a.rs");
            let fixed = round(root, "x", None);
            assert_eq!(fixed["reason"], json!("round-build-failed"), "fix round {fix_round}: {fixed}");
            assert_eq!(fixed.get("question"), None, "fix round {fix_round}: {fixed}");
            assert!(fix_kept(root, 1).is_some_and(|fix| fix.ends_with("o teste soma caiu")), "fix round {fix_round}: {fixed}");
        }

        back_with(root, 1, "src/a.rs");
        let asked = round(root, "x", None);
        assert_eq!(asked["reason"], json!("round-after-wave-limit"), "{asked}");
        let question = translate("round.after_wave.question", Locale::PtBr).replace("{waves}", "1").replace("{max}", "2");
        assert_eq!(asked["question"], json!(question), "{asked}");
        let hint = asked["hint"].as_str().unwrap_or_default();
        let limit = translate("round.after_wave.limit", Locale::PtBr).replace("{waves}", "1").replace("{max}", "2");
        let joined = |wave: &str| format!("\n- {}", translate("round_checks.joined", Locale::PtBr).replace("{wave}", wave));
        assert!(hint.starts_with(&limit) && hint.contains("o teste soma caiu"), "{hint}");
        assert!(hint.ends_with(&joined("2")) && !hint.contains(&joined("1")), "{hint}");
        assert_eq!(fix_kept(root, 1), None, "the wave past its fix rounds goes to the user: {asked}");
        assert!(fix_kept(root, 2).is_some_and(|fix| fix.ends_with("o teste soma caiu")), "{asked}");
        assert_eq!(delivered_count(root), 0, "nothing was taken: {asked}");
    }

    /// Com o Claude Code que mandou a onda aberto, a suíte vermelha manda o
    /// conserto ao agente dela. Fechado ele, a rodada seguinte recusa a mesma
    /// volta e manda despachar um agente novo pelo título da onda.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_red_build_asks_for_a_new_agent_once_the_sender_of_the_wave_closed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), json!({"buildCommand": "echo caiu; exit 1"}).to_string()).unwrap();
        round(root, "x", None);
        back_with(root, 1, "src/a.rs");
        let title = mustard_core::domain::wave_prompt::wave_title("x", 1, Locale::PtBr);
        let new_agent = translate("round.after_wave.new_agent", Locale::PtBr).replace("{wave}", "1").replace("{title}", &title);
        let open = round(root, "x", None);
        assert_eq!(open["reason"], json!("round-build-failed"), "{open}");
        assert!(!open["hint"].as_str().unwrap_or_default().contains(&new_agent), "{open}");

        orphan_the_send(root, 1);
        let closed = round(root, "x", None);
        assert_eq!(closed["reason"], json!("round-build-failed"), "{closed}");
        assert!(closed["hint"].as_str().unwrap_or_default().ends_with(&format!("\n\n{new_agent}")), "{closed}");
    }
}
