//! The panel screens and their conditional reads. No transcript recovery,
//! model request, publication or write occurs in an ordinary screen query.
use std::path::{Path,PathBuf};
use serde_json::{Value,json};
use super::sections::{self,Params};

pub(super) const SECTIONS:&[&str]=sections::SCREENS;

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

pub(super) fn revision(start:&Path,root:&Path,section:&str,spec:Option<&str>,session:Option<&str>,wave:Option<u64>,params:&Params)->String {
    let mut hash=blake3::Hasher::new();
    hash.update(format!("{}:{section}:{spec:?}:{session:?}:{wave:?}:{params:?}",mustard_core::harness_version()).as_bytes());
    for (path,depth) in [(root.join("mustard.json"),0),(root.join(".claude/spec"),1),
        (root.join(".claude/settings.local.json"),0)] {stamp(&path,&mut hash,depth);}
    if let Some(spec)=spec && let Some(path)=spec_path(root,spec) {stamp(&path,&mut hash,0);}
    if section=="overview" && let Some(session)=session {stamp(&super::live_usage::file(root,session),&mut hash,0);}
    let git=mustard_core::platform::git::run(start,&["rev-parse","--git-dir"]).out();
    let git=PathBuf::from(git.unwrap_or_default().trim());let git=if git.is_absolute(){git}else{start.join(git)};
    stamp(&git.join("HEAD"),&mut hash,0);
    if section=="usage" {
        // The live usage of the session changes at every step: the usage
        // screen reads it, but never re-reads the whole screen because of it.
        let usage=root.join(".claude/mustard/usage");
        if let Ok(entries)=std::fs::read_dir(&usage) {
            let mut paths:Vec<_>=entries.flatten().map(|entry|entry.path()).filter(|path|!path.ends_with("live")).collect();paths.sort();
            for path in paths {stamp(&path,&mut hash,2);}
        }
        for path in [root.join(".claude/judgements"),root.join(".claude/mustard/operations")] {stamp(&path,&mut hash,1);}
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

pub(super) fn query_with(start:&Path,selected:Option<&str>,session:Option<&str>,wave:Option<u64>,section:&str,previous:Option<&str>,params:&Params)->Value {
    if !SECTIONS.contains(&section) {return json!({"ok":false,"reason":"panel-invalid-section"});}
    let started=std::time::Instant::now();
    let root=mustard_core::io::spec_events::spec_root(start);
    let selected=selected.map(str::to_string).or_else(||crate::shared::context::checkout::current_spec(&start.to_string_lossy()));
    let token=revision(start,&root,section,selected.as_deref(),session,wave,params);
    if previous==Some(token.as_str()) {return json!({"ok":true,"schema_version":1,"unchanged":true,"revision":token,"section":section});}
    screen(start,&root,selected,session,section,params,token,started)
}

/// One screen of the panel: the project line, the selected spec (the one
/// asked, the checkout's, or the first of the index) and the screen's body.
fn screen(start:&Path,root:&Path,selected:Option<String>,session:Option<&str>,section:&str,params:&Params,token:String,started:std::time::Instant)->Value {
    let selected=selected.or_else(||sections::index_rows(root).0.into_iter().next().map(|row|row.name));
    let log=selected.as_deref().and_then(|name|spec_log(root,name));
    let config=mustard_core::ProjectConfig::load(root);
    let mut data=json!({"ok":true,"schema_version":1,"section":section,"revision":token,"at":chrono::Utc::now().to_rfc3339(),
        "project":{"name":mustard_core::io::spend::project_name(root).or_else(||root.file_name().map(|name|name.to_string_lossy().into_owned())),
            "branch":mustard_core::platform::git::run(start,&["branch","--show-current"]).out(),
            "version":mustard_core::harness_version(),"language":config.language().text_or_default().to_string()},
        "selected_spec":selected});
    let body=match (section,selected.as_deref(),log.as_ref()) {
        ("overview",name,log)=>sections::overview(start,root,name,log,session,config.language().text_or_default()),
        ("spec",Some(name),Some(log))=>sections::spec(start,root,name,log),
        ("items",_,Some(log))=>sections::items(log,params,&mustard_core::domain::normalize::Languages::of(&config)),
        ("item",_,Some(log))=>params.item.as_deref().map_or_else(||json!({"item":null,"reason":"panel-item-missing"}),|code|sections::item(log,code)),
        ("usage",name,log)=>sections::usage(root,name,log,session),
        _=>json!({"reason":"panel-no-spec"}),
    };
    if let (Some(fields),Value::Object(body))=(data.as_object_mut(),body) {fields.extend(body);}
    data["performance"]=json!({"ms":started.elapsed().as_millis(),"spec_logs_loaded":usize::from(log.is_some()),"model_calls":0,"network_requests":0});
    super::safe_value(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn query(start:&Path,selected:Option<&str>,section:&str,previous:Option<&str>)->Value {
        query_with(start,selected,None,None,section,previous,&Params{kind:Some("rule".into()),..Params::default()})
    }
    #[test]
    fn unchanged_screens_skip_projection_and_other_spec_logs_stay_unloaded() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),"{}").unwrap();
        for name in ["current","other"] {std::fs::create_dir_all(root.join(".claude/spec").join(name)).unwrap();}
        std::fs::write(root.join(".claude/spec/current/spec.ndjson"),"{\"v\":1,\"id\":1,\"type\":\"rule\",\"title\":\"Exact rule\",\"text\":\"Exact definition\"}\n").unwrap();
        std::fs::write(root.join(".claude/spec/other/spec.ndjson"),"broken history that must not be parsed").unwrap();
        let view=query(root,Some("current"),"items",None);
        assert_eq!(view["ok"],true);assert_eq!(view["performance"]["spec_logs_loaded"],1);
        assert_eq!(view["rows"][0]["title"],"Exact rule","{view}");assert!(!root.join(".claude/mustard").exists());
        let unchanged=query(root,Some("current"),"items",view["revision"].as_str());
        assert_eq!(unchanged["unchanged"],true);assert!(unchanged.get("rows").is_none());
        std::fs::write(root.join("mustard.json"),"{\"rtk\":false}\n").unwrap();
        assert!(query(root,Some("current"),"items",view["revision"].as_str())["unchanged"].is_null());
    }
    #[test]
    fn a_section_the_panel_no_longer_has_is_refused() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),"{}").unwrap();
        for section in ["project","definitions","waves","execution","settings","ideas","diagnostics"] {
            assert_eq!(query(root,None,section,None)["reason"],"panel-invalid-section","{section}");
        }
    }
    #[test]
    fn selected_archived_spec_remains_readable_without_loading_other_histories() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),"{}").unwrap();
        let folder=root.join(".claude/spec/.descartadas/old");std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("spec.ndjson"),"{\"v\":1,\"id\":1,\"type\":\"rule\",\"text\":\"Historical requirement\"}\n").unwrap();
        let view=query(root,Some("old"),"items",None);
        assert!(view.to_string().contains("Historical requirement"),"{view}");assert_eq!(view["performance"]["spec_logs_loaded"],1);
    }
}
