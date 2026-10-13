//! Read-only impact previews. User command strings are parsed, never executed.
//! Unknown shell expansion is disclosed rather than guessed or evaluated.
use serde_json::{Value,json};
use std::path::Path;
pub(super) fn preview(start:&Path,raw:&str)->Value {
    if raw.len()>16384 {return json!({"ok":false,"reason":"impact-invalid-request"});}
    let Ok(request)=serde_json::from_str::<Value>(raw) else {return json!({"ok":false,"reason":"impact-invalid-request"});};
    let root=mustard_core::io::spec_events::spec_root(start);
    let action=request["action"].as_str().unwrap_or_default();
    let mut effects=Vec::<Value>::new();let mut unknown=Vec::<&str>::new();let mut relevant=action!="shell";
    let status=mustard_core::platform::git::run(start,&["status","--porcelain=v1","-z"]);
    let changes:Vec<_>=status.stdout.split('\0').filter(|row|!row.is_empty()).take(256).map(crate::shared::secret::without_secrets).collect();
    match action {
        "publish"=>{
            let scope=request["scope"].as_str().unwrap_or("project");
            if !["project","spec","report"].contains(&scope) {return json!({"ok":false,"reason":"impact-invalid-scope"});}
            effects.push(json!({"kind":"publication","scope":scope,"spec":request["spec"],
                "document":request["document"],
                "content":if scope=="report"{"selected-document"}else{"sanitized-project-or-spec-status"},
                "consumption_included":request["include_consumption"]==true,"credentials_included":false,
                "destination":mustard_core::ProjectConfig::load(&root).publication.map(|config|format!("{} / {}",config.provider,config.project_name)),"access":"depends-on-provider-configuration"}));
            unknown.push("provider-access-and-upload-not-verified");
        },
        "discard-wave"=>{
            let (Some(spec),Some(wave))=(request["spec"].as_str(),request["wave"].as_u64())else{return json!({"ok":false,"reason":"impact-needs-spec-and-wave"});};
            let detail=super::wave_detail(&root,spec,wave);
            effects.push(json!({"kind":"wave-attempt","spec":spec,"wave":wave,"request_recorded":detail["request_recorded"],
                "return_received":!detail["delivery"].is_null(),"integrated":detail["delivery_integrated"]}));
            unknown.push("discard-policy-and-copy-state-must-be-checked-by-native-wave-command");
        },
        "branches"=>{
            let branches=mustard_core::platform::git::run(start,&["branch","--merged","--format=%(refname:short)"]);
            if branches.ok {effects.push(json!({"kind":"merged-branches","names":branches.stdout.lines().take(128).collect::<Vec<_>>(),"deleted":false}));}
            unknown.push("merged-does-not-authorize-deletion");
        },
        "shell"=>{
            let Some(command)=request["command"].as_str() else {return json!({"ok":false,"reason":"impact-needs-command"});};
            let mut changed_directory=false;
            for segment in crate::hooks::bash::lex::segments(command) {
                let args:Vec<_>=segment.args.iter().map(|arg|arg.text.as_str()).collect();
                match segment.name() {
                    "rm"|"rmdir"|"del"|"Remove-Item"=>{
                        relevant=true;
                        if changed_directory {unknown.push("chained-directory-changes-not-resolved");continue;}
                        for arg in args.iter().filter(|arg|!arg.starts_with('-')) {
                            if arg.contains(['$','*','?','`','{','}']) {unknown.push("shell-expansion-not-evaluated");continue;}
                            let path=start.join(arg);let canonical=path.canonicalize().ok();
                            if canonical.as_ref().zip(root.canonicalize().ok()).is_some_and(|(path,root)|path.starts_with(root)) {
                                effects.push(json!({"kind":"remove-path","path":crate::shared::secret::without_secrets(arg),"exists":path.exists(),"directory":path.is_dir(),"recovery":"untracked-content-may-be-unrecoverable"}));
                            }else{unknown.push("path-outside-project-missing-or-unresolved");}
                        }
                    },
                    "git" if args.iter().any(|arg|["clean","reset","restore","checkout","branch","push"].contains(arg))=>{
                        relevant=true;
                        if changed_directory {unknown.push("chained-directory-changes-not-resolved");continue;}
                        effects.push(json!({"kind":"git-operation","operation":args.iter().find(|arg|["clean","reset","restore","checkout","branch","push"].contains(arg)),
                            "working_changes":changes,"recovery":"depends-on-index-commits-and-operation"}));
                        unknown.push("target-revisions-pathspecs-and-remote-state-not-fully-simulated");
                    },
                    "cd"=>{changed_directory=true;unknown.push("chained-directory-changes-not-resolved");},
                    _=>unknown.push("command-effects-not-statically-known"),
                }
            }
        },
        _=>return json!({"ok":false,"reason":"impact-unsupported-action"}),
    }
    unknown.sort_unstable();unknown.dedup();
    let active:Vec<_>=mustard_core::io::spec_index::read_specs(&root).iter().flat_map(|(name,log)|{
        log.planned_waves().into_iter().filter(|wave|log.last_by_wave("send").contains_key(wave) && !log.delivered_waves().contains(wave))
            .map(|wave|json!({"spec":name,"wave":wave})).collect::<Vec<_>>()
    }).collect();
    super::safe_value(json!({"ok":true,"schema_version":1,"action":action,"at":chrono::Utc::now().to_rfc3339(),"effects":effects,
        "working_changes":changes,"working_changes_complete":status.ok && status.stdout.split('\0').filter(|row|!row.is_empty()).count()<=256,
        "active_waves":active,"unknown":unknown,"complete":unknown.is_empty(),"relevant":relevant,"executed":false,"model_calls":0,
        "token":blake3::hash(format!("{}:{}:{}",status.stdout,raw,super::dashboard::revision(start,&root,"project",request["spec"].as_str(),None,request["wave"].as_u64())).as_bytes()).to_hex().to_string(),
        "permissions":"existing-host-and-Mustard-protections-unchanged"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_never_runs_user_shell_and_discloses_unknown_expansions() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("keep"),"data").unwrap();
        let result=preview(root,&json!({"action":"shell","command":"rm keep $(touch MUST-NOT-EXIST)"}).to_string());
        assert_eq!(result["executed"],false);assert_eq!(std::fs::read_to_string(root.join("keep")).unwrap(),"data");
        assert!(!root.join("MUST-NOT-EXIST").exists());assert_eq!(result["complete"],false);
    }
}
