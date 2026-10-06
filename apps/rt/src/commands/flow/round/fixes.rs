//! Os trechos de conserto: o que a rodada devolve ao agente de uma onda cuja
//! volta ela recusou, gravado na pasta de despacho da spec para a volta
//! pendente da onda. Gravam aqui a conferência depois da onda, o comando
//! declarado que caiu antes do commit, a verificação de critério que não
//! passou e a reprovação de quem conduz; o gancho da mensagem ao agente da
//! onda e o do despacho do agente novo leem daqui.

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::wave_prompt::wave_title;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};

use super::answer::RoundRefusal;
use super::commit::ensure_criteria_proofs;
use super::queue::waves_awaiting_new_agent;
use super::report::WaveReport;
use super::stops::{fix_limit_refusal, split_at_fix_limit};

/// A verificação de um critério que caiu antes do commit, e as ondas a quem
/// o conserto volta.
pub(crate) struct CriterionFix {
    /// A recusa como a verificação a deu: a que não passou, a que saiu verde
    /// sem rodar teste ou a que cita um teste que não existe.
    pub refused: RoundRefusal,
    /// O código do critério.
    pub code: String,
    /// As ondas da rodada que cobrem o critério e a quem o conserto ainda
    /// volta, na ordem do relatório.
    pub waves: Vec<u64>,
    /// As ondas de `waves` que esperam um agente novo, cada uma com o título
    /// que o despacha: a rodada as preenche depois de gravar os trechos.
    pub new_agents: Vec<(u64, String)>,
}

impl CriterionFix {
    /// A recusa a quem conduz: a da verificação, como veio, e o próximo
    /// passo — mandar cada onda abaixo de volta ao agente dela, ou despachar
    /// um agente novo para a que perdeu o dela.
    pub(super) fn message(&self, lang: Locale) -> String {
        let mut text = self.refused.message(lang);
        if !self.waves.is_empty() {
            text.push_str("\n\n");
            text.push_str(translate("round_checks.criterion_next", lang));
        }
        for wave in &self.waves {
            let line = translate("round_checks.criterion_wave", lang).replace("{wave}", &wave.to_string()).replace("{code}", &self.code);
            text.push_str("\n- ");
            text.push_str(&line);
        }
        for (wave, title) in &self.new_agents {
            let new_agent = translate("round.after_wave.new_agent", lang).replace("{wave}", &wave.to_string()).replace("{title}", title);
            text.push_str("\n\n");
            text.push_str(&new_agent);
        }
        text
    }

    /// O trecho de conserto que volta ao agente da onda `wave`: o critério,
    /// o que a verificação fez e o que fazer, sem o passo de quem conduz.
    fn section(&self, wave: u64, lang: Locale) -> String {
        let (key, slots): (&str, Vec<(&str, String)>) = match &self.refused {
            RoundRefusal::CriterionProofFailed { command, output, .. } => {
                ("round_checks.criterion_failed_fix", vec![("{command}", command.clone()), ("{output}", output.clone())])
            }
            RoundRefusal::CriterionRanNoTest { command, tests, .. } => {
                ("round_checks.criterion_no_test_fix", vec![("{command}", command.clone()), ("{count}", tests.to_string())])
            }
            RoundRefusal::CriterionMissingTest { name, .. } => ("round_checks.criterion_missing_test_fix", vec![("{name}", name.clone())]),
            other => return other.message(lang),
        };
        let start = translate(key, lang).replace("{wave}", &wave.to_string()).replace("{code}", &self.code);
        slots.into_iter().fold(start, |text, (slot, value)| text.replace(slot, &value))
    }
}

/// Roda, antes do commit, a verificação de cada critério que as ondas
/// `waves` do relatório `reports` cobrem ([`ensure_criteria_proofs`]), sem o
/// critério que uma tarefa devolvida como não feita ainda cobre, e com a
/// verificação nova entregue (`delivered`) no lugar da gravada. A que não
/// passa recusa, e a recusa grava, antes de sair, o trecho de conserto de
/// cada onda que cobre o critério e ainda não passou por todas as rodadas de
/// conserto ([`criterion_fix`], [`keep_fixes`]): o conserto volta ao agente
/// que a fez. Devolve as verificações que passaram.
pub(super) fn prove_criteria(
    root: &Path,
    spec: &str,
    log: &SpecLog,
    reports: &[WaveReport],
    waves: &[u64],
    delivered: &[(u64, String)],
    lang: Locale,
) -> Result<Vec<String>, RoundRefusal> {
    let undone: Vec<u64> = reports.iter().flat_map(|wave| wave.undone.iter().map(|(id, _)| *id)).collect();
    ensure_criteria_proofs(root, log, waves, &undone, delivered).map_err(|refused| {
        let refused = criterion_fix(refused, log, reports, lang);
        keep_fixes(root, spec, log, &refused, lang);
        refused
    })
}

/// A recusa `refused` da verificação de um critério, com as ondas de
/// `reports` que cobrem o critério — pela onda de cada uma e pelas que ela
/// conserta, a mesma leitura que escolheu as verificações a rodar —, para o
/// conserto voltar ao agente de cada uma ([`keep_fixes`]), até o teto de
/// rodadas de conserto que a compilação, o lint e a suíte também têm
/// ([`split_at_fix_limit`]). Alguma onda que cobre o critério já passou por
/// todas: a rodada para e faz ao usuário a pergunta da conferência depois da
/// onda, com a recusa da verificação ([`fix_limit_refusal`]); a onda no teto
/// vai ao usuário, e não ao agente, e só as outras ganham o trecho de
/// conserto. Outra recusa sai como veio.
fn criterion_fix(refused: RoundRefusal, log: &SpecLog, reports: &[WaveReport], lang: Locale) -> RoundRefusal {
    let code = match &refused {
        RoundRefusal::CriterionProofFailed { code, .. }
        | RoundRefusal::CriterionRanNoTest { code, .. }
        | RoundRefusal::CriterionMissingTest { code, .. } => code.clone(),
        _ => return refused,
    };
    let codes = log.codes();
    let covers = |report: &WaveReport| {
        let own: Vec<u64> = std::iter::once(report.wave).chain(report.fixes.iter().copied()).collect();
        log.criteria_for_waves(&own).iter().any(|e| codes.get(&e.id).cloned().unwrap_or_else(|| e.id.to_string()) == code)
    };
    let waves: Vec<u64> = reports.iter().filter(|report| covers(report)).map(|report| report.wave).collect();
    let (stuck, back) = split_at_fix_limit(&waves, reports);
    let fix = CriterionFix { refused, code, waves: back, new_agents: Vec::new() };
    if stuck.is_empty() {
        return RoundRefusal::CriterionFix(Box::new(fix));
    }
    let fixes = fix.waves.iter().map(|wave| (*wave, fix.section(*wave, lang))).collect();
    fix_limit_refusal(&stuck, &fix.message(lang), fixes, lang)
}

/// O arquivo com o trecho que a conferência depois da onda, o comando
/// declarado que caiu antes do commit ou a verificação de critério que não
/// passou devolveu para a onda `wave`, gravado
/// pela volta que a rodada recusou, ou com o motivo de quem conduz a obra,
/// pela volta que ele reprovou: a última entrega da onda
/// que nenhuma rodada assumiu. A entrega nova muda o arquivo, e o trecho de
/// uma volta velha nunca chega ao agente. Nada sem volta pendente da onda.
pub(crate) fn fix_file(root: &Path, spec: &str, log: &SpecLog, wave: u64) -> Option<PathBuf> {
    let back = log.unassumed_returns().into_iter().filter(|e| e.event_type == "delivered" && e.wave() == Some(wave)).map(|e| e.id).max()?;
    Some(fixes_dir(root, spec)?.join(format!("fix-{wave}-{back}.md")))
}

/// A recusa da conferência depois da onda, a do comando declarado que caiu
/// antes do commit ou a da verificação de critério que não passou, com, para
/// cada onda recusada que espera um agente novo
/// ([`waves_awaiting_new_agent`]), a frase que manda o condutor despachar um
/// pelo título dela; a onda que não está ali segue com o conserto mandado ao
/// agente que a fez. Outra recusa sai como veio.
pub(super) fn new_agents_named(refused: RoundRefusal, root: &Path, spec: &str, log: &SpecLog, lang: Locale) -> RoundRefusal {
    match refused {
        RoundRefusal::AfterWave { mut text, question, fixes } => {
            let awaiting = waves_awaiting_new_agent(root, spec, log);
            for wave in fixes.iter().map(|(wave, _)| *wave).filter(|wave| awaiting.contains_key(wave)) {
                let line = translate("round.after_wave.new_agent", lang)
                    .replace("{wave}", &wave.to_string())
                    .replace("{title}", &wave_title(spec, wave, lang));
                text = format!("{text}\n\n{line}");
            }
            RoundRefusal::AfterWave { text, question, fixes }
        }
        RoundRefusal::CheckFailed(mut failure) => {
            let awaiting = waves_awaiting_new_agent(root, spec, log);
            let waves = failure.waves.iter().copied().filter(|wave| awaiting.contains_key(wave));
            failure.new_agents = waves.map(|wave| (wave, wave_title(spec, wave, lang))).collect();
            RoundRefusal::CheckFailed(failure)
        }
        RoundRefusal::CriterionFix(mut fix) => {
            let awaiting = waves_awaiting_new_agent(root, spec, log);
            let waves = fix.waves.iter().copied().filter(|wave| awaiting.contains_key(wave));
            fix.new_agents = waves.map(|wave| (wave, wave_title(spec, wave, lang))).collect();
            RoundRefusal::CriterionFix(fix)
        }
        other => other,
    }
}

/// A pasta de despacho da spec, onde moram os trechos de conserto.
fn fixes_dir(root: &Path, spec: &str) -> Option<PathBuf> {
    Some(store::spec_file(root, spec).ok()?.parent()?.join(".dispatch"))
}

/// A onda do trecho de conserto `path`, pelo nome que [`fix_file`] dá a ele;
/// nada para outro arquivo da pasta.
fn fix_wave(path: &Path) -> Option<u64> {
    let name = path.file_name()?.to_str()?.strip_prefix("fix-")?.strip_suffix(".md")?;
    let (wave, back) = name.split_once('-')?;
    back.parse::<u64>().ok().and(wave.parse().ok())
}

/// Apaga da pasta de despacho da spec cada trecho de conserto que não é mais
/// o da volta pendente da onda dele ([`fix_file`], pela leitura `log`): o da
/// volta velha, que a entrega nova trocou, e o da onda que a rodada já
/// comitou. Sem leitura, como no fechamento, todos saem. A pasta vazia sai
/// junto; o arquivo que não sai fica para a próxima varredura. Quem chama
/// segura a trava do passo do git e leu `log` sob ela: a rodada ao mesmo
/// tempo nunca grava um trecho mais novo que esta leitura.
pub(crate) fn sweep_fixes(root: &Path, spec: &str, log: Option<&SpecLog>) {
    let Some(dir) = fixes_dir(root, spec) else { return };
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for path in entries.flatten().map(|entry| entry.path()) {
        let Some(wave) = fix_wave(&path) else { continue };
        if log.and_then(|log| fix_file(root, spec, log, wave)).as_ref() != Some(&path) {
            let _ = std::fs::remove_file(&path);
        }
    }
    let _ = std::fs::remove_dir(&dir);
}

/// Grava, para cada onda que a conferência depois da onda recusou, o trecho
/// dela no arquivo da volta recusada ([`fix_file`]), onde o gancho da
/// mensagem ao agente da onda o lê; para cada onda a quem volta o conserto
/// do comando declarado que caiu antes do commit, o comando e o fim da saída;
/// e, para cada onda que cobre o critério cuja verificação não passou, o
/// critério e o que a verificação fez, no idioma `lang`. Antes, toda recusa
/// varre os trechos que ficaram velhos ([`sweep_fixes`]); outra recusa não
/// grava nada, e a falha de gravação deixa a mensagem do condutor passar como
/// veio.
pub(super) fn keep_fixes(root: &Path, spec: &str, log: &SpecLog, refused: &RoundRefusal, lang: Locale) {
    sweep_fixes(root, spec, Some(log));
    let fixes: Vec<(u64, String)> = match refused {
        RoundRefusal::AfterWave { fixes, .. } => fixes.clone(),
        RoundRefusal::CheckFailed(failure) => failure.waves.iter().map(|wave| (*wave, failure.fix(*wave, lang))).collect(),
        RoundRefusal::CriterionFix(fix) => fix.waves.iter().map(|wave| (*wave, fix.section(*wave, lang))).collect(),
        _ => return,
    };
    for (wave, section) in fixes {
        let Some(file) = fix_file(root, spec, log, wave) else { continue };
        let _ = file.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(&file, section);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mustard_core::io::spec_events as store;
    use mustard_core::platform::i18n::{translate, Locale};
    use serde_json::json;
    use tempfile::tempdir;

    use super::fix_file;
    use crate::commands::flow::round::tests::*;

    /// O trecho de conserto gravado para a volta pendente da onda `wave`, ou
    /// nada quando a rodada não gravou nenhum para ela.
    fn fix_kept(root: &Path, wave: u64) -> Option<String> {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        fix_file(root, "x", &log, wave).and_then(|file| std::fs::read_to_string(file).ok())
    }

    /// A onda `wave` volta com o arquivo `file` mudado na cópia dela e, com
    /// `proof`, a verificação nova do critério da spec na entrega.
    fn back_with(root: &Path, wave: u64, file: &str, proof: Option<&str>) {
        let copy = mustard_core::io::wave_prompt::slot_path(root, "x", usize::try_from(wave).unwrap() - 1);
        let before = std::fs::read_to_string(copy.join(file)).unwrap_or_default();
        std::fs::write(copy.join(file), format!("{before}// onda {wave}\n")).unwrap();
        let mut body = json!({"wave": wave, "text": "Saiu.", "files": [file], "commit": format!("a onda {wave} sai")});
        if let Some(proof) = proof {
            body["proofs"] = json!([{"criterion": "MSTD-CRIT-0001", "proof": proof}]);
        }
        assert_eq!(returned(root, body)["ok"], json!(true));
    }

    /// Duas ondas, cada uma com o próprio critério: a onda 2 passa a cobrir
    /// só o critério novo, que sempre passa.
    fn two_waves_two_criteria(root: &Path) {
        approved_with(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])], |said| {
            let crit = write(root, "x", "criterion", json!({"when": "a dobra roda", "then": "a dobra passa",
                "proof": "git --version", "form": "ubiquitous", "origin": said}));
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let old = log.visible().into_iter().find(|e| e.event_type == "wave" && e.int("n") == Some(2)).unwrap().id;
            let wave = json!({"n": 2, "text": "Onda 2.", "criteria": [id_of(&crit)], "done_when": "A suíte passa.",
                "origin": said, "replaces": old});
            assert_eq!(write(root, "x", "wave", wave)["ok"], json!(true));
        });
        std::fs::write(root.join("mustard.json"), json!({"maxCompilingWaves": 2}).to_string()).unwrap();
    }

    /// Duas ondas na mesma rodada, e a verificação do critério que só a onda
    /// 1 cobre não passa. A recusa diz a quem conduz que mande a onda 1 de
    /// volta ao agente dela, e o trecho de conserto fica gravado para a volta
    /// dela, e não para a da onda 2. A verificação que cai e a que sai verde
    /// sem rodar teste chegam ao agente com o critério e o que a verificação
    /// fez; a que cita um teste que não existe, no teste do teto de rodadas
    /// de conserto. Nada é comitado.
    #[test]
    fn a_failing_criterion_verification_sends_the_fix_to_the_wave_that_covers_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        two_waves_two_criteria(root);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);
        back_with(root, 2, "src/b.rs", None);
        let fix = |key: &str, slots: &[(&str, &str)]| {
            let start = translate(key, Locale::PtBr).replace("{wave}", "1").replace("{code}", "MSTD-CRIT-0001");
            slots.iter().fold(start, |text, (slot, value)| text.replace(slot, value))
        };
        let next = translate("round_checks.criterion_next", Locale::PtBr);
        let line = translate("round_checks.criterion_wave", Locale::PtBr).replace("{wave}", "1").replace("{code}", "MSTD-CRIT-0001");

        let broken = "git --nao-existe-esta-opcao";
        back_with(root, 1, "src/a.rs", Some(broken));
        let failed = round(root, "x", None);
        assert_eq!(failed["reason"], json!("round-criterion-proof-failed"), "{failed}");
        let hint = failed["hint"].as_str().unwrap_or_default();
        assert!(hint.ends_with(&format!("\n\n{next}\n- {line}")), "{hint}");
        let kept = fix_kept(root, 1).unwrap_or_else(|| panic!("the fix of wave 1 is kept: {failed}"));
        let head = fix("round_checks.criterion_failed_fix", &[("{command}", broken)]);
        let head = head.split_once("{output}").map(|(head, _)| head.to_string()).unwrap();
        assert!(kept.starts_with(&head) && kept.contains("nao-existe-esta-opcao"), "{kept}");
        assert_eq!(fix_kept(root, 2), None, "the wave that does not cover the criterion gets no fix: {failed}");

        let zero = "echo running 0 tests";
        back_with(root, 1, "src/a.rs", Some(zero));
        let no_test = round(root, "x", None);
        assert_eq!(no_test["reason"], json!("round-criterion-ran-no-test"), "{no_test}");
        let expected = fix("round_checks.criterion_no_test_fix", &[("{command}", zero), ("{count}", "0")]);
        assert_eq!(fix_kept(root, 1), Some(expected), "{no_test}");
        assert_eq!(fix_kept(root, 2), None, "{no_test}");
        assert_eq!(delivered_count(root), 0, "nothing was taken: {no_test}");
    }

    /// Duas ondas na mesma rodada cobrem o mesmo critério, e a verificação
    /// dele cita um teste que não existe. Só a onda 1 grava a entrega de
    /// novo. Na primeira e na segunda rodada de conserto dela a verificação
    /// ainda devolve o conserto, com o critério e o nome que faltou; na volta
    /// que vem depois das duas, a rodada para e faz ao usuário a pergunta da
    /// conferência depois da onda, com a recusa da verificação, e não grava
    /// trecho para a onda 1. A onda 2, que não passou por rodada de conserto
    /// nenhuma, ainda recebe o dela. Nada é comitado.
    #[test]
    fn a_failing_criterion_verification_returns_the_fix_for_two_fix_rounds_and_then_asks_the_user() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), json!({"maxCompilingWaves": 2}).to_string()).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);
        back_with(root, 2, "src/b.rs", None);
        let absent = "echo running 1 test teste_que_nao_existe_aqui";
        let section = |wave: &str| {
            translate("round_checks.criterion_missing_test_fix", Locale::PtBr)
                .replace("{wave}", wave)
                .replace("{code}", "MSTD-CRIT-0001")
                .replace("{name}", "teste_que_nao_existe_aqui")
        };
        for fix_round in 1..=2 {
            back_with(root, 1, "src/a.rs", Some(absent));
            let fixed = round(root, "x", None);
            assert_eq!(fixed["reason"], json!("round-criterion-missing-test"), "fix round {fix_round}: {fixed}");
            assert_eq!(fixed.get("question"), None, "fix round {fix_round}: {fixed}");
            assert_eq!(fix_kept(root, 1), Some(section("1")), "fix round {fix_round}: {fixed}");
        }

        back_with(root, 1, "src/a.rs", Some(absent));
        let asked = round(root, "x", None);
        assert_eq!(asked["reason"], json!("round-after-wave-limit"), "{asked}");
        let question = translate("round.after_wave.question", Locale::PtBr).replace("{waves}", "1").replace("{max}", "2");
        assert_eq!(asked["question"], json!(question), "{asked}");
        let hint = asked["hint"].as_str().unwrap_or_default();
        let limit = translate("round.after_wave.limit", Locale::PtBr).replace("{waves}", "1").replace("{max}", "2");
        let refused = translate("round.criterion_missing_test", Locale::PtBr)
            .replace("{code}", "MSTD-CRIT-0001")
            .replace("{name}", "teste_que_nao_existe_aqui");
        let line = |wave: &str| {
            let line = translate("round_checks.criterion_wave", Locale::PtBr).replace("{wave}", wave).replace("{code}", "MSTD-CRIT-0001");
            format!("\n- {line}")
        };
        assert!(hint.starts_with(&format!("{limit}\n\n{refused}")), "{hint}");
        assert!(hint.ends_with(&line("2")) && !hint.contains(&line("1")), "{hint}");
        assert_eq!(fix_kept(root, 1), None, "the wave past its fix rounds goes to the user: {asked}");
        assert_eq!(fix_kept(root, 2), Some(section("2")), "{asked}");
        assert_eq!(delivered_count(root), 0, "nothing was taken: {asked}");
    }

    /// Com o Claude Code que mandou a onda aberto, a verificação do critério
    /// que não passa manda o conserto ao agente dela. Fechado ele, a rodada
    /// seguinte recusa a mesma volta e manda despachar um agente novo pelo
    /// título da onda.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_failing_criterion_verification_asks_for_a_new_agent_once_the_sender_of_the_wave_closed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        back_with(root, 1, "src/a.rs", Some("git --nao-existe-esta-opcao"));
        let title = mustard_core::domain::wave_prompt::wave_title("x", 1, Locale::PtBr);
        let new_agent = translate("round.after_wave.new_agent", Locale::PtBr).replace("{wave}", "1").replace("{title}", &title);
        let open = round(root, "x", None);
        assert_eq!(open["reason"], json!("round-criterion-proof-failed"), "{open}");
        assert!(!open["hint"].as_str().unwrap_or_default().contains(&new_agent), "{open}");

        orphan_the_send(root, 1);
        let closed = round(root, "x", None);
        assert_eq!(closed["reason"], json!("round-criterion-proof-failed"), "{closed}");
        assert!(closed["hint"].as_str().unwrap_or_default().ends_with(&format!("\n\n{new_agent}")), "{closed}");
        assert!(fix_kept(root, 1).is_some(), "the new agent gets the fix of the wave: {closed}");
    }
}
