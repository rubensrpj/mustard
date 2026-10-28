//! As marcas de conserto que o fechamento gravou sem prova.
//!
//! Antes do conserto só com prova, o lint, a suíte ou a análise de
//! declarações sem uso do fechamento que caía reprovava toda onda do plano,
//! citasse a saída arquivo dela ou não. Essas marcas seguravam a obra: o
//! fechamento recusava a onda reprovada, a rodada a despachava para o
//! conserto, e só uma entrega vazia de cada uma a soltava. No começo do
//! fechamento e da rodada, a marca cuja saída gravada não cita arquivo da
//! onda sai sozinha, pela mesma leitura que decide quem reabre
//! ([`crate::commands::flow::validation::cited`]). As ondas com o conserto
//! pendente vêm de quem chama, pela leitura da fila da rodada
//! (`waves_pending_fix`): esta parte comum não importa a rodada.

use std::collections::BTreeMap;
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

/// Solta cada onda de `pending` — a onda com o conserto pendente e a marca
/// que a reprovou — cuja reprovação é uma marca da
/// compilação, do lint, da suíte ou da análise de declarações sem uso do fechamento e cuja
/// saída gravada não cita arquivo que a obra mudou e que a onda declarou ou
/// entregou. A soltura é uma marca nova, do binário, com o resultado
/// `released`, a mesma impressão e o mesmo motivo da reprovação e o texto que
/// diz por que saiu: a mais nova da onda, ela a tira da leitura das ondas
/// reprovadas sem apagar nada. O fechamento a chama antes de conferir a obra,
/// e a rodada antes de escolher as ondas que voltam ao conserto: as duas leem
/// a mesma soltura. Devolve as ondas soltas, na ordem do plano.
pub(crate) fn release(root: &Path, spec: &str, log: &SpecLog, pending: BTreeMap<u64, u64>, lang: Locale) -> Result<Vec<u64>, Refusal> {
    let marks: Vec<(u64, &SpecEvent)> = pending
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
