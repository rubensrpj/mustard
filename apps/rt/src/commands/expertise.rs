use std::path::Path;
use mustard_core::io::expertise;

#[allow(clippy::too_many_arguments)]
pub fn run(root: &Path, sync: bool, record: Option<&Path>, review: Option<&str>, export: Option<&str>, prepare: bool,native:bool,
    prepare_session:Option<&str>,complete_session:Option<&Path>,query:Option<&str>,host: &str, files: &[String]) {
    let started=std::time::Instant::now();
    let result = (|| {
        if native {return expertise::workflow::acknowledge_native(root,files,query.ok_or("expertise-short-task-required")?);}
        if prepare_session.is_some() || complete_session.is_some() {
            if files.len()>1 {return Err("expertise-session-accepts-one-file-scope".into());}
            let query=query.ok_or("expertise-short-task-required")?;
            let file=files.first().map(String::as_str);
            if let Some(host)=prepare_session {
                let config=mustard_core::ProjectConfig::load(root);
                return expertise::authoring::prepare(root,query,file,host,config.ai.session_models.get(host).map(String::as_str));
            }
            if let Some(path)=complete_session {
                if std::fs::metadata(path).map_err(|e|e.to_string())?.len()>100_000 {return Err("expertise-receipt-too-large".into());}
                let receipt=serde_json::from_slice(&std::fs::read(path).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
                return expertise::authoring::complete(root,query,file,&receipt);
            }
        }
        if sync { return expertise::sync(root); }
        if let Some(file) = record {
            let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
            if bytes.len() > 100_000 { return Err("expertise-manifest-too-large".into()); }
            let profile = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            return expertise::record(root, &profile);
        }
        if let Some(value) = review {
            let (name, fingerprint) = value.split_once(':').ok_or("expertise-review-needs-name-and-fingerprint")?;
            return expertise::review(root, name, fingerprint);
        }
        if let Some(name) = export { return expertise::export(root, name, host); }
        if prepare {
            let mut prepared=if let Some(query)=query {expertise::workflow::prepare_for_task(root,files,host,query)?}
                else {expertise::workflow::prepare(root,files,host,None,"native")?};
            if let Some(query)=query {
                let current=expertise::workflow::candidates(root,files);
                if files.is_empty() || files.iter().any(|file| !current.iter().any(|profile|
                    profile.origin=="authored" && expertise::workflow::task_fit(profile,query,files)>0 && expertise::matches(profile, std::slice::from_ref(file)))) {
                    let config=mustard_core::ProjectConfig::load(root);
                    let file = if files.len()==1 {files.first().map(String::as_str)}else{None};
                    prepared["procedure_preparation"]=expertise::authoring::prepare(root,query,file,host,
                        config.ai.session_models.get(host).map(String::as_str))?;
                }
            }
            return Ok(prepared);
        }
        expertise::report(root, files)
    })();
    match result {
        Ok(value) => {
            super::spec_events::conversation::record_output_call(root,"expertise",None,started,&value,&value.to_string());
            println!("{value}");
        }
        Err(reason) => {
            let value=serde_json::json!({"ok":false,"reason":reason});
            super::spec_events::conversation::record_output_call(root,"expertise",None,started,&value,&value.to_string());
            println!("{value}"); std::process::exit(1);
        }
    }
}
