//! A recusa da análise final de declarações sem uso.
//!
//! O texto da recusa junta os achados que recusam, e cada um cita o arquivo
//! da declaração. É por esse arquivo, e não pela onda do achado, que o
//! fechamento decide quem volta ao conserto, pela mesma leitura do lint e da
//! suíte ([`crate::commands::flow::validation::cited`]).

use std::path::Path;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::platform::i18n::Locale;

use super::CloseRefusal;

/// A recusa da análise final de declarações sem uso da spec `spec`, com o
/// texto de cada achado que recusa; `None` sem achado que recuse. A prova é
/// do mapa; `outputs` são as saídas inteiras dos comandos verdes da máquina,
/// onde o aviso de código morto do compilador, quando existe, só acrescenta.
pub(super) fn refusal(root: &Path, spec: &str, log: &SpecLog, lang: Locale, outputs: &[String]) -> Option<CloseRefusal> {
    let failures: Vec<String> = crate::commands::flow::round::final_unused(root, spec, log, lang, outputs)
        .into_iter()
        .filter(|(refuses, _)| *refuses)
        .map(|(_, text)| text)
        .collect();
    (!failures.is_empty()).then(|| CloseRefusal::NewUnused { text: failures.join("\n"), no_wave: None })
}

#[cfg(test)]
mod tests {
    use mustard_core::io::spec_events as store;
    use mustard_core::platform::i18n::{Locale, translate};
    use serde_json::json;
    use tempfile::tempdir;

    use super::super::tests::{branched_from_dev, close_with_lint, git_at, ready_with_waves, round_for, wave_file};
    use super::super::{CloseRefusal, authorize_final_repair};
    use crate::commands::flow::round::RoundOpts;

    /// Duas ondas entregues no mesmo commit, e a onda 2 deixa no arquivo dela
    /// uma função privada que nada usa. O fechamento roda com o lint verde,
    /// que não escreve aviso de compilador nenhum: a análise final recusa
    /// pelo mapa, só a onda 2 reabre, com o achado no trecho de conserto, e a
    /// rodada seguinte despacha só ela.
    #[test]
    fn the_unused_analysis_reopens_only_the_wave_whose_file_it_cites() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);
        branched_from_dev(root);

        let file = wave_file(2);
        let unused = "fn lonely_total() {}";
        std::fs::write(root.join(&file), format!("fn one() {{ dois(); }}\nfn dois() {{}}\n{unused}\n")).unwrap();
        let declaration = json!({"kind": "function", "name": "lonely_total", "line": 3, "end_line": 3, "used_by": []});
        let base = json!({"modules": [{"path": &file, "language": "rust", "declarations": []}]});
        let after = json!({"modules": [{"path": &file, "language": "rust", "declarations": [declaration]}]});
        // O retrato do mapa da base mora fora do projeto: ele é cache, e
        // chegou a 89 MB dentro da pasta de uma spec numa máquina de verdade.
        let kept = mustard_core::io::wave_prompt::validation_base_map(root, "x");
        std::fs::create_dir_all(kept.parent().unwrap()).unwrap();
        std::fs::write(&kept, base.to_string()).unwrap();
        mustard_core::io::project_map::write_text(root, &after.to_string()).unwrap();

        let refused = close_with_lint(root, "git --version", None);
        assert_eq!(refused["reason"], json!("final-new-unused"), "{refused}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert_eq!(log.last_rejected().keys().copied().collect::<Vec<_>>(), vec![2], "{refused}");
        let fix = mustard_core::domain::wave_prompt::fix_lines(&log, 2);
        assert!(fix.first().and_then(|e| e.str_field("text")).is_some_and(|text| text.contains("`lonely_total`")), "{fix:?}");
        assert!(mustard_core::domain::wave_prompt::fix_lines(&log, 1).is_empty());

        let next = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        let sent: Vec<u64> = next["dispatch"].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect();
        assert_eq!(sent, vec![2], "{next}");
    }

    /// O achado sem uso num arquivo que a branch mudou, mas que nenhuma onda
    /// declarou nem entregou: nenhuma onda reabre, e a recusa diz que o
    /// arquivo citado não é de onda nenhuma.
    #[test]
    fn an_unused_finding_in_a_file_no_wave_touched_reopens_no_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);
        branched_from_dev(root);
        std::fs::write(root.join("src/extra.rs"), "fn lonely_total() {}\n").unwrap();
        git_at(root, &["add", "src/extra.rs"]);
        git_at(root, &["commit", "-q", "-m", "fora das ondas"]);

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let text = translate("round.after_wave.unused", Locale::PtBr)
            .replace("{name}", "lonely_total")
            .replace("{file}", "src/extra.rs")
            .replace("{line}", "1");
        let refused = authorize_final_repair(root, "x", &log, CloseRefusal::NewUnused { text, no_wave: None }, Locale::PtBr);
        assert_eq!(refused.reason(), "final-new-unused");
        let said = refused.message(Locale::PtBr);
        assert!(said.contains(translate("close.cited_unowned", Locale::PtBr)), "{said}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(!log.events.iter().any(|e| e.event_type == "validation_failure"), "{said}");
    }
}
