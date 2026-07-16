//! `scan-patterns-decline` — persist the enrich agent's justified refusal of a
//! mold candidate so `scan-patterns-list` stops re-proposing it.
//!
//! The patterns agent may DECLINE a worklist candidate (exemplars that are all
//! generated code behind a read-deny, a role already covered by another mold,
//! a cluster with no teachable shape). Before this command the refusal left no
//! trace on disk, so every scan re-proposed and re-judged the same candidate
//! forever. The orchestrator now relays each `=== DECLINE: <slug> ===` block
//! here; the store is `.claude/scan-declined.json` (slug → reason, a `BTreeMap`
//! so the bytes are stable). Re-auditing a refusal = deleting its entry (or the
//! whole file) and rescanning.
//!
//! Fail-open per the `mustard-rt run` contract: any IO/serde failure prints a
//! clear stderr line and exits 0. The only non-zero exit is an empty
//! `--slug`/`--reason` — a caller bug worth surfacing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mustard_core::io::fs as mfs;

/// The declined-candidates store, relative to the workspace root.
fn store_path(root: &Path) -> PathBuf {
    root.join(".claude").join("scan-declined.json")
}

/// Read the declined map (slug → reason). Missing or unparseable → empty.
pub(crate) fn declined(root: &Path) -> BTreeMap<String, String> {
    let Ok(text) = std::fs::read_to_string(store_path(root)) else {
        return BTreeMap::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Run `scan-patterns-decline`: record `slug` → `reason` in the store.
pub fn run(root: &Path, slug: &str, reason: &str) {
    let slug = slug.trim();
    let reason = reason.trim();
    if slug.is_empty() || reason.is_empty() {
        eprintln!("scan-patterns-decline: --slug and --reason must be non-empty");
        std::process::exit(1);
    }

    let mut map = declined(root);
    map.insert(slug.to_string(), reason.to_string());
    let json = match serde_json::to_string_pretty(&map) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("scan-patterns-decline: cannot serialise the store: {e}");
            return;
        }
    };
    let path = store_path(root);
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("scan-patterns-decline: cannot create {}: {e}", parent.display());
            return;
        }
    }
    if let Err(e) = mfs::write_atomic(&path, format!("{json}\n").as_bytes()) {
        eprintln!("scan-patterns-decline: cannot write {}: {e}", path.display());
        return;
    }
    println!("scan-patterns-decline: recorded {slug}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_store_reads_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(declined(dir.path()).is_empty());
    }

    #[test]
    fn run_records_and_accumulates() {
        let dir = tempfile::tempdir().unwrap();
        run(dir.path(), "api-type", "covered by api-status-pattern");
        run(dir.path(), "api-id", "exemplars are generated migrations");
        let map = declined(dir.path());
        assert_eq!(map.len(), 2);
        assert_eq!(map["api-type"], "covered by api-status-pattern");
        assert_eq!(map["api-id"], "exemplars are generated migrations");
        // Byte-stable: BTreeMap keys serialise sorted.
        let text = std::fs::read_to_string(dir.path().join(".claude/scan-declined.json")).unwrap();
        assert!(text.find("api-id").unwrap() < text.find("api-type").unwrap());
    }

    #[test]
    fn unparseable_store_degrades_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::write(dir.path().join(".claude/scan-declined.json"), "not json").unwrap();
        assert!(declined(dir.path()).is_empty(), "fail-open, never a panic");
    }
}
