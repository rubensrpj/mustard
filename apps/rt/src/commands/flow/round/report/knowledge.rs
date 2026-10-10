//! Promote only newly accepted deliveries to grounded, reusable hypotheses.
use mustard_core::io::spec_events as store;
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn capture(
    root: &Path,
    spec: &str,
    recorded: &mut [Value],
    warnings: &mut Vec<Value>,
) {
    // Reuse accepted wave conclusions in subsequent source investigations.
    // Formatting or concurrent edits invalidate receipts instead of re-stamping them.
    if let Ok(path) = store::spec_file(root, spec)
        && let Ok(Some(accepted)) = store::read(&path)
    {
        for event in accepted
            .visible()
            .into_iter()
            .filter(|event| event.event_type == "delivered" && !event.returned())
        {
            if !recorded
                .iter()
                .any(|item| item["type"] == "delivered" && item["id"] == event.id)
            {
                continue;
            }
            if let Some(notes) = event.fields.get("knowledge") {
                match mustard_core::io::knowledge::waves::capture(root,root,spec,event.id,true,notes) {
                    Ok(result)=>if let Some(item)=recorded.iter_mut().find(|item|item["type"]=="delivered" && item["id"]==event.id) {item["knowledge"]=result;},
                    Err(reason)=>warnings.push(json!({"reason":"wave-knowledge-not-captured","delivery":event.id,"detail":reason})),
                }
            }
        }
    }
}
