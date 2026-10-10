//! Read-only projection shared by mods and the statusline. The event logs
//! remain authoritative; opening a panel creates no page, scan or model turn.

pub mod cli;
mod jev_usage;
mod consumption;
mod publication;
#[cfg(test)]
pub(crate) use publication::prepare as prepare_publication;

use std::collections::BTreeSet;
use std::path::Path;

use mustard_core::domain::spec_events::{Hidden, SpecLog};
use mustard_core::domain::spec_state::{State, final_approval};
use mustard_core::io::{spec_index, spend};
use serde_json::{Value, json};

pub(crate) fn snapshot(start: &Path, selected: Option<&str>) -> Value {
    snapshot_session(start,selected,None)
}

pub(crate) fn snapshot_session(start: &Path, selected: Option<&str>, session:Option<&str>) -> Value {
    let root = mustard_core::io::spec_events::spec_root(start);
    let selected = selected.map(str::to_string).or_else(|| crate::shared::context::checkout::current_spec(&start.to_string_lossy()));
    let jev = jev_usage::Ledger::read(&root);
    let costs = consumption::costs(&root);
    let mut specs: Vec<Value> =
        spec_index::read_specs(&root).into_iter().map(|(name, log)| spec_view(&root, &name, &log, selected.as_deref() == Some(&name), &jev)).collect();
    for spec in &mut specs {
        let name=spec["name"].as_str().unwrap_or_default().to_string();
        spec["usage"]["claude_cost"]=json!({"known_micro_usd":costs["by_spec"][&name],"basis":costs["basis"],
            "coverage":"observed-same-spec-intervals-only","billed_micro_usd":null});
    }
    specs.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let branch = mustard_core::platform::git::run(start, &["branch", "--show-current"]).out();
    json!({"ok":true,"schema_version":1,"at":chrono::Utc::now().to_rfc3339(),
        "project":{"language":crate::commands::spec_events::project(&root).lang.to_string(),"version":mustard_core::harness_version(),"publication_url":spec_index::project_page_url(&root),"name":spend::project_name(&root).or_else(||root.file_name().map(|n|n.to_string_lossy().into_owned())),"branch":branch},
        "selected_spec":selected,"specs":specs,
        "jev":jev.project(),"claude_cost":costs,"consumption":consumption::history(&root,spend::machine_dir().as_deref()),
        "statusline":consumption::statusline(&root,session),
        "commands":{"panel":"/mustard-panel","publish":"/mustard-pages"}})
}

pub(crate) fn observe_statusline(data:&Value,gain:Option<&crate::shared::rtk_gain::RtkGain>) {
    consumption::observe_statusline(data,gain);
}

fn spec_view(root: &Path, name: &str, log: &SpecLog, selected: bool, jev: &jev_usage::Ledger) -> Value {
    let visible = log.visible();
    let hidden = log.hidden();
    let waves: Vec<Value> = log
        .planned_waves()
        .into_iter()
        .map(|wave| {
            let sent = visible.iter().rev().find(|e| e.event_type == "send" && e.wave() == Some(wave));
            let integrated = visible.iter().rev().find(|e| e.event_type == "delivered" && e.wave() == Some(wave));
            let commit = visible.iter().rev().find(|e| e.event_type == "commit" && e.ints("waves").contains(&wave));
            let received =
                log.events.iter().rev().find(|e| e.event_type == "delivered" && e.wave() == Some(wave) && hidden.get(&e.id) == Some(&Hidden::Returned));
            let repair = log.last_rejected().get(&wave).is_some_and(|id| integrated.is_none_or(|e| e.id < *id));
            let status = if repair {
                "repair-required"
            } else if commit.is_some() {
                "committed"
            } else if integrated.is_some() {
                "integrated"
            } else if received.is_some() {
                "received"
            } else if sent.is_some() {
                "running"
            } else {
                "planned"
            };
            json!({"wave":wave,"status":status,"commit":commit.and_then(|e|e.str_field("sha")),
            "summary":integrated.and_then(|e|e.str_field("text")),
            "tokens":sent.and_then(|e|e.int("tokens")),"model":sent.and_then(|e|e.str_field("model_used")),
            "usage_breakdown":sent.and_then(|e|e.fields.get("usage_breakdown")),
            "configured_model":sent.and_then(|e|e.str_field("model")),"effort":sent.and_then(|e|e.str_field("effort"))})
        })
        .collect();
    // Conductor usage is cumulative and repeated on wave sends. Take it once.
    let caller = visible.iter().filter(|e| e.event_type == "send").filter_map(|e| e.int("caller_tokens")).max();
    let unknown = waves.iter().filter(|w| w["tokens"].as_u64().is_none()).count();
    let tokens = if unknown == 0 && !waves.is_empty() { Some(waves.iter().filter_map(|w| w["tokens"].as_u64()).sum::<u64>()) } else { None };
    let mut breakdown=mustard_core::io::transcript::TokenBreakdown::default();
    let mut detail_unknown=0;
    for wave in &waves {
        if let Ok(detail)=serde_json::from_value::<mustard_core::io::transcript::TokenBreakdown>(wave["usage_breakdown"].clone()) {breakdown.combine(&detail);}else{detail_unknown+=1;}
    }
    let caller_breakdown=visible.iter().filter(|e|e.event_type=="send").max_by_key(|e|e.int("caller_tokens")).and_then(|e|e.fields.get("caller_usage_breakdown"));
    let stages: Vec<Value> = visible
        .iter()
        .filter(|e| e.event_type == "stage_run")
        .map(|e| {
            json!({
                "stage":e.str_field("stage"),"phase":e.str_field("phase"),"result":e.str_field("result"),"ms":e.int("ms"),"at":e.at(),
                "scope":e.str_field("scope"),"attempt":e.int("attempt"),"version":e.str_field("version")
            })
        })
        .collect();
    let publication=visible.iter().rev().find(|e|e.event_type=="publish" && e.str_field("page")==Some("spec")
        && e.fields.get("ok").and_then(Value::as_bool)==Some(true)).map(|event|json!({
            "url":event.str_field("url"),"snapshot_id":event.str_field("snapshot_id"),"at":event.at(),"provider":event.str_field("provider")}));
    let c = mustard_core::ProjectConfig::load(root).commands();
    let missing: BTreeSet<&str> = ["buildCommand", "lintCommand", "testCommand"]
        .into_iter()
        .filter(|key| match *key {
            "buildCommand" => c.build.is_none(),
            "lintCommand" => c.lint.is_none(),
            _ => c.test.is_none(),
        })
        .collect();
    json!({"name":name,"goal":mustard_core::domain::spec_index::goal_of(log),"phase":State::from_log(log).phase,
        "waves":waves,"scheduling":super::flow::round::scheduling_snapshot(root,log),"usage":{"wave_tokens":tokens,"conductor_tokens":caller,"waves_with_unknown_usage":unknown,
            "known_wave_breakdown":breakdown,"waves_with_unknown_breakdown":detail_unknown,"conductor_breakdown":caller_breakdown,"origin":"deduplicated-transcripts"},
        "stages":stages,"final_validation_valid":if selected {Some(super::flow::validation::reusable(root,log))} else {None},
        "counted_waves":wave_counts(log),
        "publication":publication,"jev":jev.spec(name),"review_approved":final_approval(log).is_some(),"undeclared_commands":missing})
}

/// The same progress used by the terminal and the full local projection.
pub(crate) fn wave_counts(log: &SpecLog) -> (usize, usize) {
    let counted = log.counted_waves();
    (log.delivered_waves().intersection(&counted).count(), counted.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_an_empty_project_is_read_only_and_reports_unknown_cost() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), "{}").unwrap();
        let result = snapshot(dir.path(), None);
        assert_eq!(result["specs"], json!([]));
        assert_eq!(result["jev"]["cost_micro_usd"], Value::Null);
        assert!(!dir.path().join(".claude").exists());
    }
}
