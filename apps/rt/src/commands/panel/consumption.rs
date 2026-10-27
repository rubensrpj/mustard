//! A medição de custo que o anfitrião manda, gravada no recibo da sessão
//! ([`crate::shared::session_costs`]), a linha de status e a projeção
//! histórica de tokens.
use std::path::Path;

use mustard_core::io::fs::lock::read_shared;
use mustard_core::io::{sha256::Sha256, spend};
use serde_json::{Value, json};

pub(super) use crate::shared::session_costs::costs;

fn statusline_path(root: &Path, session: &str) -> std::path::PathBuf {
    let mut hash = Sha256::new();
    hash.update(session.as_bytes());
    root.join(".claude/mustard/usage/statusline")
        .join(format!("{}.json", hash.hex_digest()))
}

pub(super) fn observe_statusline(data: &Value) {
    let Some(session) = data["session_id"].as_str() else {
        return;
    };
    let Some(cwd) = data["workspace"]["current_dir"]
        .as_str()
        .or_else(|| data["cwd"].as_str())
    else {
        return;
    };
    let root = mustard_core::io::spec_events::spec_root(Path::new(cwd));
    if !mustard_core::ProjectConfig::exists(&root) {
        return;
    }
    let view = json!({"measured_at":chrono::Utc::now().to_rfc3339(),"origin":"host-statusline",
        "model":data["model"]["display_name"],"duration_ms":data["cost"]["total_duration_ms"],
        "api_duration_ms":data["cost"]["total_api_duration_ms"],"context":data["context_window"]});
    if let Ok(bytes) = serde_json::to_vec(&view) {
        let _ = mustard_core::io::fs::write_atomic(statusline_path(&root, session), &bytes);
    }
}

pub(super) fn statusline(root: &Path, session: Option<&str>) -> Value {
    session
        .and_then(|session| read_shared(&statusline_path(root, session)).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

pub(super) fn record(start: &Path, session: &str, measurement: &Value) -> Value {
    if session.is_empty() || session.len() > 200 {
        return json!({"ok":false,"reason":"usage-invalid-session"});
    }
    let root = mustard_core::io::spec_events::spec_root(start);
    if !mustard_core::ProjectConfig::exists(&root) {
        return json!({"ok":true,"recorded":false,"reason":"project-not-initialized"});
    }
    let live=super::live_usage::record(start,session,measurement);
    if measurement["cost"]["usd"].as_f64().is_none() {
        return json!({"ok":live["ok"],"recorded":live["recorded"],"cost_recorded":false,"reason":"usage-cost-unknown","live":live});
    }
    let spec = crate::shared::context::checkout::spec_of_checkout_branch(&start.to_string_lossy());
    crate::shared::session_costs::add(&root, session, measurement, spec.as_deref()).map_or_else(
        |reason| json!({"ok":false,"reason":reason}),
        |()| json!({"ok":true,"recorded":true,"live":live}),
    )
}

pub(super) fn history(root: &Path, dir: Option<&Path>) -> Value {
    let Some(dir) = dir else {
        return json!({"available":false,"reason":"no-machine-folder"});
    };
    let ledger = match spend::load(dir) {
        Ok(ledger) => ledger,
        Err(error) => return json!({"available":false,"reason":error.reason()}),
    };
    let own_dir = spend::project_history_dir(root);
    let own_ledger = match spend::load(&own_dir) {
        Ok(ledger) => ledger,
        Err(error) => return json!({"available":false,"reason":error.reason()}),
    };
    let today = spend::today();
    let rows: Vec<_> = ledger
        .rows
        .iter()
        .chain(ledger.open_rows.iter().filter(|row| row.day == today))
        .collect();
    let own: Vec<_> = own_ledger
        .rows
        .iter()
        .chain(own_ledger.open_rows.iter().filter(|row| row.day == today))
        .collect();
    let sum = |rows: &[&mustard_core::domain::spend::DayRow]| {
        rows.iter()
            .fold(0u64, |total, row| total.saturating_add(row.tokens))
    };
    json!({"available":own_ledger.counted_through.is_some(),"project_tokens":sum(&own),"machine_tokens":if ledger.counted_through.is_some(){Some(sum(&rows))}else{None},
        "counted_through":own_ledger.counted_through,"measured_at":own_ledger.measured_at,"machine_measured_at":ledger.measured_at,
        "today_partial":true,"today_measured":own_ledger.measured_at.is_some() && own_ledger.open_day.as_deref()==Some(today.as_str()),
        "project_days":own,"project_scope":"canonical-project-root","origin":"deduplicated-transcripts","cost_micro_usd":null,
        "trend":mustard_core::domain::spend::trend(&ledger.rows).map(|trend|json!({"change_percent":trend.change_percent}))})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn global_measure_event_does_not_initialize_an_unrelated_project() {
        let root = tempfile::tempdir().unwrap();
        let result = record(root.path(), "session", &json!({"cost":{"usd":1.0}}));
        assert_eq!(result["ok"], true);
        assert_eq!(result["recorded"], false);
        assert_eq!(result["reason"], "project-not-initialized");
        assert!(!root.path().join(".claude").exists());
    }
    #[test]
    fn historical_project_and_machine_totals_have_different_scopes() {
        let dir = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let name = spend::project_name(root.path()).unwrap();
        let row = |project, tokens| mustard_core::domain::spend::DayRow {
            project,
            day: "2026-10-01".into(),
            tokens,
            ..Default::default()
        };
        spend::update(dir.path(), |ledger| {
            ledger.rows = vec![row(name, 100), row("another".into(), 900)];
            ledger.counted_through = Some("2026-10-01".into());
        })
        .unwrap();
        spend::update(
            &spend::project_history_dir(root.path()),
            |ledger| {
                ledger.rows = vec![row("actual-project".into(), 100)];
                ledger.counted_through = Some("2026-10-01".into());
            },
        )
        .unwrap();
        let result = history(root.path(), Some(dir.path()));
        assert_eq!(result["project_tokens"], 100);
        assert_eq!(result["machine_tokens"], 1000);
    }
}
