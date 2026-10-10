//! Describe observed breadth without rewriting the caller's native query.
use super::{Answer, Request};
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn assess(root: &Path, answer: &Answer) -> Value {
    let Some(files) = answer.report["result"]["numFiles"].as_u64() else { return Value::Null };
    let indexed = crate::io::knowledge::indexed_file_count(root);
    let broad = indexed.is_some_and(|total| files.saturating_mul(2) > total && total > 1);
    json!({"returned_files":files,"indexed_project_files":indexed,
        "selectivity":if broad {"majority-of-indexed-project"}else{"not-established"},
        "next_step":if files==0 {"refine pattern or scope"}else if files>1 {"inspect file names; narrow path and request matching content"}else{"read matching source"},
        "native_query_rewritten":false,"index_inventory_may_be_partial":true})
}

pub(super) fn is_file_discovery(request: &Request) -> bool {
    request.tool == "Glob" || request.tool == "Grep" && request.input["output_mode"] == "files_with_matches"
}
