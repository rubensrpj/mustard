//! As perguntas que as duas portas antigas da aprovação, o modo de plano e o
//! comando de barra, ainda fazem enquanto aprovam: qual spec espera
//! aprovação e se ela já foi aprovada, lidas do `meta.json` e do registro de
//! eventos da spec. A resposta à pergunta com opções passou para a
//! testemunha ([`super::approval_witness`]), que lê o estado do
//! `spec.ndjson`.

use mustard_core::domain::model::contract::HookInput;
use mustard_core::io::fs;
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use mustard_core::ClaudePaths;
use serde_json::Value;
use std::path::Path;

/// A spec que espera aprovação nesta sessão: a spec atual, pela escada
/// única, quando ela está em plano e ainda não foi aprovada; senão, a única
/// spec do projeto nessa situação. `None` na dúvida.
pub(crate) fn active_spec(cwd: &str, input: &HookInput) -> Option<String> {
    crate::shared::spec_state::active_spec(cwd, input.session_id.as_deref())
        .filter(|spec| awaiting(cwd, spec))
        .or_else(|| unique_pending_plan(cwd))
}

fn awaiting(cwd: &str, spec: &str) -> bool {
    is_awaiting_approval(cwd, spec) && !already_approved(cwd, spec)
}

/// A única spec em plano e sem aprovação; nenhuma ou mais de uma dá `None`.
fn unique_pending_plan(cwd: &str) -> Option<String> {
    let spec_dir = ClaudePaths::for_project(Path::new(cwd)).ok()?.spec_dir();
    let mut pending = fs::read_dir(&spec_dir)
        .ok()?
        .into_iter()
        .filter(|e| e.is_dir)
        .map(|e| e.file_name)
        .filter(|name| awaiting(cwd, name));
    let first = pending.next()?;
    if pending.next().is_some() {
        return None;
    }
    Some(first)
}

/// A spec está no estágio `Plan` do `meta.json`.
pub(crate) fn is_awaiting_approval(cwd: &str, spec: &str) -> bool {
    let Some(sp) = ClaudePaths::for_project(Path::new(cwd))
        .and_then(|p| p.for_spec(spec))
        .ok()
    else {
        return false;
    };
    let Some(meta) = mustard_core::read_meta(&sp.meta_json_path()) else {
        return false;
    };
    meta.stage
        .as_deref()
        .map(|s| s.trim().eq_ignore_ascii_case("Plan"))
        .unwrap_or(false)
}

/// O registro de eventos da spec já tem a aprovação do `approve-spec`.
pub(crate) fn already_approved(cwd: &str, spec: &str) -> bool {
    let Some(events_dir) = ClaudePaths::for_project(Path::new(cwd))
        .and_then(|p| p.for_spec(spec))
        .ok()
        .map(|sp| sp.events_dir())
    else {
        return false;
    };
    read_harness_events_from_ndjson_dir(&events_dir).iter().any(|ev| {
        ev.event == "pipeline.status"
            && ev.payload.get("to").and_then(Value::as_str) == Some("approved")
    })
}
