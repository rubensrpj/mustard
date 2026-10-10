//! Explain indexed scope separately from SQLite integrity and search success.
use crate::domain::project_map::MapRefusal;
use crate::io::project_map::{self as store, unreadable};
use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use std::path::Path;

pub fn report(root: &Path) -> Result<Value, MapRefusal> {
    let db = store::open_existing(&store::model_path(root))?;
    let columns: Vec<String> = db
        .conn()
        .prepare("PRAGMA table_info(census)")
        .map_err(|e| unreadable(e.into()))?
        .query_map([], |r| r.get(1))
        .map_err(|e| unreadable(e.into()))?
        .collect::<Result<_, _>>()
        .map_err(|e| unreadable(e.into()))?;
    if !columns.iter().any(|c| c == "coverage_report") {
        return Ok(json!({"status":"unknown","reason":"rescan-required-for-coverage"}));
    }
    let raw: Option<String> =
        db.conn().query_row("SELECT coverage_report FROM census LIMIT 1", [], |r| r.get(0)).optional().map_err(|e| unreadable(e.into()))?.flatten();
    let Some(census) = raw.and_then(|raw| serde_json::from_str::<Value>(&raw).ok()).filter(Value::is_object) else {
        return Ok(json!({"status":"unknown","reason":"rescan-required-for-coverage"}));
    };
    let code = census["code_files_read"].as_u64().unwrap_or(0);
    let other: u64 = census["unsupported_exts"].as_array().into_iter().flatten().filter_map(|r| r["count"].as_u64()).sum();
    // Coverage is a scan-time aggregate. Decoding every source pack here
    // would defeat point retrieval and make unrelated malformed JSON block
    // an otherwise valid exact-symbol query. Audit checks current integrity.
    let parse =
        census.get("parse").filter(|v| v.is_object()).cloned().unwrap_or_else(|| json!({"status":"unknown","reason":"rescan-required-for-parser-counts"}));
    Ok(json!({"status":if code==0 && other>0 {"no-code-grammar-matched"}else{"partial-scope"},
        "code_files":code,"extensions_without_code_parser":census["unsupported_exts"],"unreadable_or_non_utf8_files":census["non_utf8_skipped"],
        "skipped_directories":census["skipped_build_dirs"],"parse":parse,
        "test_symbols":"indexed with test_only=true; static test evidence does not prove production behavior or test execution",
        "meaning":"visited files at last scan; non-code extensions can include indexed text resources; ignored directories are not enumerated; no completeness guarantee"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parser_gaps_survive_storage_and_old_census_requires_rescan() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        store::write_text(
            root,
            &json!({"modules":[],"coverage":{"code_files_read":0,"non_utf8_skipped":1,
            "unsupported_exts":[{"ext":"cpp","count":3}],"skipped_build_dirs":["vendor"]}})
            .to_string(),
        )
        .unwrap();
        let report = report(root).unwrap();
        assert_eq!(report["status"], "no-code-grammar-matched");
        assert_eq!(report["extensions_without_code_parser"][0]["count"], 3);
        let db = store::open_existing(&store::model_path(root)).unwrap();
        db.conn().execute("UPDATE census SET coverage_report=NULL", []).unwrap();
        assert_eq!(super::report(root).unwrap()["status"], "unknown");
    }
}
