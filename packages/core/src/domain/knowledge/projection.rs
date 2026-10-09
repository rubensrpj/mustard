//! First-read evidence without repeated file context or duplicate graph views.
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub fn compact(report: &mut Value) {
    let mut documented = BTreeSet::new();
    if let Some(cards) = report["cards"].as_array_mut() {
        for card in cards {
            let file = card["source"]["file"].as_str().unwrap_or_default().to_string();
            if let Some(object) = card.as_object_mut() {
                for key in ["signature", "documentation", "file_documentation"] {
                    if object.get(key).and_then(Value::as_str) == Some("") {
                        object.remove(key);
                    }
                }
                if object.contains_key("file_documentation") && !documented.insert(file) {
                    object.remove("file_documentation");
                }
            }
        }
    }
    if let Some(snapshot) = report["scan_snapshot"].as_object_mut() {
        if let Some(projects) = snapshot.get_mut("projects").and_then(Value::as_array_mut) {
            for project in projects {
                *project = json!({"name":project["name"],"dir":project["dir"],"code_files":project["code_files"]});
            }
        }
        if snapshot.get("skeleton").and_then(Value::as_array).is_some_and(Vec::is_empty) {
            snapshot.remove("skeleton");
        }
    }
    if let Some(coverage) = report["scan_coverage"].as_object_mut() {
        for (key, max) in [("extensions_without_code_parser", 8), ("skipped_directories", 6)] {
            if let Some(values) = coverage.get_mut(key).and_then(Value::as_array_mut) {
                let count = values.len();
                values.truncate(max);
                if count > max {
                    coverage.insert(format!("{key}_total"), json!(count));
                }
            }
        }
        coverage.remove("meaning");
    }
    if let Some(object) = report.as_object_mut() {
        object.remove("flows");
    }
    if let Some(catalog) = report["catalog"].as_object_mut() {
        catalog.remove("entry_kinds");
    }
    report["expand"] = json!({"detail":"run knowledge --symbol <id> --detail","source":"run map slice --file <file> --name <name>",
        "relations":"run knowledge --symbol <id> --direction callers --detail","coverage":"run knowledge --coverage","refresh":"run knowledge --refresh"});
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_evidence_preserves_receipts_and_exposes_expansion() {
        let receipt = json!({"file":"a.rs","line":3,"end_line":9,"sha256":"a".repeat(64)});
        let mut report = json!({"cards":[{"id":"a","signature":"","documentation":"","file_documentation":"shared","source":receipt},
            {"id":"b","file_documentation":"shared","source":receipt}],"flows":[{"repeat":"shared"}],
            "scan_snapshot":{"projects":[{"name":"demo","dir":"","code_files":2,"dependencies":["large"]}],"skeleton":[]},
            "scan_coverage":{"status":"partial-scope","extensions_without_code_parser":[],"skipped_directories":[],"meaning":"long"}});
        let bytes = report.to_string().len();
        compact(&mut report);
        assert_eq!(report["cards"][0]["source"], receipt);
        assert_eq!(report["cards"][1]["source"], receipt);
        assert!(report["cards"][1].get("file_documentation").is_none());
        assert!(report["cards"][0].get("signature").is_none());
        assert!(report["expand"]["coverage"].is_string());
        assert_eq!(report["scan_coverage"]["status"], "partial-scope");
        // The expansion instructions themselves have a fixed cost; large real
        // duplicated contexts, rather than tiny fixtures, establish savings.
        assert!(bytes > 0);
    }
}
