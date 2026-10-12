//! Receipt metadata only. Session interpretation tokens are never added to
//! Jev charges or to the host's transcript totals a second time.
use std::path::Path;
use serde_json::{Value,json};
use mustard_core::domain::spec_events::SpecLog;

pub(super) fn snapshot(root:&Path)->Value {
    let mut logs=vec![super::super::spec_events::conversation::project_operations(root)];
    for (name,log) in mustard_core::io::spec_index::read_specs(root) {
        logs.push(log);
        logs.push(super::super::spec_events::conversation::recorded_operations(root,&name));
    }
    let catalog=match mustard_core::io::expertise::list(root) {
        Ok(profiles)=> {
            let active=profiles.iter().filter(|p|mustard_core::io::expertise::active(root,p)).count();
            json!({"available":true,"profiles":profiles.len(),"active":active,"inactive":profiles.len()-active,
                "native_navigation":profiles.iter().filter(|p|p.origin=="native-navigation").count()})
        }
        Err(_)=>json!({"available":false}),
    };
    json!({"analysis":aggregate(&logs),"catalog":catalog,"full_input_target":100_000,
        "usage_basis":"caller-reported; host totals include these tokens already; not net savings"})
}

fn aggregate(logs:&[SpecLog])->Value {
    let mut calls=0u64; let mut failures=0u64; let mut reused=0u64; let mut analyses=0u64;
    let mut input=0u64; let mut output=0u64; let mut unknown=0u64; let mut over=0u64;
    for call in logs.iter().flat_map(SpecLog::visible).filter(|e|e.event_type=="call" && e.str_field("command")==Some("knowledge")) {
        calls+=1;
        if call.str_field("result")==Some("refused") {failures+=1;}
        if call.fields.get("knowledge_cached")==Some(&Value::Bool(true)) {reused+=1;}
        let completed=call.int("knowledge_session_model_calls").unwrap_or(0);
        analyses=analyses.saturating_add(completed);
        if completed>0 {
            if let Some(tokens)=call.int("knowledge_input_tokens") {input=input.saturating_add(tokens);} else {unknown+=1;}
            output=output.saturating_add(call.int("knowledge_output_tokens").unwrap_or(0));
            if call.int("knowledge_input_tokens").is_some_and(|tokens|tokens>100_000) {over+=1;}
        }
    }
    json!({"operations":calls,"failures":failures,"reused_operations":reused,"completed_analyses":analyses,
        "reported_input_tokens":input,"reported_output_tokens":output,"analyses_with_unknown_input":unknown,"over_target":over})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_analysis_records_failures_and_reuse_without_a_spec_or_secret_payload() {
        let dir=tempfile::tempdir().unwrap(); let root=dir.path();
        for report in [json!({"ok":true,"cached":false,"session_model_calls":1,"input_tokens":100_001,"output_tokens":20,"interpretation":{"text":"private code"}}),
            json!({"ok":true,"cached":true,"session_model_calls":0}),json!({"ok":false,"reason":"knowledge-unavailable","detail":"private prompt"})] {
            assert!(super::super::super::spec_events::conversation::record_output_call(root,"knowledge",None,std::time::Instant::now(),&report,&report.to_string()).is_some());
        }
        let observed=snapshot(root);
        assert_eq!(observed["analysis"]["operations"],3);
        assert_eq!(observed["analysis"]["completed_analyses"],1);
        assert_eq!(observed["analysis"]["reused_operations"],1);
        assert_eq!(observed["analysis"]["failures"],1);
        assert_eq!(observed["analysis"]["over_target"],1);
        assert!(!observed.to_string().contains("private"));
        assert!(mustard_core::io::spec_index::read_specs(root).is_empty());
    }
}
