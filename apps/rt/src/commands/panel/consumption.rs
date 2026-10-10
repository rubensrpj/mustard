//! Host cost receipts and historical token projection. Session totals and
//! per-spec deltas are distinct; neither is added to wave usage twice.
use std::collections::BTreeMap;
use std::path::Path;

use mustard_core::io::fs::lock::{LockedFile, read_shared};
use mustard_core::io::{sha256::Sha256, spend};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Receipt {
    last_micro_usd: Option<u64>,
    total_micro_usd: u64,
    unattributed_micro_usd: u64,
    by_spec: BTreeMap<String, u64>,
    last_spec: Option<String>,
    measured_at: String,
    first_at: String,
    model: Option<String>,
}

fn folder(root: &Path) -> std::path::PathBuf {
    root.join(".claude/mustard/usage/sessions")
}

fn statusline_path(root: &Path, session: &str) -> std::path::PathBuf {
    let mut hash = Sha256::new();
    hash.update(session.as_bytes());
    root.join(".claude/mustard/usage/statusline")
        .join(format!("{}.json", hash.hex_digest()))
}

pub(super) fn observe_statusline(data: &Value, gain: Option<&crate::shared::rtk_gain::RtkGain>) {
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
        "api_duration_ms":data["cost"]["total_api_duration_ms"],"context":data["context_window"],
        "rtk":gain.map(|gain|json!({"saved_tokens":gain.saved,"percent":gain.pct}))});
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

fn read(text: &str) -> Result<Receipt, String> {
    if text.trim().is_empty() {
        Ok(Receipt::default())
    } else {
        serde_json::from_str(text).map_err(|_| "usage-receipt-unreadable".into())
    }
}

fn apply(
    receipt: &mut Receipt,
    measurement: &Value,
    spec: Option<&str>,
    at: &str,
) -> Result<(), String> {
    let Some(cost) = measurement["cost"]["usd"].as_f64() else {
        return Ok(());
    };
    if !cost.is_finite() || cost < 0.0 || cost > 1_000_000.0 {
        return Err("usage-invalid-cost".into());
    }
    let cost = (cost * 1_000_000.0).round() as u64;
    let delta = match receipt.last_micro_usd {
        Some(previous) if cost >= previous => cost - previous,
        Some(_) => return Err("usage-out-of-order-or-reset".into()),
        None => cost,
    };
    receipt.total_micro_usd = receipt.total_micro_usd.saturating_add(delta);
    // An interval crossing branches and the initial nonzero baseline cannot
    // be attributed safely. Browsing another spec never changes ownership.
    if receipt.last_micro_usd.is_some()
        && receipt.last_spec.as_deref() == spec
        && let Some(spec) = spec
    {
        let total = receipt.by_spec.entry(spec.into()).or_default();
        *total = total.saturating_add(delta);
    } else {
        receipt.unattributed_micro_usd = receipt.unattributed_micro_usd.saturating_add(delta);
    }
    receipt.last_micro_usd = Some(cost);
    receipt.last_spec = spec.map(str::to_string);
    if receipt.first_at.is_empty() {
        receipt.first_at = at.into();
    }
    receipt.measured_at = at.into();
    receipt.model = measurement["model"]
        .as_str()
        .map(str::to_string)
        .or_else(|| receipt.model.clone());
    Ok(())
}

pub(super) fn record(start: &Path, session: &str, measurement: &Value) -> Value {
    if session.is_empty() || session.len() > 200 {
        return json!({"ok":false,"reason":"usage-invalid-session"});
    }
    if measurement["cost"]["usd"].as_f64().is_none() {
        return json!({"ok":true,"recorded":false,"reason":"usage-cost-unknown"});
    }
    let root = mustard_core::io::spec_events::spec_root(start);
    if !mustard_core::ProjectConfig::exists(&root) {
        return json!({"ok":true,"recorded":false,"reason":"project-not-initialized"});
    }
    let spec = crate::shared::context::checkout::spec_of_checkout_branch(&start.to_string_lossy());
    let mut hash = Sha256::new();
    hash.update(session.as_bytes());
    let path = folder(&root).join(format!("{}.json", hash.hex_digest()));
    let result = (|| -> Result<(), String> {
        let mut lock = LockedFile::exclusive(&path).map_err(|e| e.to_string())?;
        let mut receipt = read(&lock.read_to_string().map_err(|e| e.to_string())?)?;
        apply(
            &mut receipt,
            measurement,
            spec.as_deref(),
            &chrono::Utc::now().to_rfc3339(),
        )?;
        let mut bytes = serde_json::to_vec(&receipt).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        lock.replace(&bytes).map_err(|e| e.to_string())
    })();
    result.map_or_else(
        |reason| json!({"ok":false,"reason":reason}),
        |()| json!({"ok":true,"recorded":true}),
    )
}

pub(super) fn costs(root: &Path) -> Value {
    let mut by_spec = BTreeMap::<String, u64>::new();
    let mut total = 0u64;
    let mut unattributed = 0u64;
    let mut known = 0;
    let mut unreadable = 0;
    let mut newest = String::new();
    let mut model = None;
    if let Ok(entries) = std::fs::read_dir(folder(root)) {
        for path in entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        {
            let receipt = read_shared(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| read(&text));
            match receipt {
                Ok(receipt) => {
                    known += 1;
                    total = total.saturating_add(receipt.total_micro_usd);
                    unattributed = unattributed.saturating_add(receipt.unattributed_micro_usd);
                    for (name, cost) in receipt.by_spec {
                        let sum = by_spec.entry(name).or_default();
                        *sum = sum.saturating_add(cost);
                    }
                    if receipt.measured_at > newest {
                        newest = receipt.measured_at;
                        model = receipt.model;
                    }
                }
                Err(_) => unreadable += 1,
            }
        }
    }
    json!({"known_micro_usd":if known>0{Some(total)}else{None},"unattributed_micro_usd":if known>0{Some(unattributed)}else{None},
        "by_spec":by_spec,"observed_sessions":known,"unreadable_sessions":unreadable,"measured_at":if newest.is_empty(){None}else{Some(newest)},
        "model":model,"basis":"host-api-equivalent-estimate","coverage":"observed-sessions-only","billed_micro_usd":null})
}

pub(super) fn history(root: &Path, dir: Option<&Path>) -> Value {
    let Some(dir) = dir else {
        return json!({"available":false,"reason":"no-machine-folder"});
    };
    let ledger = match spend::load(dir) {
        Ok(ledger) => ledger,
        Err(error) => return json!({"available":false,"reason":error.reason()}),
    };
    let own_dir = root.join(".claude/mustard/usage/project-history");
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
        "project_days":own,"project_scope":"canonical-project-root","origin":"deduplicated-transcripts","cost_micro_usd":null})
}

/// Cache exact project history separately from the legacy machine ledger,
/// whose display names can merge unrelated projects of the same basename.
/// Closed days remain cached; only new days and today's partial rows recount.
pub(super) fn refresh_history(
    start: &Path,
    machine: &crate::commands::spec::spend::Machine,
) -> Value {
    let Some(config) = machine.config.as_deref() else {
        return json!({"ok":false,"reason":"no-machine-folder"});
    };
    let root = mustard_core::io::spec_events::spec_root(start);
    if !mustard_core::ProjectConfig::exists(&root) {
        return json!({"ok":true,"recorded":false,"reason":"project-not-initialized"});
    }
    let dir = root.join(".claude/mustard/usage/project-history");
    let result = (|| -> Result<(), mustard_core::domain::spend::Refusal> {
        let range = spend::load(&dir)?.to_count(&machine.today);
        let closed = range
            .as_ref()
            .map(|range| spend::project_days(config, &root, range));
        let range_today = mustard_core::domain::spend::Range {
            first: Some(machine.today.clone()),
            last: machine.today.clone(),
        };
        let open = spend::project_days(config, &root, &range_today)
            .into_iter()
            .map(|row| mustard_core::domain::spend::DayRow {
                partial: true,
                ..row
            })
            .collect::<Vec<_>>();
        spend::update(&dir, |ledger| {
            if let (Some(range), Some(closed)) = (&range, closed) {
                ledger.record_counted(range, closed);
            }
            ledger.open_rows = open;
            ledger.open_day = Some(machine.today.clone());
            ledger.measured_at = Some(chrono::Utc::now().to_rfc3339());
        })?;
        Ok(())
    })();
    result.map_or_else(
        |error| json!({"ok":false,"reason":error.reason()}),
        |()| json!({"ok":true}),
    )
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
    fn duplicate_totals_and_branch_switches_never_double_count_or_misattribute() {
        let mut receipt = Receipt::default();
        for (cost, spec) in [
            (1.0, Some("a")),
            (1.2, Some("a")),
            (1.2, Some("a")),
            (1.4, Some("b")),
            (1.6, Some("b")),
        ] {
            apply(&mut receipt, &json!({"cost":{"usd":cost}}), spec, "now").unwrap();
        }
        assert_eq!(receipt.total_micro_usd, 1_600_000);
        assert_eq!(receipt.by_spec["a"], 200_000);
        assert_eq!(receipt.by_spec["b"], 200_000);
        assert_eq!(receipt.unattributed_micro_usd, 1_200_000);
        assert!(
            apply(
                &mut receipt,
                &json!({"cost":{"usd":0.1}}),
                Some("b"),
                "later"
            )
            .is_err()
        );
        assert_eq!(receipt.total_micro_usd, 1_600_000);
    }
    #[test]
    fn missing_and_invalid_cost_do_not_become_zero_observations() {
        let mut receipt = Receipt::default();
        apply(&mut receipt, &json!({}), Some("a"), "now").unwrap();
        assert_eq!(receipt.last_micro_usd, None);
        assert!(apply(&mut receipt, &json!({"cost":{"usd":-1}}), None, "now").is_err());
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
            &root.path().join(".claude/mustard/usage/project-history"),
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
    #[test]
    fn project_refresh_excludes_another_project_with_the_same_name() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let left = dir.path().join("left/repo");
        let right = dir.path().join("right/repo");
        for root in [&left, &right] {
            std::fs::create_dir_all(root).unwrap();
            std::fs::write(root.join("mustard.json"), "{}").unwrap();
        }
        let folder = config.join("projects/fixture");
        std::fs::create_dir_all(&folder).unwrap();
        let line = |id, cwd: &Path, tokens| {
            json!({"type":"assistant","timestamp":"2026-10-01T12:00:00Z","cwd":cwd,
            "message":{"id":id,"role":"assistant","content":[],"usage":{"input_tokens":tokens,"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}).to_string()
        };
        std::fs::write(
            folder.join("session.jsonl"),
            format!(
                "{}\n{}\n",
                line("left", &left, 100),
                line("right", &right, 900)
            ),
        )
        .unwrap();
        let machine = crate::commands::spec::spend::Machine {
            config: Some(config),
            dir: Some(dir.path().join("machine")),
            today: spend::today(),
        };
        assert_eq!(refresh_history(&left, &machine)["ok"], true);
        assert_eq!(
            spend::load(&left.join(".claude/mustard/usage/project-history"))
                .unwrap()
                .rows[0]
                .tokens,
            100
        );
    }
}
