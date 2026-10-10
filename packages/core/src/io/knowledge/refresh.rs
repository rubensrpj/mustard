//! An explicit review queue, not automatic generation or a fresh receipt.
use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};

use super::{Interpretation, current, note_card};
use crate::domain::{knowledge, normalize::Languages};

#[allow(clippy::too_many_arguments)]
pub(super) fn report(
    tree: &Path,
    notes: &[Interpretation],
    query: &str,
    file: Option<&str>,
    limit: usize,
    all: bool,
    detail: bool,
    languages: &Languages,
    hashes: &mut BTreeMap<String, Option<(String, u64)>>,
) -> Value {
    let documents: Vec<_> = notes.iter().map(note_card).collect();
    let mut candidates = Vec::new();
    let mut matching_stale = 0;
    for i in knowledge::ranked(&documents, query, languages) {
        let note = &notes[i];
        if file.is_some_and(|file| !note.sources.iter().any(|source| source.file == file)) {
            continue;
        }
        let mut changed = Vec::new();
        let mut unchanged = Vec::new();
        for source in &note.sources {
            if current(tree, source, hashes) {
                unchanged.push(source);
            } else {
                let now = hashes.get(&source.file).and_then(Option::as_ref);
                let reason = match now {
                    None => "missing-unreadable-or-outside-tree",
                    Some((hash, _)) if hash != &source.sha256 => "content-changed",
                    Some(_) => "invalid-source-range",
                };
                changed.push(json!({"previous_source":source,"reason":reason,
                    "current_file":now.map(|(hash,lines)|json!({"sha256":hash,"lines":lines})),
                    "current_symbol_range":"unknown; inspect refreshed scan, do not reuse old line numbers"}));
            }
        }
        if changed.is_empty() {
            continue;
        }
        matching_stale += 1;
        if !all && candidates.len() >= limit.clamp(1, 50) {
            continue;
        }
        let mut item = json!({"id":note.id,"title":note.title,"previous_status":note.status,
            "origin":note.origin,"changed_sources":changed,"unchanged_sources":unchanged,
            "semantic_state":"stale; review required before a new receipt"});
        if detail {
            item["previous_text"] = json!(note.text);
        }
        candidates.push(item);
    }
    json!({"ok":true,"schema_version":knowledge::VERSION,"query":query,
        "operation":"interpretation-refresh-review","refresh_candidates":candidates,
        "matching_stale_count":matching_stale,"omitted_candidates":matching_stale-candidates.len(),
        "cards":[],"interpretations":[],"projection":if detail {"detail"} else {"summary"},
        "gaps":["Previous interpretations are not current evidence. Current hashes do not prove their meaning or locate moved declarations."],
        "next":"Refresh scan, inspect only affected files/symbols and necessary unchanged sources, then explicitly record a reviewed or hypothesis receipt. No receipt is produced automatically.",
        "remote_model_calls":0})
}
