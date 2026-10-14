//! Section projections and conditional reads. No transcript recovery, model
//! request, publication or write occurs in an ordinary dashboard query.
use std::path::{Path,PathBuf};
use serde_json::{Value,json};

pub(super) const SECTIONS:&[&str]=&["project","definitions","waves","execution","usage","settings","ideas","diagnostics"];

fn stamp(path:&Path,hash:&mut blake3::Hasher,depth:u8) {
    let Ok(meta)=std::fs::symlink_metadata(path) else {hash.update(b"missing");return;};
    hash.update(path.to_string_lossy().as_bytes());
    hash.update(&meta.len().to_le_bytes());
    if let Ok(time)=meta.modified().and_then(|time|time.duration_since(std::time::UNIX_EPOCH).map_err(std::io::Error::other)) {
        hash.update(&time.as_nanos().to_le_bytes());
    }
    // Never follow links or recurse into code, transcripts or exported pages.
    if depth>0 && meta.is_dir() && let Ok(entries)=std::fs::read_dir(path) {
        let mut paths:Vec<_>=entries.flatten().map(|entry|entry.path()).collect();paths.sort();
        for path in paths {stamp(&path,hash,depth-1);}
    }
}

pub(super) fn revision(start:&Path,root:&Path,section:&str,spec:Option<&str>,session:Option<&str>,wave:Option<u64>)->String {
    let mut hash=blake3::Hasher::new();
    hash.update(format!("{}:{section}:{spec:?}:{session:?}:{wave:?}",mustard_core::harness_version()).as_bytes());
    for (path,depth) in [(root.join("mustard.json"),0),(root.join(".claude/spec"),1),
        (root.join(".claude/mustard/panel/preferences.json"),0),
        (root.join(".claude/settings.local.json"),0)] {stamp(&path,&mut hash,depth);}
    if let Some(spec)=spec && let Some(path)=spec_path(root,spec) {stamp(&path,&mut hash,0);}
    if section=="ideas" {stamp(&root.join(".claude/pending/ledger.json"),&mut hash,0);}
    if section=="project" {stamp(&mustard_core::io::project_map::model_path(root),&mut hash,0);stamp(&mustard_core::io::project_map::model_path(root).with_extension("db-wal"),&mut hash,0);}
    if matches!(section,"project"|"waves"|"execution") && let Some(session)=session {stamp(&super::live_usage::file(root,session),&mut hash,0);}
    let git=mustard_core::platform::git::run(start,&["rev-parse","--git-dir"]).out();
    let git=PathBuf::from(git.unwrap_or_default().trim());let git=if git.is_absolute(){git}else{start.join(git)};
    stamp(&git.join("HEAD"),&mut hash,0);
    if section=="usage" || section=="diagnostics" {
        for (path,depth) in [(root.join(".claude/judgements"),1),(root.join(".claude/mustard/observations.db"),0),
            (root.join(".claude/mustard/observations.db-wal"),0),
            (root.join(".claude/mustard/host-observations"),1),(root.join(".claude/mustard/installation.json"),0),
            (root.join(".claude/mustard/operations"),1),(root.join(".claude/mustard/usage"),3),
            (root.join(".claude/mustard/panel/diagnostics.json"),0),
            (mustard_core::io::project_map::model_path(root),0),
            (mustard_core::io::project_map::model_path(root).with_extension("db-wal"),0)] {stamp(&path,&mut hash,depth);}
    }
    hash.finalize().to_hex().to_string()
}

fn spec_path(root:&Path,name:&str)->Option<PathBuf> {
    let live=mustard_core::io::spec_events::spec_file(root,name).ok()?;
    if live.is_file(){Some(live)}else{
        let archived=root.join(".claude/spec").join(mustard_core::io::spec_index::DISCARDED_DIR).join(name).join("spec.ndjson");
        archived.is_file().then_some(archived)
    }
}
pub(super) fn spec_log(root:&Path,name:&str)->Option<mustard_core::domain::spec_events::SpecLog> {
    spec_path(root,name).and_then(|path|mustard_core::io::spec_events::read(&path).ok().flatten())
}

pub(super) fn query(start:&Path,selected:Option<&str>,session:Option<&str>,wave:Option<u64>,section:&str,previous:Option<&str>)->Value {
    if !SECTIONS.contains(&section) {return json!({"ok":false,"reason":"panel-invalid-section"});}
    let started=std::time::Instant::now();
    let root=mustard_core::io::spec_events::spec_root(start);
    let selected=selected.map(str::to_string).or_else(||crate::shared::context::checkout::current_spec(&start.to_string_lossy()));
    let token=revision(start,&root,section,selected.as_deref(),session,wave);
    if previous==Some(token.as_str()) {return json!({"ok":true,"schema_version":1,"unchanged":true,"revision":token,"section":section});}
    let index=std::fs::read_to_string(root.join(".claude/spec/index.ndjson")).unwrap_or_default();
    let mut specs:Vec<Value>=index.lines().filter_map(|line|serde_json::from_str::<Value>(line).ok())
        .filter(|row|row["type"]=="spec").map(|row|json!({"name":row["name"],"phase":row["phase"],"goal":row["goal"],
            "updated":row["updated"],"counted_waves":row["counted_waves"],"summary_only":true})).collect();
    // An absent/stale index must not hide a newly created spec. Only enumerate
    // names; other specs' histories and usage are never parsed for this view.
    if let Ok(entries)=std::fs::read_dir(root.join(".claude/spec")) {
        for entry in entries.flatten().filter(|entry|entry.file_type().is_ok_and(|kind|kind.is_dir())) {
            let name=entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with('.') && entry.path().join("spec.ndjson").is_file() && !specs.iter().any(|row|row["name"]==name) {
                specs.push(json!({"name":name,"summary_only":true}));
            }
        }
    }
    let selected=selected.or_else(||specs.first().and_then(|row|row["name"].as_str()).map(str::to_string));
    let config=mustard_core::ProjectConfig::load(&root);
    let mut data=json!({"ok":true,"schema_version":1,"section":section,"revision":token,"at":chrono::Utc::now().to_rfc3339(),
        "project":{"name":mustard_core::io::spend::project_name(&root).or_else(||root.file_name().map(|name|name.to_string_lossy().into_owned())),
            "branch":mustard_core::platform::git::run(start,&["branch","--show-current"]).out(),
            "version":mustard_core::harness_version(),"language":config.language().text_or_default().to_string(),
            "publication_url":mustard_core::io::spec_index::project_page_url(&root)},
        "selected_spec":selected,"commands":{"panel":"/mustard-panel","publish":"/mustard-pages"}});
    let jev=if section=="usage" {super::jev_usage::Ledger::read(&root)}else{super::jev_usage::Ledger::default()};
    if !matches!(section,"ideas"|"settings"|"diagnostics") && let Some(name)=selected.as_deref() && let Some(log)=spec_log(&root,name) {
        let mut detail=super::spec_view_mode(&root,name,&log,matches!(section,"execution"|"project"|"definitions"),&jev,section);
        if section=="definitions" {detail["definitions"]=super::definitions(&log);}
        else {detail.as_object_mut().map(|fields|fields.remove("definitions"));}
        if section!="usage" {for key in ["operations","usage","jev","stages"] {detail.as_object_mut().map(|fields|fields.remove(key));}}
        if let Some(row)=specs.iter_mut().find(|row|row["name"]==name) {*row=detail;}else{specs.push(detail);}
        if section=="waves" && let Some(wave)=wave {data["wave_detail"]=super::wave_detail(&root,name,wave);}
    }
    specs.sort_by(|a,b|a["name"].as_str().cmp(&b["name"].as_str()));data["specs"]=json!(specs);
    if section=="usage" {
        let costs=super::consumption::costs(&root);
        for spec in data["specs"].as_array_mut().into_iter().flatten().filter(|row|row["summary_only"]!=true) {
            let name=spec["name"].as_str().unwrap_or_default().to_string();
            spec["usage"]["claude_cost"]=json!({"known_micro_usd":costs["by_spec"][&name],"basis":costs["basis"]});
        }
        data["jev"]=jev.project();data["jev"]["configuration"]=super::jev_usage::configuration(&root,&config,std::env::var(mustard_core::io::jev_gate::KEY_ENV).ok());
        data["claude_cost"]=costs;data["rtk"]=crate::shared::rtk_gain::cached(&root);
        data["consumption"]=super::consumption::history(&root,mustard_core::io::spend::machine_dir().as_deref());
        data["knowledge"]=super::knowledge_usage::snapshot(&root);
        data["statusline"]=super::consumption::statusline(&root,session);
        data["live_usage"]=super::live_usage::snapshot(&root,session);
        data["integration_health"]=crate::shared::integration_health::snapshot(&root);
        data["retrieval"]=json!({"vectors_enabled":config.ai_vectors_enabled(),"index_present":mustard_core::io::project_map::model_path(&root).is_file()});
        data["installation"]=std::fs::read_to_string(root.join(".claude/mustard/installation.json")).ok().and_then(|text|serde_json::from_str::<Value>(&text).ok()).unwrap_or(Value::Null);
    }
    if section=="ideas" {data["ideas"]=crate::commands::event::pending::ideas::run(&root,None);}
    if matches!(section,"project"|"diagnostics") {data["project_profile"]=mustard_core::io::project_profile::summary(&root,&[]);}
    if matches!(section,"waves"|"execution"|"project") {data["live_usage"]=super::live_usage::snapshot(&root,session);}
    if section=="diagnostics" {data["diagnostics"]=super::diagnostics::snapshot(&root);data["integration_health"]=crate::shared::integration_health::snapshot(&root);}
    data["preferences"]=super::preferences::run(&root,None);
    data["performance"]=json!({"ms":started.elapsed().as_millis(),"spec_logs_loaded":usize::from(selected.is_some() && !matches!(section,"ideas"|"settings"|"diagnostics")),
        "model_calls":0,"network_requests":0});
    super::safe_value(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unchanged_sections_skip_projection_and_other_spec_logs_stay_unloaded() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),"{}").unwrap();
        for name in ["current","other"] {std::fs::create_dir_all(root.join(".claude/spec").join(name)).unwrap();}
        std::fs::write(root.join(".claude/spec/current/spec.ndjson"),"{\"v\":1,\"id\":1,\"type\":\"context\",\"title\":\"Goal\",\"text\":\"Exact definition\"}\n").unwrap();
        std::fs::write(root.join(".claude/spec/other/spec.ndjson"),"broken history that must not be parsed").unwrap();
        let view=query(root,Some("current"),None,None,"definitions",None);
        assert_eq!(view["ok"],true);assert_eq!(view["performance"]["spec_logs_loaded"],1);
        assert!(view.to_string().contains("Exact definition"));assert!(!root.join(".claude/mustard").exists());
        let unchanged=query(root,Some("current"),None,None,"definitions",view["revision"].as_str());
        assert_eq!(unchanged["unchanged"],true);assert!(unchanged.get("specs").is_none());
        std::fs::write(root.join("mustard.json"),"{\"rtk\":false}\n").unwrap();
        assert!(query(root,Some("current"),None,None,"definitions",view["revision"].as_str())["unchanged"].is_null());
    }
    #[test]
    fn a_receipt_still_in_the_write_ahead_log_changes_the_usage_section() {
        use mustard_core::io::observations::{self,Receipt};
        let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),"{}").unwrap();
        observations::record(root,&Receipt{version:1,spec:"demo".into(),dispatch:7,wave:2,host:"claude".into(),
            actor:"fixture".into(),call:"call".into(),tool:"Bash".into(),source:"git --version".into(),
            input_sha256:observations::digest(b"command"),output_sha256:observations::digest(b"output"),
            exit_code:Some(0),result_status:"returned".into(),observed_at:"2026-01-02T00:00:00Z".into()}).unwrap();
        let view=query(root,None,None,None,"usage",None);
        assert_eq!(view["ok"],true,"{view}");
        // Enquanto outra conexão segura o banco aberto, a gravação nova fica só
        // no log ao lado dele, e o arquivo principal não muda.
        std::fs::write(root.join(".claude/mustard/observations.db-wal"),b"frames of a new receipt").unwrap();
        assert!(query(root,None,None,None,"usage",view["revision"].as_str())["unchanged"].is_null());
    }
    #[test]
    fn selected_archived_spec_remains_readable_without_loading_other_histories() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),"{}").unwrap();
        let folder=root.join(".claude/spec/.descartadas/old");std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("spec.ndjson"),"{\"v\":1,\"id\":1,\"type\":\"context\",\"text\":\"Historical requirement\"}\n").unwrap();
        let view=query(root,Some("old"),None,None,"definitions",None);
        assert!(view.to_string().contains("Historical requirement"));assert_eq!(view["specs"].as_array().unwrap().len(),1);
    }
}
