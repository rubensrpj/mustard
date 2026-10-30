//! Os testes da reprovação do fechamento que perdeu o motivo
//! ([`crate::commands::flow::round::expired`]), pela rodada e pelo fechamento
//! que a soltam.

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mustard_core::domain::spec_events::SpecLog;
    use mustard_core::domain::spec_state::PhaseWriter;
    use mustard_core::io::spec_events as store;
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::super::tests::{branched_from_dev, id_of, ready_with_waves, round_for, wave_file, write};
    use crate::commands::flow::round::RoundOpts;
    use crate::commands::flow::round::expired::expiry_fields;
    use crate::commands::spec_events::write::record;

    fn log_of(root: &Path) -> SpecLog {
        store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
    }

    fn round(root: &Path) -> Value {
        round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None)
    }

    /// A reprovação do fechamento na onda `wave`, com os campos de vencimento
    /// `extra`; devolve o número dela.
    fn verdict(root: &Path, wave: u64, (reason, text): (&str, &str), extra: Value) -> u64 {
        let mut draft = json!({"wave": wave, "final": true, "result": "rejected", "text": text, "reason": reason,
            "fingerprint": "impressao", "files": [wave_file(wave)], "author": "binary"});
        draft.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
        record(root, "x", "validation_failure", draft.as_object().cloned().unwrap(), PhaseWriter::Binary).unwrap().written.id
    }

    /// Os critérios da spec, na ordem em que foram gravados.
    fn criteria(root: &Path) -> Vec<u64> {
        log_of(root).visible().into_iter().filter(|e| e.event_type == "criterion").map(|e| e.id).collect()
    }

    /// Quatro ondas reprovadas pelo fechamento: a da prova de um critério, a
    /// de antes do campo dos itens, que cita o mesmo critério só no texto, a
    /// de outro critério, e a de prova que já passava antes gravada sem o
    /// código de antes. A última já nasce vencida e a rodada nem a despacha;
    /// as vagas da rodada só dão para o conserto das duas primeiras, que fica
    /// no ar, e a terceira espera vaga. Devolve a reprovação das quatro ondas.
    fn waves_sent_to_repair(root: &Path) -> [u64; 4] {
        ready_with_waves(root, "x", &["git --version", "git --help"], 4);
        branched_from_dev(root);
        let proofs = criteria(root);
        let cited = format!("A verificação do critério {} não passou.", log_of(root).codes()[&proofs[0]]);
        let failed = ("criterion-failed", "A prova caiu.");
        let first = verdict(root, 1, failed, json!({"items": [proofs[0]]}));
        let second = verdict(root, 2, ("criterion-failed", &cited), json!({}));
        let third = verdict(root, 3, failed, json!({"items": [proofs[1]]}));
        let fourth = verdict(root, 4, ("criterion-passed-before", "Já passava."), json!({}));
        let sent = round(root);
        assert_eq!(sent["ok"], json!(true), "{sent}");
        let log = log_of(root);
        assert_eq!(log.unanswered_sends().into_keys().collect::<Vec<_>>(), vec![1, 2], "o conserto das duas primeiras está no ar: {sent}");
        [first, second, third, fourth]
    }

    /// A reprovação cujo critério ganhou versão nova, e a de prova que já
    /// passava antes gravada sem o código de antes, perdem o motivo: a rodada
    /// as solta, não manda as ondas ao conserto, cancela o envio aberto delas
    /// sem agente e não as conta no limite de consertos. A reprovação do
    /// critério que segue valendo fica, e o conserto dela segue no ar.
    #[test]
    fn an_expired_verdict_sends_no_wave_to_repair() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let [first, second, third, fourth] = waves_sent_to_repair(root);
        let proofs = criteria(root);
        let before = log_of(root);
        let said = before.get(proofs[0]).and_then(|e| e.int("origin")).unwrap();
        id_of(&write(
            root,
            "x",
            "criterion",
            json!({"replaces": proofs[0], "when": "a onda roda de novo", "then": "a suíte passa", "proof": "git --version",
                   "form": "ubiquitous", "origin": said}),
        ));
        let sends_before = before.events.iter().filter(|e| e.event_type == "send").count();

        let next = round(root);
        assert_eq!(next["ok"], json!(true), "{next}");
        let sent: Vec<u64> = next["dispatch"].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect();
        assert_eq!(sent, vec![3], "só a que ainda vale sai ao conserto, na vaga que o cancelamento liberou: {next}");

        let log = log_of(root);
        let released: Vec<(u64, u64)> = log
            .events
            .iter()
            .filter(|e| e.event_type == "validation_failure" && e.str_field("result") == Some("released"))
            .map(|e| (e.wave().unwrap_or_default(), e.int("expired").unwrap_or_default()))
            .collect();
        assert_eq!(released, vec![(4, fourth), (1, first), (2, second)], "só as vencidas saem: {next}");
        assert_eq!(log.last_rejected().into_iter().collect::<Vec<_>>(), vec![(3, third)], "{next}");
        assert_eq!(log.unanswered_sends().into_keys().collect::<Vec<_>>(), vec![3], "o envio das vencidas fecha e só o da que vale fica: {next}");
        let cancelled: Vec<u64> =
            log.events.iter().filter(|e| e.event_type == "delivered" && e.str_field("author") == Some("binary")).filter_map(|e| e.wave()).collect();
        assert_eq!(cancelled, vec![1, 2], "{next}");
        assert_eq!(log.events.iter().filter(|e| e.event_type == "send").count(), sends_before + 1, "só a onda 3 ganhou envio: {next}");
        assert!(crate::commands::flow::round::waves_stuck(&log).is_empty(), "{next}");
    }

    /// O limite de consertos conta as reprovações que ainda valem: a vencida e
    /// a soltura dela não entram na conta nem a interrompem, e a que vem
    /// depois completa o limite com as de antes.
    #[test]
    fn the_repair_limit_does_not_count_an_expired_verdict() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 1);
        let failed = ("criterion-failed", "A prova caiu.");
        for _ in 0..2 {
            verdict(root, 1, failed, json!({"items": []}));
        }
        let lost = verdict(root, 1, ("criterion-passed-before", "Já passava."), json!({}));
        let release = json!({"wave": 1, "final": true, "result": "released", "text": "Venceu.", "reason": "criterion-passed-before",
            "fingerprint": "impressao", "expired": lost, "author": "binary"});
        record(root, "x", "validation_failure", release.as_object().cloned().unwrap(), PhaseWriter::Binary).unwrap();
        assert!(crate::commands::flow::round::waves_stuck(&log_of(root)).is_empty(), "duas reprovações valendo ainda não param a onda");

        verdict(root, 1, failed, json!({"items": []}));
        assert_eq!(crate::commands::flow::round::waves_stuck(&log_of(root)).keys().copied().collect::<Vec<_>>(), vec![1]);
    }

    /// A reprovação que o fechamento grava leva a versão vigente de cada item
    /// cuja prova caiu e, na prova que já passava antes, o código de antes da
    /// obra; com uma falha que não é prova de item, `items` fica vazio.
    #[test]
    fn a_new_verdict_records_the_item_version_and_the_code_before() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 1);
        branched_from_dev(root);
        let log = log_of(root);
        let code = log.codes().get(&criteria(root)[0]).cloned().unwrap();
        let version = criteria(root)[0];
        let base = crate::commands::flow::validation::base_commit(root, &log).expect("o código de antes");

        let passed_before = expiry_fields(root, &log, &[(Some(code.as_str()), true)]);
        assert_eq!(passed_before.get("items"), Some(&json!([version])));
        assert_eq!(passed_before.get("before"), Some(&json!(base)));
        let failed = expiry_fields(root, &log, &[(Some(code.as_str()), false)]);
        assert_eq!(failed.get("items"), Some(&json!([version])));
        assert_eq!(failed.get("before"), None, "só a prova que já passava antes grava o código de antes");
        let mixed = expiry_fields(root, &log, &[(Some(code.as_str()), false), (None, false)]);
        assert_eq!(mixed.get("items"), Some(&json!([])), "falha que não é de item não vence por item");
    }
}
