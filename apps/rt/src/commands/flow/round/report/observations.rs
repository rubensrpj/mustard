//! Resolve native provenance before storing a return; retain it only on the
//! delivery it belongs to, never on other waves closed by a repair.
use std::path::Path;

use mustard_core::domain::spec_events::{Refusal, SpecLog};
use serde_json::{Map, Value, json};

use super::{RoundRefusal, WaveReport};

pub(super) fn check(
    root: &Path,
    spec: &str,
    log: &SpecLog,
    sent: u64,
    wave: u64,
    draft: &mut Map<String, Value>,
) -> Result<(), RoundRefusal> {
    if let Some(references) = draft.get("observations") {
        let send = log
            .get(sent)
            .ok_or(RoundRefusal::Refused(Refusal::NoOpenSend { wave }))?;
        let resolved = mustard_core::io::observations::resolve(
            root,
            spec,
            log.dispatch_position(sent),
            send.at(),
            references,
        )
        .map_err(|reason| {
            RoundRefusal::Refused(Refusal::MissingField {
                event_type: "delivered".into(),
                field: format!("observations: {reason}"),
            })
        })?;
        draft.insert("observations".into(), resolved);
    }
    draft.insert("report_status".into(), json!("reported-unverified"));
    Ok(())
}

pub(super) fn carry(log: &SpecLog, report: &WaveReport, wave: u64, draft: &mut Map<String, Value>) {
    draft.insert("report_status".into(), json!("reported-unverified"));
    if wave == report.wave
        && let Some(observations) = report
            .returns
            .last()
            .and_then(|id| log.get(*id))
            .and_then(|event| event.fields.get("observations"))
    {
        draft.insert("observations".into(), observations.clone());
    }
}
