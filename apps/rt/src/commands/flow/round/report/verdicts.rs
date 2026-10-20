//! O veredito oficial de cada volta do revisor, ensaiado antes de qualquer
//! gravação, com os itens que a revisão final não cumpriu.

use mustard_core::domain::spec_events::Refusal;
use serde_json::{Value, json};

use super::super::agreed::Unmet;
use super::super::rehearsal::{Rehearsed, rehearse};
use super::{VerdictReport, replaced, settle_verdict};
use crate::commands::spec_events::write::RecordCheck;

/// Cada veredito ensaiado, com a onda dele, e os itens que a revisão final
/// não cumpriu.
type Rehearsals = (Vec<(Option<u64>, Rehearsed)>, Vec<Unmet>);

/// Ensaia em `check` o veredito oficial de cada volta do revisor de
/// `verdicts`: os critérios citados existem, antes de qualquer gravação.
/// Devolve cada veredito ensaiado, com a onda dele, e os itens que a revisão
/// final não cumpriu. A triagem desses itens lê a spec como este ensaio a
/// deixa, na rodada e na pergunta que espera fora das travas.
pub(in crate::commands::flow::round) fn rehearse_verdicts(check: &mut RecordCheck, verdicts: &[VerdictReport]) -> Result<Rehearsals, Refusal> {
    let mut rehearsed = Vec::new();
    let mut review_unmet: Vec<Unmet> = Vec::new();
    for verdict in verdicts {
        let mut draft = verdict.fields.clone();
        review_unmet.extend(settle_verdict(check.log(), &mut draft)?);
        // A revisão final não aponta onda: ela responde pelo combinado
        // inteiro, não por uma onda dele. Só a revisão de uma onda usa a
        // última onda do plano quando o veredito não diz qual.
        let is_final = draft.get("final") == Some(&Value::Bool(true));
        let wave = if is_final { verdict.wave } else { verdict.wave.or_else(|| check.log().planned_waves().last().copied()) };
        if let Some(wave) = wave {
            draft.insert("wave".into(), json!(wave));
        }
        // O veredito oficial aponta em `replaces` todas as voltas do revisor
        // desde o pedido de revisão: é ele que fecha o pedido.
        if let Some(returns) = replaced(&verdict.returns) {
            draft.insert("replaces".into(), returns);
        }
        draft.insert("author".into(), json!("review"));
        rehearsed.push((wave, rehearse(check, "verdict", draft)?));
    }
    Ok((rehearsed, review_unmet))
}
