//! Project-scoped RTK compression, refreshed only on the usage screen.
//! Global lifetime totals and output compression are never model savings.

use serde_json::Value;
use mustard_core::platform::code_tools::{MachineRunner,RunOutcome,ToolRunner};

/// Cached project-scoped output compression. Ordinary panel/statusline polls
/// spawn no RTK process. A failed refresh retains the last known measurement.
pub fn cached(root: &std::path::Path) -> Value {
    let root=mustard_core::io::spec_events::spec_root(root);
    let switches=mustard_core::platform::project_seed::settings::Switches::read(&root);
    let mut view=std::fs::read_to_string(cache_path(&root)).ok()
        .and_then(|text|serde_json::from_str::<Value>(&text).ok()).filter(Value::is_object)
        .unwrap_or_else(||serde_json::json!({"status":"not-measured","scope":"project-directory"}));
    view["configured"]=serde_json::json!(switches.rtk);
    view["hook_configured"]=serde_json::json!(switches.rtk_hook);
    view
}

fn cache_path(root:&std::path::Path)->std::path::PathBuf {root.join(".claude/mustard/usage/rtk.json")}

pub fn refresh(root:&std::path::Path) {
    let root=mustard_core::io::spec_events::spec_root(root);
    let runner=MachineRunner::new(&std::env::var("PATH").unwrap_or_default()).at(&root);
    let measured=query(&runner);
    let mut view=retain_measurement(cached(&root),measured);
    view["refresh_at"]=serde_json::json!(chrono::Utc::now().to_rfc3339());
    if let Ok(bytes)=serde_json::to_vec(&view) {let _=mustard_core::io::fs::write_atomic(cache_path(&root),&bytes);}
}

fn retain_measurement(mut previous:Value, measured:Value)->Value {
    if measured["status"]=="measured" || measured["status"]=="no-recorded-commands" {return measured;}
    previous["refresh_status"]=measured["status"].clone();
    previous["stale"]=serde_json::json!(true);
    previous
}

fn query(runner:&impl ToolRunner)->Value {
    let (outcome,raw)=runner.output("rtk",&["gain","--project","--daily","--format","json"],std::time::Duration::from_secs(3));
    let failure=|reason|serde_json::json!({"status":reason,"scope":"project-directory"});
    if outcome!=RunOutcome::Succeeded {return failure(match outcome {RunOutcome::TimedOut {..}=>"timed-out",_=>"unavailable"});}
    let Ok(data)=serde_json::from_str::<Value>(&raw) else {return failure("invalid-response")};
    let summary=data.get("summary").unwrap_or(&data);
    // Unknown/malformed data must never look like a measured zero.
    if !["total_commands","total_input","total_output"].iter()
        .all(|key|summary[*key].as_f64().is_some_and(|v|v.is_finite() && v>=0.0))
        || !["total_saved","avg_savings_pct"].iter()
        .all(|key|summary[*key].as_f64().is_some_and(f64::is_finite)) {return failure("invalid-response");}
    if summary["total_commands"].as_f64()==Some(0.0) {return failure("no-recorded-commands");}
    let dates:Vec<_>=data["daily"].as_array().into_iter().flatten().filter_map(|day|day["date"].as_str())
        .filter(|date|chrono::NaiveDate::parse_from_str(date,"%Y-%m-%d").is_ok()).collect();
    serde_json::json!({"status":"measured","scope":"project-directory","basis":"rtk-output-bytes-divided-by-four",
        "measured_at":chrono::Utc::now().to_rfc3339(),"commands":summary["total_commands"],
        "input_tokens":summary["total_input"],"output_tokens":summary["total_output"],
        "saved_tokens":summary["total_saved"],"percent":summary["avg_savings_pct"],
        "period_start":dates.iter().min(),"period_end":dates.iter().max(),"net_model_savings_percent":null,"stale":false})
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    struct Runner { outcome:RunOutcome, raw:String }
    impl ToolRunner for Runner {
        fn on_path(&self,_:&str)->bool {panic!("no separate installation probe")}
        fn run(&self,_:&str,_:&[&str])->bool {panic!("no secondary commands")}
        fn found_off_path(&self,_:&str)->Option<std::path::PathBuf> {None}
        fn output(&self,program:&str,args:&[&str],limit:Duration)->(RunOutcome,String) {
            assert_eq!(program,"rtk");
            assert_eq!(args,["gain","--project","--daily","--format","json"]);
            assert_eq!(limit,Duration::from_secs(3));
            (self.outcome,self.raw.clone())
        }
    }
    fn response()->Value {json!({"summary":{"total_commands":10,"total_input":1000,
        "total_output":600,"total_saved":400,"avg_savings_pct":40.0},"daily":[]})}
    fn measured(data:Value)->Value {query(&Runner{outcome:RunOutcome::Succeeded,raw:data.to_string()})}

    #[test]
    fn measures_project_output_without_claiming_model_savings() {
        let mut history=response();
        history["daily"]=json!([{"date":"2026-10-11"},{"date":"invalid"},{"date":"2026-10-01"}]);
        let view=measured(history);
        assert_eq!(view["status"],"measured");
        assert_eq!(view["scope"],"project-directory");
        assert_eq!(view["commands"],10);
        assert_eq!(view["saved_tokens"],400);
        assert_eq!(view["percent"],40.0);
        assert_eq!(view["period_start"],"2026-10-01");
        assert_eq!(view["period_end"],"2026-10-11");
        assert!(view["net_model_savings_percent"].is_null());
        let mut expanded=response();
        expanded["summary"]["total_output"]=json!(1100);
        expanded["summary"]["total_saved"]=json!(-100);
        expanded["summary"]["avg_savings_pct"]=json!(-10.0);
        assert_eq!(measured(expanded)["percent"],-10.0,"growth must not become fictitious savings");
    }
    #[test]
    fn failed_or_malformed_readings_never_become_measured_zero() {
        for (outcome,raw,status) in [(RunOutcome::Failed,"{}","unavailable"),
            (RunOutcome::TimedOut{after:Duration::from_secs(3)},"{}","timed-out"),
            (RunOutcome::Succeeded,"not json","invalid-response"),
            (RunOutcome::Succeeded,"{\"summary\":{\"total_commands\":0}}","invalid-response")] {
            let view=query(&Runner{outcome,raw:raw.into()});
            assert_eq!(view["status"],status);
            assert!(view["saved_tokens"].is_null());
        }
        let empty=measured(json!({"total_commands":0,"total_input":0,"total_output":0,"total_saved":0,"avg_savings_pct":0}));
        assert_eq!(empty["status"],"no-recorded-commands");
        assert!(empty["percent"].is_null(),"no sample is not measured zero savings");
    }
    #[test]
    fn cached_reads_keep_the_last_dated_measurement_after_a_timeout() {
        let root=tempfile::tempdir().unwrap();
        let missing=cached(root.path());
        assert_eq!(missing["status"],"not-measured");
        let first=measured(response());
        let retained=retain_measurement(first.clone(),json!({"status":"timed-out"}));
        mustard_core::io::fs::write_atomic(cache_path(root.path()),&serde_json::to_vec(&retained).unwrap()).unwrap();
        let view=cached(root.path());
        assert_eq!(view["percent"],40.0);
        assert_eq!(view["measured_at"],first["measured_at"]);
        assert_eq!(view["refresh_status"],"timed-out");
        assert_eq!(view["stale"],true);
        let replaced=retain_measurement(retained,first.clone());
        assert_eq!(replaced,first);
        assert!(replaced["refresh_status"].is_null());
    }
}
