use serde_json::json;
use std::path::Path;

pub fn run(root: &Path, query: &mustard_core::io::knowledge::Query<'_>, markdown: bool, out: Option<&Path>, record: Option<&Path>) {
    let start = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    let anchor = mustard_core::io::spec_events::spec_root(&start);
    let git = mustard_core::platform::git::run(&start, &["rev-parse", "--show-toplevel"]);
    let tree = if git.ok { std::path::PathBuf::from(git.stdout.trim()) } else { anchor.clone() };
    let root = anchor.as_path();
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
        let (report, map) = mustard_core::io::knowledge::query_with(root, &tree, query).map_err(|e| format!("{e:?}"))?;
        let text = if markdown { mustard_core::domain::knowledge::markdown(&report, &map) } else { report.to_string() };
        if let Some(out) = out {
            mustard_core::io::fs::write_atomic(out, text.as_bytes()).map_err(|e| e.to_string())?;
            return Ok(json!({"ok":true,"file":out,"local_model_calls":0,"remote_model_calls":0}).to_string());
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
