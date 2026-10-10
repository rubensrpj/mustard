//! Source versions describe freshness, never correctness or test coverage.
use mustard_core::domain::citation;
use mustard_core::io::sha256::Sha256;
use serde_json::{Value, json};
use std::path::Path;

pub(crate) fn source_version(root: &Path, source: &str) -> Option<String> {
    let (file, _) = citation::file_citation(source)?;
    let project = root.canonicalize().ok()?;
    let path = root.join(file).canonicalize().ok()?;
    if !path.starts_with(project) || !path.is_file() {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let mut digest = Sha256::new();
    digest.update(&bytes);
    Some(digest.hex_digest())
}

pub(crate) fn revalidate(root: &Path, value: &mut Value) -> bool {
    let mut changed = false;
    match value {
        Value::Array(items) => {
            for item in items {
                changed |= revalidate(root, item);
            }
        }
        Value::Object(fields) => {
            if fields.contains_key("source_version") && fields.contains_key("source") {
                let current = fields["source"].as_str().and_then(|source| source_version(root, source));
                let status = match (fields["source_version"].as_str(), current.as_deref()) {
                    (Some(old), Some(now)) if old == now => "source-unchanged",
                    (Some(_), Some(_)) => "source-changed",
                    _ => "source-unavailable",
                };
                fields.insert("current_source_version".into(), json!(current));
                fields.insert("source_status".into(), json!(status));
                fields.insert("verification".into(), json!("requires-source-confirmation"));
                changed = true;
            }
            for child in fields.values_mut() {
                changed |= revalidate(root, child);
            }
        }
        _ => {}
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_edited_source_invalidates_freshness_without_approving_or_erasing_a_fact() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("billing.rs"), "fn invoice() {}\n").unwrap();
        let source = "billing.rs:1";
        let mut fact =
            json!({"text":"Invoices are sent once.","source":source,"source_version":source_version(root,source),"classification":"syntax-candidate"});
        assert!(revalidate(root, &mut fact));
        assert_eq!(fact["source_status"], "source-unchanged");
        assert_eq!(fact["verification"], "requires-source-confirmation");
        std::fs::write(root.join("billing.rs"), "fn invoice() { retry(); }\n").unwrap();
        revalidate(root, &mut fact);
        assert_eq!(fact["source_status"], "source-changed");
        assert_eq!(fact["text"], "Invoices are sent once.");
        std::fs::remove_file(root.join("billing.rs")).unwrap();
        revalidate(root, &mut fact);
        assert_eq!(fact["source_status"], "source-unavailable");
    }
}
