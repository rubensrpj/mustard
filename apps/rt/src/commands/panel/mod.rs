//! Read-only projection shared by mods and the statusline. The event logs
//! remain authoritative; opening a panel creates no page, scan or model turn.

pub mod cli;
mod jev_usage;
mod knowledge_usage;
mod consumption;
mod publication;
mod wave_usage;
mod settings;
mod dashboard;
mod preferences;
mod impact;
mod diagnostics;
mod live_usage;
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
    let mut jev_view=jev.project();
    let config=mustard_core::ProjectConfig::load(&root);
    jev_view["configuration"]=jev_usage::configuration(&root,&config,std::env::var(mustard_core::io::jev_gate::KEY_ENV).ok());
    let integration_health=crate::shared::integration_health::snapshot(&root);
    json!({"ok":true,"schema_version":1,"at":chrono::Utc::now().to_rfc3339(),
        "project":{"language":crate::commands::spec_events::project(&root).lang.to_string(),"version":mustard_core::harness_version(),"publication_url":spec_index::project_page_url(&root),"name":spend::project_name(&root).or_else(||root.file_name().map(|n|n.to_string_lossy().into_owned())),"branch":branch},
        "selected_spec":selected,"specs":specs,
        "jev":jev_view,"rtk":crate::shared::rtk_gain::cached(&root),"claude_cost":costs,"consumption":consumption::history(&root,spend::machine_dir().as_deref()),
        "retrieval":{"vectors_enabled":config.ai_vectors_enabled(),"index_present":mustard_core::io::project_map::model_path(&root).is_file()},
        "knowledge":knowledge_usage::snapshot(&root),
        "hosts":integration_health["hosts"],
        "integration_health":integration_health,
        "installation":std::fs::read_to_string(root.join(".claude/mustard/installation.json")).ok().and_then(|text|serde_json::from_str::<Value>(&text).ok()),
        "statusline":consumption::statusline(&root,session),
        "commands":{"panel":"/mustard-panel","publish":"/mustard-pages"}})
}

pub(crate) fn observe_statusline(data:&Value) {
    consumption::observe_statusline(data);
}

fn spec_view(root: &Path, name: &str, log: &SpecLog, selected: bool, jev: &jev_usage::Ledger) -> Value {
    spec_view_mode(root,name,log,selected,jev,"all")
}

fn spec_view_mode(root: &Path, name: &str, log: &SpecLog, selected: bool, jev: &jev_usage::Ledger, section:&str) -> Value {
    let visible = log.visible();
    let live = if matches!(section,"all"|"usage") {wave_usage::read(root,name)}else{Value::Null};
    let hidden = log.hidden();
    let planned=log.planned_waves();
    let dispatches=log.last_dispatch_by_wave();
    let rejected=log.last_rejected();
    let measured_wave=|wave:u64|dispatches.get(&wave).and_then(|dispatch|wave_usage::current_at(&live,wave,*dispatch));
    let waves: Vec<Value> = planned.iter().copied()
        .map(|wave| {
            let measured=measured_wave(wave);
            let sent = visible.iter().rev().find(|e| e.event_type == "send" && e.wave() == Some(wave));
            let attempt=dispatches.get(&wave).copied();
            let belongs=|id:u64|attempt.is_none_or(|start|id>start);
            let integrated = visible.iter().rev().find(|e| e.event_type == "delivered" && e.wave() == Some(wave) && belongs(e.id));
            let commit = visible.iter().rev().find(|e| e.event_type == "commit" && e.ints("waves").contains(&wave) && belongs(e.id));
            let received =
                log.events.iter().rev().find(|e| e.event_type == "delivered" && e.wave() == Some(wave) && hidden.get(&e.id) == Some(&Hidden::Returned) && belongs(e.id));
            let repair = rejected.get(&wave).is_some_and(|id| belongs(*id) && integrated.is_none_or(|e| e.id < *id));
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
            "tokens":measured.and_then(|u|u["tokens"].as_u64()).or_else(||sent.and_then(|e|e.int("tokens"))),
            "model":measured.and_then(|u|u["model_used"].as_str()).or_else(||sent.and_then(|e|e.str_field("model_used"))),
            "usage_breakdown":measured.and_then(|u|u.get("usage_breakdown")).or_else(||sent.and_then(|e|e.fields.get("usage_breakdown"))),
            "configured_model":sent.and_then(|e|e.str_field("model")),"effort":sent.and_then(|e|e.str_field("effort")),
            "expertise":sent.and_then(|e|e.fields.get("expertise"))})
        })
        .collect();
    // Conductor usage is cumulative and repeated on wave sends. Take it once.
    let caller = visible.iter().filter(|e| e.event_type == "send").filter_map(|e| e.int("caller_tokens"))
        .chain(planned.iter().filter_map(|wave|measured_wave(*wave)?.get("caller_tokens")?.as_u64())).max();
    let unknown = waves.iter().filter(|w| w["tokens"].as_u64().is_none()).count();
    let tokens = if unknown == 0 && !waves.is_empty() { Some(waves.iter().filter_map(|w| w["tokens"].as_u64()).sum::<u64>()) } else { None };
    let mut breakdown=mustard_core::io::transcript::TokenBreakdown::default();
    let mut detail_unknown=0;
    for wave in &waves {
        if let Ok(detail)=serde_json::from_value::<mustard_core::io::transcript::TokenBreakdown>(wave["usage_breakdown"].clone()) {breakdown.combine(&detail);}else{detail_unknown+=1;}
    }
    let caller_breakdown=visible.iter().filter(|e|e.event_type=="send")
        .filter_map(|e|e.int("caller_tokens").zip(e.fields.get("caller_usage_breakdown")))
        .chain(planned.iter().filter_map(|wave| {
            let usage=measured_wave(*wave)?;
            usage["caller_tokens"].as_u64().zip(usage.get("caller_usage_breakdown"))
        })).max_by_key(|(tokens,_)|*tokens).map(|(_,detail)|detail);
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
        "waves":waves,"scheduling":if selected {super::flow::round::scheduling_snapshot(root,log)} else {Value::Null},"usage":{"wave_tokens":tokens,
            "known_wave_tokens":waves.iter().filter_map(|wave|wave["tokens"].as_u64()).sum::<u64>(),"measured_at":live["measured_at"],
            "conductor_tokens":caller,"waves_with_unknown_usage":unknown,
            "known_wave_breakdown":breakdown,"waves_with_unknown_breakdown":detail_unknown,"conductor_breakdown":caller_breakdown,"origin":"deduplicated-transcripts"},
        "stages":stages,"final_validation_valid":if selected {Some(super::flow::validation::reusable(root,log))} else {None},
        "counted_waves":wave_counts(log),
        "publication":publication,"jev":jev.spec(name),"operations":if matches!(section,"all"|"usage") {operations(root,name,log)}else{Value::Null},
        "definitions":if selected && matches!(section,"all"|"definitions") {definitions(log)} else {Value::Null},
        "review_approved":final_approval(log).is_some(),"undeclared_commands":missing})
}

fn operations(root:&Path,name:&str,log: &SpecLog) -> Value {
    let transport=super::spec_events::conversation::recorded_operations(root,name);
    let calls: Vec<_> = log.visible().into_iter().chain(transport.visible()).filter(|event| event.event_type == "call" && !event.fields.contains_key("request")
        && event.fields.get("invocation").and_then(Value::as_bool)!=Some(false)
        && !matches!(event.str_field("command"),Some("wave assembly"|"wave items"))).collect();
    let mut commands = std::collections::BTreeMap::<String, Value>::new();
    let mut totals = json!({"calls":0,"refusals":0,"input_bytes":0,"output_bytes":0,"estimated_input_tokens":0,"estimated_output_tokens":0,"calls_without_payload_measurement":0,
        "search":{"calls":0,"enriched":0,"task_evidence":0,"fallback":0,"ms":0,
            "decision_coverage":0,"choice_gates":{},"selection_statuses":{},"local_model_calls":0,"remote_model_calls":0,"cache_hits":0,"accepted_choices":0}});
    for call in calls {
        let command = commands.entry(call.str_field("command").unwrap_or_default().into())
            .or_insert_with(|| json!({"calls":0,"refusals":0,"reasons":{},"input_bytes":0,"output_bytes":0,"estimated_input_tokens":0,"estimated_output_tokens":0}));
        command["calls"] = json!(command["calls"].as_u64().unwrap_or(0) + 1);
        totals["calls"] = json!(totals["calls"].as_u64().unwrap_or(0) + 1);
        if call.str_field("result") == Some("refused") {
            command["refusals"] = json!(command["refusals"].as_u64().unwrap_or(0) + 1);
            totals["refusals"] = json!(totals["refusals"].as_u64().unwrap_or(0) + 1);
            let reason = call.str_field("refusal").unwrap_or("unknown");
            command["reasons"][reason] = json!(command["reasons"][reason].as_u64().unwrap_or(0) + 1);
        }
        for field in ["input_bytes", "output_bytes", "estimated_input_tokens", "estimated_output_tokens"] {
            if let Some(value) = call.int(field) { totals[field] = json!(totals[field].as_u64().unwrap_or(0).saturating_add(value));
                command[field]=json!(command[field].as_u64().unwrap_or(0).saturating_add(value)); }
        }
        if call.int("input_bytes").is_none() { totals["calls_without_payload_measurement"] = json!(totals["calls_without_payload_measurement"].as_u64().unwrap_or(0) + 1); }
        if call.str_field("command")==Some("search") {
            totals["search"]["calls"]=json!(totals["search"]["calls"].as_u64().unwrap_or(0)+1);
            totals["search"]["ms"]=json!(totals["search"]["ms"].as_u64().unwrap_or(0).saturating_add(call.int("ms").unwrap_or(0)));
            for (key,field) in [("enriched","search_enriched"),("task_evidence","search_task_evidence"),("fallback","search_fallback")] {
                if call.fields.get(field)==Some(&Value::Bool(true)){totals["search"][key]=json!(totals["search"][key].as_u64().unwrap_or(0)+1);}
            }
            if let Some(gate)=call.str_field("search_choice_gate") {
                totals["search"]["decision_coverage"]=json!(totals["search"]["decision_coverage"].as_u64().unwrap_or(0)+1);
                totals["search"]["choice_gates"][gate]=json!(totals["search"]["choice_gates"][gate].as_u64().unwrap_or(0)+1);
            }
            if let Some(status)=call.str_field("search_selection_status") {
                totals["search"]["selection_statuses"][status]=json!(totals["search"]["selection_statuses"][status].as_u64().unwrap_or(0)+1);
            }
            for (key,field) in [("local_model_calls","search_local_model_calls"),("remote_model_calls","search_remote_model_calls"),("accepted_choices","search_accepted_choices")] {
                if let Some(value)=call.int(field) {totals["search"][key]=json!(totals["search"][key].as_u64().unwrap_or(0)+value);}
            }
            if call.fields.get("search_selection_cached")==Some(&Value::Bool(true)) {totals["search"]["cache_hits"]=json!(totals["search"]["cache_hits"].as_u64().unwrap_or(0)+1);}
        }
    }
    totals["by_command"] = json!(commands);
    totals["token_basis"] = json!("transport-characters-divided-by-four; estimate, not provider usage");
    totals
}

fn safe_value(mut value: Value) -> Value {
    match &mut value {
        Value::String(text) => *text = crate::shared::secret::without_secrets(text),
        Value::Array(values) => { for value in values { *value = safe_value(value.take()); } },
        Value::Object(values) => { for value in values.values_mut() { *value = safe_value(value.take()); } },
        _ => {},
    }
    value
}

fn definitions(log: &SpecLog) -> Value {
    let codes = log.codes();
    let mut groups = std::collections::BTreeMap::<String, Vec<Value>>::new();
    for event in log.visible().into_iter().filter(|event| matches!(event.event_type.as_str(),
        "context" | "concern" | "rule" | "decision" | "limit" | "contract" | "error" | "edge_case" | "out_of_scope" | "criterion" | "task" | "work_type") || event.event_type=="point" && event.str_field("status")==Some("open")) {
        let mut item = display_item(event, &codes);
        if let Some(fields) = item.as_object_mut() { for key in ["v", "at", "author", "search", "keys"] { fields.remove(key); } }
        groups.entry(event.event_type.clone()).or_default().push(safe_value(item));
    }
    json!(groups)
}

fn display_item(event: &mustard_core::domain::spec_events::SpecEvent, codes: &std::collections::BTreeMap<u64,String>) -> Value {
    use mustard_core::domain::spec_events::{Kind,type_spec};
    let mut item=crate::commands::spec_events::shown(event,codes);
    if let Some(spec)=type_spec(&event.event_type) {
        for field in spec.fields {
            if let Some(value)=item.get_mut(field.name) {
                match field.kind {
                    Kind::Ref => { if let Some(code)=value.as_u64().and_then(|id|codes.get(&id)) {*value=json!(code);} },
                    Kind::Refs => { if let Some(values)=value.as_array_mut() { for value in values {
                        if let Some(code)=value.as_u64().and_then(|id|codes.get(&id)) {*value=json!(code);}
                    } } },
                    _ => {},
                }
            }
        }
    }
    item
}

/// Large historical requests are loaded only for the selected wave, never
/// included in every background project summary.
pub(crate) fn wave_detail(start: &Path, name: &str, wave: u64) -> Value {
    let root = mustard_core::io::spec_events::spec_root(start);
    let Some(log) = dashboard::spec_log(&root,name) else {return Value::Null;};
    let visible = log.visible();
    let planned = visible.iter().rev().find(|event| event.event_type == "wave" && event.wave() == Some(wave));
    let sent = visible.iter().rev().find(|event| event.event_type == "send" && event.wave() == Some(wave));
    let codes=log.codes();
    // A usage update replaces a send after its return. Follow replacements
    // to the start of this attempt instead of treating that update as a resend.
    let attempt=sent.map(|event|log.dispatch_position(event.id));
    let received = log.events.iter().rev().find(|event| event.event_type == "delivered" && event.wave() == Some(wave)
        && attempt.is_none_or(|start|event.id>start));
    let steps: Vec<_> = visible.iter().filter(|event| event.event_type == "step" && event.wave() == Some(wave)).map(|event| display_item(event,&codes)).collect();
    let tasks: Vec<_> = visible.iter().filter(|event| event.event_type == "task" && event.wave() == Some(wave)).map(|event| display_item(event,&codes)).collect();
    let criterion_ids: BTreeSet<_> = planned.into_iter().flat_map(|event|event.ints("criteria")).filter_map(|id|log.current(id).map(|event|event.id)).collect();
    let criteria: Vec<_> = visible.iter().filter(|event| event.event_type == "criterion" && criterion_ids.contains(&event.id)).map(|event| display_item(event,&codes)).collect();
    let tests: Vec<_> = visible.iter().filter(|event| {
        let legacy = event.event_type == "criterion_run";
        let native = event.event_type == "stage_run" && event.str_field("stage") == Some("criterion-proof") && event.ints("waves").contains(&wave);
        (legacy || native) && attempt.is_none_or(|start| event.id > start)
            && event.int("criterion").and_then(|id| log.current(id)).is_some_and(|criterion| criterion_ids.contains(&criterion.id))
    })
        .map(|event| {let mut item=display_item(event,&codes);if let Some(code)=event.int("criterion").and_then(|id|codes.get(&id)){item["criterion"]=json!(code);}item}).collect();
    let user_input = planned.and_then(|event| event.int("origin")).and_then(|id| log.get(id)).and_then(|event| event.str_field("text"));
    safe_value(json!({"spec":name,"wave":wave,"plan":planned.map(|event| &event.fields),
        "user_input":user_input,"request":sent.and_then(|event|event.str_field("text")),
        "request_recorded":sent.is_some(),"sent_at":sent.map(|event|event.at()),
        "expertise":sent.and_then(|event|event.fields.get("expertise")),
        "skills":sent.and_then(|event|event.fields.get("skills")),
        "executor_requested":sent.and_then(|event|event.str_field("agent")),
        "delivery":received.map(|event|&event.fields),"delivery_integrated":received.is_some_and(|event|visible.iter().any(|current|current.id==event.id)),
        "observations":crate::shared::tool_observations::list(&root,name,attempt),
        "report_status":"reported-unverified",
        "steps":steps,"tasks":tasks,"criteria":criteria,"tests":tests}))
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
        assert_eq!(result["retrieval"],json!({"vectors_enabled":true,"index_present":false}));
        assert!(!dir.path().join(".claude").exists());
    }

    #[test]
    fn search_decision_coverage_does_not_invent_reasons_for_legacy_calls() {
        let root=tempfile::tempdir().unwrap();
        let rows=[json!({"v":1,"id":1,"type":"call","command":"search","invocation":true}),
            json!({"v":1,"id":2,"type":"call","command":"search","invocation":true,
                "search_choice_gate":"eligible","search_selection_status":"jev-choice",
                "search_local_model_calls":1,"search_remote_model_calls":0,
                "search_selection_cached":true,"search_accepted_choices":2})];
        let log=mustard_core::domain::spec_events::parse_log(&rows.iter().map(Value::to_string).collect::<Vec<_>>().join("\n"));
        let result=operations(root.path(),"demo",&log);
        assert_eq!(result["search"]["calls"],2);
        assert_eq!(result["search"]["decision_coverage"],1);
        assert_eq!(result["search"]["choice_gates"],json!({"eligible":1}));
        assert_eq!(result["search"]["local_model_calls"],1);
        assert_eq!(result["search"]["remote_model_calls"],0);
        assert_eq!(result["search"]["cache_hits"],1);
        assert_eq!(result["search"]["accepted_choices"],2);
    }

    #[test]
    fn selected_wave_exposes_its_exact_request_and_never_borrows_a_previous_attempts_return() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();
        std::fs::write(root.join("mustard.json"),"{}").unwrap();
        let folder=root.join(".claude/spec/demo");std::fs::create_dir_all(&folder).unwrap();
        let input=format!("Pedido exato.\n{}\nFinal.","a".repeat(9000));
        let rows=[
            json!({"v":1,"id":1,"type":"message","text":"Pedido do usuário."}),
            json!({"v":1,"id":2,"type":"criterion","code":"MSTD-CRIT-0001","when":"Ao pedir","then":"Arquivo entregue","proof":"echo ok"}),
            json!({"v":1,"id":3,"type":"wave","n":1,"text":"Texto da onda.","criteria":[2],"done_when":"Conferido.","origin":1}),
            json!({"v":1,"id":4,"type":"task","wave":1,"title":"Gerar arquivo","code":"MSTD-TASK-0001","text":"Gerar cópia.","covers":[2]}),
            json!({"v":1,"id":5,"type":"send","wave":1,"text":input,"role":"wave"}),
            json!({"v":1,"id":6,"type":"delivered","wave":1,"text":"Volta anterior.","returned":true}),
            json!({"v":1,"id":7,"type":"send","wave":1,"text":input,"role":"wave","replaces":5,"tokens":180_000}),
            json!({"v":1,"id":8,"type":"step","wave":1,"item":4,"text":"Teste mínimo aprovado."}),
            json!({"v":1,"id":9,"type":"stage_run","phase":"delivery","stage":"criterion-proof","criterion":2,"waves":[1],"command":"pnpm test","result":"pass","exit":0,"ms":20}),
        ];
        let text=rows.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")+"\n";
        std::fs::write(folder.join("spec.ndjson"),&text).unwrap();
        let detail=wave_detail(root,"demo",1);
        assert_eq!(detail["request"],input);assert_eq!(detail["plan"]["text"],"Texto da onda.");
        assert_eq!(detail["user_input"],"Pedido do usuário.");assert_eq!(detail["tasks"][0]["code"],"MSTD-TASK-0001");
        assert_eq!(detail["tests"][0]["criterion"],"MSTD-CRIT-0001");
        assert_eq!(detail["delivery"]["text"],"Volta anterior.","a usage update is the same attempt");
        assert_eq!(detail["delivery_integrated"],false);
        std::fs::write(folder.join("spec.ndjson"),text+&json!({"v":1,"id":10,"type":"send","wave":1,"role":"wave","text":"Novo pedido.","resends":7}).to_string()+"\n").unwrap();
        assert!(wave_detail(root,"demo",1)["delivery"].is_null(),"an earlier return does not belong to a new send");
        let summary=snapshot(root,Some("demo"));
        assert_eq!(summary["specs"][0]["waves"][0]["status"],"running");
        assert!(summary["specs"][0]["waves"][0]["summary"].is_null());
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(),1,"read-only content loading");
    }
}
