use serde_json::json;
use std::path::Path;

mod evaluation;

pub struct Modes<'a> { pub coverage: bool, pub topics: Option<&'a Path>, pub evaluate: Option<&'a Path>, pub responsibility:bool,
    pub task: mustard_core::domain::knowledge::investigation::Task<'a> }

fn read_manifest<T:serde::de::DeserializeOwned>(path:&Path)->Result<T,String> {
    if std::fs::metadata(path).map_err(|e|e.to_string())?.len()>1_000_000 {return Err("knowledge-manifest-too-large".into());}
    serde_json::from_str(&std::fs::read_to_string(path).map_err(|e|e.to_string())?).map_err(|e|e.to_string())
}

pub fn run(root: &Path, query: &mustard_core::io::knowledge::Query<'_>, markdown: bool, out: Option<&Path>, record: Option<&Path>, modes:Modes<'_>) {
    let start = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    let anchor = mustard_core::io::spec_events::spec_root(&start);
    let git = mustard_core::platform::git::run(&start, &["rev-parse", "--show-toplevel"]);
    let tree = if git.ok { std::path::PathBuf::from(git.stdout.trim()) } else { anchor.clone() };
    let root = if tree != anchor && mustard_core::io::project_map::model_path(&tree).is_file() {
        tree.as_path()
    } else {anchor.as_path()};
    let answer: Result<String, String> = (|| {
        if let Some(record) = record {
            let metadata = std::fs::metadata(record).map_err(|e| e.to_string())?;
            if metadata.len() > 1_000_000 {
                return Err("knowledge-receipt-too-large".into());
            }
            let text = std::fs::read_to_string(record).map_err(|e| e.to_string())?;
            let receipt = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            return mustard_core::io::knowledge::record_at(root, &tree, &receipt).map(|v| v.to_string()).map_err(|e| format!("{e:?}"));
        }
        // Validate the destination before retrieving or writing evidence.
        if let Some(out) = out {
            let absolute = std::path::absolute(out).map_err(|e| e.to_string())?;
            if absolute == mustard_core::io::project_map::model_path(root) || out.extension().is_none_or(|e| e != if markdown { "md" } else { "json" }) {
                return Err("knowledge-invalid-export-path".into());
            }
        }
        let selector=if modes.coverage || !modes.responsibility {None}else{crate::shared::knowledge_selection::KnowledgeSelector::configured(&anchor)};
        let selector=selector.as_ref().map(|s|s as &dyn mustard_core::domain::knowledge::selection::SymbolSelector);
        let (report,text)=if modes.coverage {
            let report=mustard_core::io::knowledge::coverage::report(root).map_err(|e|format!("{e:?}"))?;
            (report.clone(),report.to_string())
        } else if let Some(path)=modes.evaluate {
            let plan=read_manifest(path)?;
            let report=evaluation::evaluate(root,&tree,&plan,query,selector)?;
            (report.clone(),report.to_string())
        } else if let Some(path)=modes.topics {
            let plan=read_manifest(path)?;
            let (report,map)=mustard_core::io::knowledge::dossier::assemble(root,&tree,&plan,query,selector,modes.responsibility).map_err(|e|format!("{e:?}"))?;
            let text=if markdown {mustard_core::io::knowledge::dossier::markdown(&report,&map)}else{report.to_string()};
            (report,text)
        } else {
            let (report,map)=if modes.responsibility {mustard_core::io::knowledge::query_with_selector(root,&tree,query,selector)} else {mustard_core::io::knowledge::query_for(root,&tree,query,modes.task)}.map_err(|e|format!("{e:?}"))?;
            let text=if markdown {mustard_core::domain::knowledge::markdown(&report,&map)}else{report.to_string()};
            (report,text)
        };
        if let Some(out) = out {
            mustard_core::io::fs::write_atomic(out, text.as_bytes()).map_err(|e| e.to_string())?;
            return Ok(json!({"ok":true,"file":out,"local_model_calls":report.get("local_model_calls").cloned().unwrap_or_else(||json!(0)),
                "remote_model_calls":report.get("remote_model_calls").cloned().unwrap_or_else(||json!(0))}).to_string());
        }
        Ok(text)
    })();
    match answer {
        Ok(text) => println!("{text}"),
        Err(detail) => {
            println!(
                "{}",
                json!({"ok":false,"reason":"knowledge-unavailable","detail":detail,"hint": "Refresh scan or use an exact search; missing evidence does not prove missing functionality." })
            );
            std::process::exit(1);
        }
    }
}
