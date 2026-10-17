//! As marcas de conserto que o fechamento gravou sem prova.
//!
//! Antes do conserto só com prova, o lint, a suíte ou a análise de
//! declarações sem uso do fechamento que caía reprovava toda onda do plano,
//! citasse a saída arquivo dela ou não. Essas marcas seguravam a obra: o
//! fechamento recusava a onda reprovada, a rodada a despachava para o
//! conserto, e só uma entrega vazia de cada uma a soltava. No começo do
//! fechamento e da rodada, a marca cuja saída gravada não cita arquivo da
//! onda sai sozinha, pela mesma leitura que decide quem reabre
//! ([`crate::commands::flow::validation::cited`]).

use std::path::Path;

use mustard_core::domain::spec_events::{Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::json;

use crate::commands::flow::validation::{Work, cited};
use crate::commands::spec_events::write::record;

/// Os motivos das marcas que reabriam toda onda do plano sem prova: o lint, a
/// suíte e a análise de declarações sem uso do fechamento, que hoje só
/// reabrem a onda cujo arquivo a saída cita. A compilação do fechamento segue
/// a mesma leitura. A marca do critério que falhou aponta as ondas que o
/// cobrem, e não sai daqui.
const UNPROVEN_REASONS: [&str; 4] = ["build-failed", "lint-failed", "suite-failed", "final-new-unused"];

/// Solta cada onda com o conserto pendente cuja reprovação é uma marca da
/// compilação, do lint, da suíte ou da análise de declarações sem uso do fechamento e cuja
/// saída gravada não cita arquivo que a obra mudou e que a onda declarou ou
/// entregou. A soltura é uma marca nova, do binário, com o resultado
/// `released`, a mesma impressão e o mesmo motivo da reprovação e o texto que
/// diz por que saiu: a mais nova da onda, ela a tira da leitura das ondas
/// reprovadas sem apagar nada. O fechamento a chama antes de conferir a obra,
/// e a rodada antes de escolher as ondas que voltam ao conserto: as duas leem
/// a mesma soltura. Devolve as ondas soltas, na ordem do plano.
pub(crate) fn release(root: &Path, spec: &str, log: &SpecLog, lang: Locale) -> Result<Vec<u64>, Refusal> {
    let marks: Vec<(u64, &SpecEvent)> = crate::commands::flow::round::waves_pending_fix(log)
        .into_iter()
        .filter_map(|(wave, id)| log.get(id).map(|mark| (wave, mark)))
        .filter(|(_, mark)| mark.event_type == "validation_failure")
        .filter(|(_, mark)| mark.str_field("reason").is_some_and(|reason| UNPROVEN_REASONS.contains(&reason)))
        .collect();
    if marks.is_empty() {
        return Ok(Vec::new());
    }
    let waves: Vec<u64> = marks.iter().map(|(wave, _)| *wave).collect();
    let work = Work::of(root, log, &waves);
    let mut released = Vec::new();
    for (wave, mark) in marks {
        if cited(root, mark.str_field("text").unwrap_or_default(), &work).waves.contains(&wave) {
            continue;
        }
        let text = translate("close.stale_mark_released", lang).replace("{wave}", &wave.to_string());
        let draft = json!({"wave": wave, "final": true, "result": "released", "text": text, "reason": mark.str_field("reason"),
            "fingerprint": mark.str_field("fingerprint"), "author": "binary"});
        record(root, spec, "validation_failure", draft.as_object().cloned().unwrap_or_default(), PhaseWriter::Binary)?;
        released.push(wave);
    }
    Ok(released)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mustard_core::domain::spec_events::SpecLog;
    use mustard_core::domain::spec_state::PhaseWriter;
    use mustard_core::io::spec_events as store;
    use mustard_core::platform::i18n::{Locale, translate};
    use mustard_core::view::document::WaveState;
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::super::CloseRefusal;
    use super::super::tests::{branched_from_dev, close_for, git_at, ready_with_waves, round_for, wave_file};
    use crate::commands::flow::close::CloseOpts;
    use crate::commands::flow::round::RoundOpts;
    use crate::commands::spec_events::write::record;

    /// A impressão gravada nas marcas de antes.
    const BEFORE: &str = "impressao-de-antes";

    fn log_of(root: &Path) -> SpecLog {
        store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
    }

    /// A marca que o fechamento gravava na onda `wave` quando o comando caía:
    /// o texto da recusa, com a saída `output`, e a linha do conserto.
    fn old_mark(root: &Path, wave: u64, reason: &str, output: &str) {
        let (command, output) = ("cargo clippy".to_string(), output.to_string());
        let failure = match reason {
            "build-failed" => CloseRefusal::BuildFailed { command, output, no_wave: None },
            "suite-failed" => CloseRefusal::SuiteFailed { command, output, no_wave: None },
            "lint-failed" => CloseRefusal::LintFailed { command, output, no_wave: None },
            "final-new-unused" => CloseRefusal::NewUnused { text: output, no_wave: None },
            _ => CloseRefusal::CriterionFailed { code: "MSTD-CRIT-0001".into(), output },
        };
        let text = format!("{}\n{}", failure.message(Locale::PtBr), translate("close.repair_next", Locale::PtBr));
        let draft = json!({"wave": wave, "final": true, "result": "rejected", "text": text, "reason": reason,
            "fingerprint": BEFORE, "files": [wave_file(wave)], "author": "binary"});
        record(root, "x", "validation_failure", draft.as_object().cloned().unwrap(), PhaseWriter::Binary).unwrap();
    }

    /// Uma onda solta: o número, o motivo, a impressão e o autor da soltura.
    type Release<'a> = (u64, Option<&'a str>, Option<&'a str>, Option<&'a str>);

    /// As ondas soltas, cada uma com o motivo, a impressão e o autor da
    /// soltura.
    fn released(log: &SpecLog) -> Vec<Release<'_>> {
        log.events
            .iter()
            .filter(|e| e.event_type == "validation_failure" && e.str_field("result") == Some("released"))
            .map(|e| (e.wave().unwrap_or_default(), e.str_field("reason"), e.str_field("fingerprint"), e.str_field("author")))
            .collect()
    }

    fn close_with(root: &Path, config: &Value) -> Value {
        std::fs::write(root.join("mustard.json"), config.to_string()).unwrap();
        close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), ..Default::default() }, None)
    }

    /// Duas ondas reprovadas pela regra antiga, uma pelo lint e outra pela
    /// suíte, com a saída citando só um arquivo que a branch não mudou: o
    /// fechamento solta as duas, com a mesma impressão e o mesmo motivo, e
    /// segue para o lint e a suíte. A suíte cai de novo fora da obra, e
    /// nenhuma onda reabre: a rodada seguinte não pede agente nenhum.
    #[test]
    fn stale_final_marks_without_a_cited_file_are_released() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);
        branched_from_dev(root);
        old_mark(root, 1, "lint-failed", "error: unused variable at lib/fora.rs:1:4");
        old_mark(root, 2, "suite-failed", "error: unused variable at lib/fora.rs:1:4");
        assert_eq!(crate::commands::flow::round::waves_pending_fix(&log_of(root)).into_keys().collect::<Vec<_>>(), vec![1, 2]);

        let config = json!({"lintCommand": "echo lint >> rodou", "testCommand": "echo suite >> rodou; echo error: lib/fora.rs:1:4; exit 1"});
        let refused = close_with(root, &config);
        assert_eq!(refused["reason"], json!("suite-failed"), "{refused}");
        assert_eq!(std::fs::read_to_string(root.join("rodou")).unwrap_or_default(), "lint\nsuite\n", "{refused}");

        let log = log_of(root);
        assert_eq!(
            released(&log),
            vec![(1, Some("lint-failed"), Some(BEFORE), Some("binary")), (2, Some("suite-failed"), Some(BEFORE), Some("binary"))],
            "{refused}"
        );
        assert!(log.last_rejected().is_empty(), "{refused}");
        let states = crate::commands::flow::round::wave_states(&log);
        assert_eq!(states.values().copied().collect::<Vec<_>>(), vec![WaveState::Approved, WaveState::Approved], "{states:?}");

        let next = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        assert_eq!(next["dispatch"].as_array().map(Vec::len).unwrap_or_default(), 0, "{next}");
        assert_eq!(next["command"], json!("mustard-rt run close --spec x"), "{next}");
    }

    /// A marca da compilação do fechamento segue a mesma leitura: a que cita
    /// só um arquivo TypeScript que o projeto não tem sai solta, com a mesma
    /// impressão e o mesmo motivo, e o fechamento compila de novo.
    #[test]
    fn a_build_mark_without_a_cited_wave_file_is_released() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 1);
        branched_from_dev(root);
        old_mark(root, 1, "build-failed", "web/fora.ts(1,8): error TS2304: Cannot find name 'x'.");

        let refused = close_with(root, &json!({"buildCommand": "echo build >> rodou"}));
        assert_eq!(std::fs::read_to_string(root.join("rodou")).unwrap_or_default(), "build\n", "{refused}");
        assert_eq!(released(&log_of(root)), vec![(1, Some("build-failed"), Some(BEFORE), Some("binary"))], "{refused}");
    }

    /// A marca cuja saída cita um arquivo que a branch mudou e que a onda
    /// declarou fica, e a de um critério que falhou também, mesmo sem citar
    /// arquivo nenhum: o fechamento recusa pela onda reprovada antes de rodar
    /// comando nenhum, e nada sai solto.
    #[test]
    fn final_marks_with_a_cited_wave_file_or_from_a_criterion_stay() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);
        branched_from_dev(root);
        old_mark(root, 1, "criterion-failed", "a prova caiu");
        old_mark(root, 2, "lint-failed", &format!("error: lib/fora.rs:1:4 and {}:2:5", wave_file(2)));

        let refused = close_with(root, &json!({"lintCommand": "echo lint >> rodou"}));
        assert_eq!(refused["reason"], json!("wave-rejected"), "{refused}");
        assert!(!root.join("rodou").exists(), "{refused}");
        let log = log_of(root);
        assert!(released(&log).is_empty(), "{refused}");
        assert_eq!(log.last_rejected().into_keys().collect::<Vec<_>>(), vec![1, 2], "{refused}");
    }

    /// A análise de declarações sem uso gravava o mesmo achado em toda onda
    /// do plano. Com o achado só no arquivo da onda 2, o fechamento solta a
    /// onda 1, com a mesma impressão e o mesmo motivo, e mantém a 2, que
    /// recusa o fechamento antes de rodar comando nenhum.
    #[test]
    fn a_stale_unused_mark_stays_only_on_the_wave_whose_file_it_cites() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);
        branched_from_dev(root);
        let finding = translate("round.after_wave.unused", Locale::PtBr)
            .replace("{name}", "lonely_total")
            .replace("{file}", &wave_file(2))
            .replace("{line}", "3");
        old_mark(root, 1, "final-new-unused", &finding);
        old_mark(root, 2, "final-new-unused", &finding);

        let refused = close_with(root, &json!({"lintCommand": "echo lint >> rodou"}));
        assert_eq!(refused["reason"], json!("wave-rejected"), "{refused}");
        assert!(!root.join("rodou").exists(), "{refused}");
        let log = log_of(root);
        assert_eq!(released(&log), vec![(1, Some("final-new-unused"), Some(BEFORE), Some("binary"))], "{refused}");
        assert_eq!(log.last_rejected().into_keys().collect::<Vec<_>>(), vec![2], "{refused}");
    }

    /// A rodada que vem antes de qualquer fechamento lê as mesmas marcas: a
    /// do lint que cita só um arquivo TypeScript que a branch mudou, mas que
    /// nenhuma onda declarou nem entregou, sai solta, e a onda 1 não volta ao
    /// conserto. A da suíte que cita o arquivo da onda 2 fica, e só a onda 2
    /// sai para o conserto.
    #[test]
    fn a_round_before_the_close_sends_no_repair_for_a_stale_mark() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);
        branched_from_dev(root);
        std::fs::create_dir_all(root.join("web")).unwrap();
        std::fs::write(root.join("web/extra.ts"), "export function lonelyTotal(): number {\n  return 0;\n}\n").unwrap();
        git_at(root, &["add", "web/extra.ts"]);
        git_at(root, &["commit", "-q", "-m", "fora das ondas"]);
        old_mark(root, 1, "lint-failed", "web/extra.ts(1,17): error TS6133: 'lonelyTotal' is declared but its value is never read.");
        old_mark(root, 2, "suite-failed", &format!("error: {}:2:5", wave_file(2)));

        let next = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        let sent: Vec<u64> = next["dispatch"].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect();
        assert_eq!(sent, vec![2], "{next}");
        let log = log_of(root);
        assert_eq!(released(&log), vec![(1, Some("lint-failed"), Some(BEFORE), Some("binary"))], "{next}");
        assert_eq!(log.last_rejected().into_keys().collect::<Vec<_>>(), vec![2], "{next}");
    }
}
